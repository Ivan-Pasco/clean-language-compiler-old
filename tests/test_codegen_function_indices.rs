//! Function indices the code generator hands out are always real.
//!
//! - A function reference to a symbol with no compiled function is a codegen
//!   error. It used to load index 0, a handle to whatever function came first.
//! - A tree-shaken import has no index. `register_import_function` used to
//!   return `u32::MAX` for it, and every caller had to remember to check.

use clean_language_compiler::codegen::mir_codegen::MirCodeGenerator;
use clean_language_compiler::codegen::CodeGenerator;
use clean_language_compiler::mir::mir_types::{
    BasicBlockId, MirBasicBlock, MirFunction, MirFunctionAttributes, MirInstruction, MirLocal,
    MirOperand, MirOperation, MirProgram, MirTerminator, MirType, ValueId,
};
use clean_language_compiler::resolver::SymbolId;
use clean_language_compiler::types::WasmType;
use std::collections::{BTreeMap, HashMap, HashSet};

/// `start` stores a reference to `missing` (a symbol with no function) in a
/// local.
fn program_referencing(missing: MirOperand) -> MirProgram {
    let start_symbol = SymbolId(5001);
    let block_id = BasicBlockId(0);
    let handle = ValueId(0);
    let block = MirBasicBlock {
        id: block_id,
        label: None,
        instructions: vec![MirInstruction {
            dest: Some(handle),
            operation: MirOperation::Copy { source: missing },
            location: Default::default(),
        }],
        terminator: MirTerminator::Return { value: None },
        location: Default::default(),
    };
    let mut locals = HashMap::new();
    locals.insert(
        handle,
        MirLocal {
            name: Some("handle".to_string()),
            local_type: MirType::I32,
            is_mutable: true,
            location: Default::default(),
        },
    );
    let start = MirFunction {
        symbol_id: start_symbol,
        name: "start".to_string(),
        parameters: vec![],
        return_type: MirType::Void,
        blocks: HashMap::from([(block_id, block)]),
        entry_block: block_id,
        locals,
        next_value_id: 1,
        next_block_id: 1,
        if_merges: Default::default(),
        attributes: MirFunctionAttributes {
            entry_point: true,
            exported: true,
        },
        location: Default::default(),
    };
    MirProgram {
        functions: BTreeMap::from([(start_symbol, start)]),
        globals: BTreeMap::new(),
        string_pool: vec![],
        entry_point: Some(start_symbol),
        symbol_name_map: BTreeMap::from([(start_symbol, "start".to_string())]),
        externals: vec![],
        test_functions: vec![],
        class_ids: BTreeMap::new(),
        capability_dispatch: BTreeMap::new(),
        class_json_trees: BTreeMap::new(),
        instance_conversions: BTreeMap::new(),
    }
}

fn codegen_error(program: MirProgram) -> String {
    match MirCodeGenerator::new().generate(program) {
        Ok(_) => panic!("codegen must reject a reference to a function that was never compiled"),
        Err(errors) => format!("{errors:?}"),
    }
}

#[test]
fn reference_to_uncompiled_function_is_a_codegen_error() {
    let error = codegen_error(program_referencing(MirOperand::Function(SymbolId(424_242))));
    assert!(error.contains("no compiled function"), "{error}");
}

#[test]
fn named_reference_to_uncompiled_function_is_a_codegen_error() {
    let error = codegen_error(program_referencing(MirOperand::NamedFunction {
        name: "not_compiled".to_string(),
        symbol_id: SymbolId(424_242),
    }));
    assert!(error.contains("not_compiled"), "{error}");
}

#[test]
fn tree_shaken_import_has_no_index() {
    let mut generator = CodeGenerator::new();
    generator.set_reachable_imports(HashSet::from(["_db_query".to_string()]));
    let shaken = generator
        .register_import_function(
            "env",
            "http_get",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )
        .unwrap();
    assert_eq!(shaken, None);
    assert_eq!(generator.get_function_index("http_get"), None);

    let kept = generator
        .register_import_function("env", "_db_query", &[WasmType::I32], Some(WasmType::I32))
        .unwrap();
    assert_eq!(kept, Some(0));
    assert_eq!(generator.get_function_index("_db_query"), Some(0));
}
