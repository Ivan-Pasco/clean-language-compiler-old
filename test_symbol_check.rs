use clean_language_compiler::resolver::GlobalSymbolTable;

fn main() {
    let mut symbol_table = GlobalSymbolTable::new();
    symbol_table.register_builtin_functions();
    
    // Check if our compare functions are registered
    let test_names = vec![
        "compare.integer.greaterThan",
        "compare.integer.equal",
        "conditional.integer",
        "logical.and",
    ];
    
    for name in test_names {
        if let Some(symbol_id) = symbol_table.lookup_symbol(name) {
            println!("✅ Found '{}' with SymbolId({:?})", name, symbol_id);
            if let Some(symbol) = symbol_table.get_symbol(symbol_id) {
                println!("   Symbol info: {:?}", symbol.kind);
            }
        } else {
            println!("❌ NOT FOUND: '{}'", name);
        }
    }
}
