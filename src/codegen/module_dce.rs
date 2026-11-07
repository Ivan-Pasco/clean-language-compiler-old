//! Remove the functions a finished module can never call.
//!
//! The code generator registers every runtime helper (string, list, JSON,
//! validator, ...) before it knows what the program uses, so a `print("hi")`
//! module carried 500+ functions. This pass runs on the encoded module: it
//! walks the call graph from the module's roots and drops every defined
//! function nothing reaches, renumbering the rest.
//!
//! Imports are kept: which host functions a module imports is decided when
//! they are registered (the Import Minimality Rule gates in
//! `register_import_function`), and hosts and tooling read the import list.
//!
//! Roots: exported functions, functions placed in a table by an element
//! segment, and the start function. Edges: `call`, `return_call` and
//! `ref.func`. `call_indirect` goes through the table, whose entries are
//! roots.
//!
//! Table slots are not renumbered: a function value is its slot in
//! `__indirect_function_table` (`slot N holds function N`, see
//! `finalize_module`), so element segments keep their offsets and only the
//! function index stored in each slot changes.
//!
//! The pass handles the module shapes the code generator emits (function
//! imports only, active function-index element segments with constant
//! offsets, no function references in global initializers). Anything else is
//! returned unchanged.

use std::ops::Range;

use wasm_encoder::{
    ConstExpr, ElementSection, Elements, EntityType, ExportKind, ExportSection, FunctionSection,
    ImportSection, RawSection, StartSection,
};
use wasmparser::{ElementItems, ElementKind, ExternalKind, Operator, Parser, Payload, TypeRef};

use crate::error::CompilerError;

/// A function index operand inside a function body.
struct IndexOperand {
    /// Byte range of the operand's LEB128 encoding in the module.
    range: Range<usize>,
    function_index: u32,
}

struct Body {
    range: Range<usize>,
    operands: Vec<IndexOperand>,
}

struct ElementSegment {
    table_index: u32,
    offset: i32,
    functions: Vec<u32>,
}

/// A section of the input, in input order.
enum Section {
    Raw(u8, Range<usize>),
    Import,
    Function,
    Export,
    Start,
    Element,
    Code,
}

struct ModuleInfo<'a> {
    sections: Vec<Section>,
    imports: Vec<(&'a str, &'a str, u32)>,
    local_types: Vec<u32>,
    exports: Vec<(&'a str, ExternalKind, u32)>,
    start: Option<u32>,
    elements: Vec<ElementSegment>,
    bodies: Vec<Body>,
}

fn invalid(error: wasmparser::BinaryReaderError) -> CompilerError {
    CompilerError::codegen_error(
        format!("dead-function removal could not read the generated module: {error}"),
        None,
        None,
    )
}

