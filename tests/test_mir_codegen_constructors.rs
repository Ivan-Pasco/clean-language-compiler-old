//! `MirCodeGenerator::new()` (used by `Default` and tests) is the Server-target
//! generator the compile pipeline builds with `with_target`: both produce the
//! same module from the same MIR.

use clean_language_compiler::codegen::mir_codegen::MirCodeGenerator;
use clean_language_compiler::mir::mir_types::{
    BasicBlockId, MirBasicBlock, MirFunction, MirFunctionAttributes, MirProgram, MirTerminator,
    MirType,
};
use clean_language_compiler::resolver::SymbolId;
use clean_language_compiler::CompilationTarget;
use std::collections::{BTreeMap, HashMap};

fn empty_start_program() -> MirProgram {
    let start = SymbolId(5001);
    let block_id = BasicBlockId(0);
    let block = MirBasicBlock {
        id: block_id,
        label: None,
        instructions: vec![],
        terminator: MirTerminator::Return { value: None },
        location: Default::default(),
    };
    let function = MirFunction {
        symbol_id: start,
        name: "start".to_string(),
        parameters: vec![],
        return_type: MirType::Void,
        blocks: HashMap::from([(block_id, block)]),
        entry_block: block_id,
        locals: HashMap::new(),
        next_value_id: 0,
        next_block_id: 1,
        if_merges: Default::default(),
        attributes: MirFunctionAttributes {
            entry_point: true,
            exported: true,
        },
        location: Default::default(),
    };
    MirProgram {
        functions: BTreeMap::from([(start, function)]),
        globals: BTreeMap::new(),
        string_pool: vec![],
        entry_point: Some(start),
        symbol_name_map: BTreeMap::from([(start, "start".to_string())]),
        externals: vec![],
        test_functions: vec![],
        class_ids: BTreeMap::new(),
        capability_dispatch: BTreeMap::new(),
        class_json_trees: BTreeMap::new(),
        instance_conversions: BTreeMap::new(),
    }
}

#[test]
fn new_is_the_server_target_generator() {
    let from_new = MirCodeGenerator::new()
        .generate(empty_start_program())
        .expect("generates");
    let from_target = MirCodeGenerator::with_target(CompilationTarget::Server)
        .generate(empty_start_program())
        .expect("generates");
    let from_default = MirCodeGenerator::default()
        .generate(empty_start_program())
        .expect("generates");
    assert_eq!(from_new.wasm_bytes, from_target.wasm_bytes);
    assert_eq!(from_default.wasm_bytes, from_target.wasm_bytes);
    wasmparser::validate(&from_new.wasm_bytes).expect("valid module");
}