/// Read the module, or `None` when it has a shape this pass does not handle.
fn read(wasm: &[u8]) -> Result<Option<ModuleInfo<'_>>, CompilerError> {
    let mut info = ModuleInfo {
        sections: Vec::new(),
        imports: Vec::new(),
        local_types: Vec::new(),
        exports: Vec::new(),
        start: None,
        elements: Vec::new(),
        bodies: Vec::new(),
    };
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(invalid)?;
        match payload {
            Payload::Version { .. } | Payload::End(_) => {}
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.map_err(invalid)?;
                    let TypeRef::Func(type_index) = import.ty else {
                        return Ok(None);
                    };
                    info.imports.push((import.module, import.name, type_index));
                }
                info.sections.push(Section::Import);
            }
            Payload::FunctionSection(reader) => {
                for type_index in reader {
                    info.local_types.push(type_index.map_err(invalid)?);
                }
                info.sections.push(Section::Function);
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(invalid)?;
                    info.exports.push((export.name, export.kind, export.index));
                }
                info.sections.push(Section::Export);
            }
            Payload::StartSection { func, .. } => {
                info.start = Some(func);
                info.sections.push(Section::Start);
            }
            Payload::ElementSection(reader) => {
                for element in reader {
                    let element = element.map_err(invalid)?;
                    let ElementKind::Active {
                        table_index,
                        offset_expr,
                    } = element.kind
                    else {
                        return Ok(None);
                    };
                    let mut offset_reader = offset_expr.get_operators_reader();
                    let Operator::I32Const { value: offset } =
                        offset_reader.read().map_err(invalid)?
                    else {
                        return Ok(None);
                    };
                    let ElementItems::Functions(functions) = element.items else {
                        return Ok(None);
                    };
                    let functions = functions
                        .into_iter()
                        .collect::<Result<Vec<u32>, _>>()
                        .map_err(invalid)?;
                    info.elements.push(ElementSegment {
                        table_index: table_index.unwrap_or(0),
                        offset,
                        functions,
                    });
                }
                info.sections.push(Section::Element);
            }
            Payload::GlobalSection(ref reader) => {
                for global in reader.clone() {
                    let global = global.map_err(invalid)?;
                    let mut operators = global.init_expr.get_operators_reader();
                    while !operators.eof() {
                        if let Operator::RefFunc { .. } = operators.read().map_err(invalid)? {
                            return Ok(None);
                        }
                    }
                }
                let (id, range) = payload.as_section().expect("a global section is a section");
                info.sections.push(Section::Raw(id, range));
            }
            Payload::CodeSectionStart { .. } => info.sections.push(Section::Code),
            Payload::CodeSectionEntry(body) => {
                let range = body.range();
                let mut operators = body.get_operators_reader().map_err(invalid)?;
                let mut operands = Vec::new();
                while !operators.eof() {
                    let (operator, offset) = operators.read_with_offset().map_err(invalid)?;
                    let function_index = match operator {
                        Operator::Call { function_index }
                        | Operator::ReturnCall { function_index }
                        | Operator::RefFunc { function_index } => function_index,
                        _ => continue,
                    };
                    // Each of these opcodes is one byte, followed by the index.
                    operands.push(IndexOperand {
                        range: offset + 1..operators.original_position(),
                        function_index,
                    });
                }
                info.bodies.push(Body { range, operands });
            }
            other => match other.as_section() {
                Some((id, range)) => info.sections.push(Section::Raw(id, range)),
                None => return Ok(None),
            },
        }
    }
    if info.local_types.len() != info.bodies.len() {
        return Ok(None);
    }
    Ok(Some(info))
}

fn write_u32_leb128(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The module without the functions nothing can call.
pub(crate) fn remove_unreachable_functions(wasm: Vec<u8>) -> Result<Vec<u8>, CompilerError> {
    let Some(info) = read(&wasm)? else {
        return Ok(wasm);
    };
    let import_count = info.imports.len() as u32;
    let function_count = import_count + info.local_types.len() as u32;

    // Reachability from the roots.
    let mut reached = vec![false; function_count as usize];
    reached[..import_count as usize].fill(true);
    let mut worklist: Vec<u32> = info
        .exports
        .iter()
        .filter(|(_, kind, _)| *kind == ExternalKind::Func)
        .map(|(_, _, index)| *index)
        .chain(
            info.elements
                .iter()
                .flat_map(|e| e.functions.iter().copied()),
        )
        .chain(info.start)
        .collect();
    while let Some(index) = worklist.pop() {
        let Some(seen) = reached.get_mut(index as usize) else {
            return Ok(wasm);
        };
        if std::mem::replace(seen, true) || index < import_count {
            continue;
        }
        let body = &info.bodies[(index - import_count) as usize];
        worklist.extend(body.operands.iter().map(|o| o.function_index));
    }

    // New index of every kept function (imports keep theirs; order preserved).
    let mut new_index = vec![u32::MAX; function_count as usize];
    let mut next = 0;
    for (old, kept) in reached.iter().enumerate() {
        if *kept {
            new_index[old] = next;
            next += 1;
        }
    }
    let remap = |old: u32| new_index[old as usize];

    let mut module = wasm_encoder::Module::new();
    for section in &info.sections {
        match section {
            Section::Raw(id, range) => {
                module.section(&RawSection {
                    id: *id,
                    data: &wasm[range.clone()],
                });
            }
            Section::Import => {
                let mut imports = ImportSection::new();
                for (module_name, name, type_index) in &info.imports {
                    imports.import(module_name, name, EntityType::Function(*type_index));
                }
                module.section(&imports);
            }
            Section::Function => {
                let mut functions = FunctionSection::new();
                for (local, type_index) in info.local_types.iter().enumerate() {
                    if reached[import_count as usize + local] {
                        functions.function(*type_index);
                    }
                }
                module.section(&functions);
            }
            Section::Export => {
                let mut exports = ExportSection::new();
                for (name, kind, index) in &info.exports {
                    let (kind, index) = match kind {
                        ExternalKind::Func => (ExportKind::Func, remap(*index)),
                        ExternalKind::Table => (ExportKind::Table, *index),
                        ExternalKind::Memory => (ExportKind::Memory, *index),
                        ExternalKind::Global => (ExportKind::Global, *index),
                        ExternalKind::Tag => (ExportKind::Tag, *index),
                    };
                    exports.export(name, kind, index);
                }
                module.section(&exports);
            }
            Section::Start => {
                if let Some(start) = info.start {
                    module.section(&StartSection {
                        function_index: remap(start),
                    });
                }
            }
            Section::Element => {
                let mut elements = ElementSection::new();
                for segment in &info.elements {
                    let functions: Vec<u32> = segment
                        .functions
                        .iter()
                        .map(|index| remap(*index))
                        .collect();
                    elements.active(
                        Some(segment.table_index),
                        &ConstExpr::i32_const(segment.offset),
                        Elements::Functions(&functions),
                    );
                }
                module.section(&elements);
            }
            Section::Code => {
                let mut code = wasm_encoder::CodeSection::new();
                for (local, body) in info.bodies.iter().enumerate() {
                    if !reached[import_count as usize + local] {
                        continue;
                    }
                    let mut bytes = Vec::with_capacity(body.range.len());
                    let mut cursor = body.range.start;
                    for operand in &body.operands {
                        bytes.extend_from_slice(&wasm[cursor..operand.range.start]);
                        write_u32_leb128(&mut bytes, remap(operand.function_index));
                        cursor = operand.range.end;
                    }
                    bytes.extend_from_slice(&wasm[cursor..body.range.end]);
                    code.raw(&bytes);
                }
                module.section(&code);
            }
        }
    }
    Ok(module.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wat(source: &str) -> Vec<u8> {
        wat::parse_str(source).expect("valid wat")
    }

    fn function_names(wasm: &[u8]) -> (Vec<String>, usize) {
        let mut imports = Vec::new();
        let mut locals = 0;
        for payload in Parser::new(0).parse_all(wasm) {
            match payload.unwrap() {
                Payload::ImportSection(reader) => {
                    for import in reader {
                        imports.push(import.unwrap().name.to_string());
                    }
                }
                Payload::FunctionSection(reader) => locals = reader.count() as usize,
                _ => {}
            }
        }
        (imports, locals)
    }

    #[test]
    fn drops_unreached_functions_and_keeps_imports() {
        let input = wat(r#"
            (module
              (import "env" "used" (func $used (param i32)))
              (import "env" "unused" (func $unused))
              (table 8 8 funcref)
              (elem (i32.const 5) $by_table)
              (func $dead (call $unused))
              (func $helper (param i32) (call $used (local.get 0)))
              (func $by_table)
              (func $main (export "main") (call $helper (i32.const 1)))
              (memory (export "memory") 1))
        "#);
        let output = remove_unreachable_functions(input).unwrap();
        wasmparser::validate(&output).expect("valid output");
        let (imports, locals) = function_names(&output);
        assert_eq!(imports, vec!["used", "unused"]);
        assert_eq!(locals, 3, "helper, by_table and main remain");
    }

    #[test]
    fn calls_and_table_slots_still_reach_the_same_functions() {
        let input = wat(r#"
            (module
              (type $t (func (result i32)))
              (table 8 8 funcref)
              (elem (i32.const 6) $seven)
              (func $dead (result i32) (i32.const 0))
              (func $seven (result i32) (i32.const 7))
              (func $call (export "direct") (result i32) (call $seven))
              (func (export "indirect") (result i32) (call_indirect (type $t) (i32.const 6))))
        "#);
        let output = remove_unreachable_functions(input).unwrap();
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &output).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        for name in ["direct", "indirect"] {
            let f = instance
                .get_typed_func::<(), i32>(&mut store, name)
                .unwrap();
            assert_eq!(f.call(&mut store, ()).unwrap(), 7, "{name}");
        }
    }
}
