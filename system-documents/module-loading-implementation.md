# Module Loading Implementation

**Date:** October 8, 2025
**Status:** ✅ Complete
**Related:** codebase-cleanup-report.md

## Overview

Successfully implemented module loading functionality in both module resolution systems, enabling the compiler to load and parse external `.cln` module files during compilation.

## Implementation

### 1. HIR-Level Module Loading (resolver/module_resolver.rs)

**Location:** `src/resolver/module_resolver.rs:276-356`

This implementation is used by **Stage 4 (Name and Module Resolution)** of the 7-stage pipeline.

**Implementation Details:**

```rust
pub fn load_module_hir(&mut self, module_name: &str) -> Result<&HirProgram, CompilerError> {
    if let Some(module) = self.modules.get_mut(module_name) {
        if module.hir.is_none() {
            // 1. Read file from module.file_path
            let source = fs::read_to_string(&module.file_path)?;

            // 2. Tokenize (Stage 1)
            let source_code = SourceCode::new(source, path);
            let mut lexer = SpecificationLexer::new(&source_code);
            let tokens = lexer.tokenize()?;

            // 3. Parse (Stage 2)
            let mut parser = SpecificationParser::new(tokens, path);
            let ast = parser.parse_program()?;

            // 4. Build HIR (Stage 3)
            let mut hir_builder = HirBuilder::new();
            let hir_result = hir_builder.build_hir(ast)?;

            // 5. Extract exports (functions with indices as SymbolIds)
            let mut exports = HashMap::new();
            for (idx, func) in hir_result.hir.functions.iter().enumerate() {
                exports.insert(func.name.clone(), SymbolId(idx));
            }

            // 6. Extract dependencies (imported modules)
            let deps: Vec<String> = hir_result.hir.imports
                .iter()
                .map(|i| i.module_name.clone())
                .collect();

            // 7. Store HIR and metadata
            module.hir = Some(hir_result.hir);
            module.exports = exports;
            module.dependencies = deps;
        }

        Ok(module.hir.as_ref().unwrap())
    } else {
        Err(/* module not found */)
    }
}
```

**Pipeline Integration:**
- Used by: `Resolver::resolve()` in Stage 4
- Input: Module name (string)
- Output: Loaded HIR with exports and dependencies
- Caching: HIR stored in `LoadedModule::hir` field (lazy loading)

### 2. AST-Level Module Loading (module/mod.rs)

**Location:** `src/module/mod.rs:195-274`

This implementation is used by legacy code (SemanticAnalyzer, test runners, etc.) that works at AST level.

**Implementation Details:**

```rust
fn load_module(&mut self, module_name: &str) -> Result<Module, CompilerError> {
    // Check cache first
    if let Some(cached) = self.module_cache.get(module_name) {
        return Ok(cached.clone());
    }

    // Find module file
    let module_path = self.find_module_file(module_name)?;

    // Read source
    let source = fs::read_to_string(&module_path)?;

    // Stage 1: Tokenize
    let source_code = SourceCode::new(source, path);
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer.tokenize()?;

    // Stage 2: Parse to AST
    let mut parser = SpecificationParser::new(tokens, path);
    let program = parser.parse_program()?;

    // Extract exports (public functions and classes)
    let mut functions = HashMap::new();
    for func in &program.functions {
        if func.visibility == Visibility::Public {
            functions.insert(func.name.clone(), func.clone());
        }
    }

    let mut classes = HashMap::new();
    for class in &program.classes {
        classes.insert(class.name.clone(), class.clone());
    }

    let exports = ModuleExports { functions, classes, types: HashMap::new() };

    // Create and cache module
    let module = Module { name, file_path, program, exports };
    self.module_cache.insert(name.clone(), module.clone());

    Ok(module)
}
```

**Usage:**
- Used by: SemanticAnalyzer (legacy), test runners, cln binary
- Input: Module name (string)
- Output: Loaded AST Program with exports
- Caching: Module cached in `module_cache` HashMap

## Architecture

### Two Module Resolution Systems

The compiler has two separate module resolution systems due to its dual pipeline architecture:

```
┌─────────────────────────────────────────────────────────┐
│                   Active 7-Stage Pipeline               │
├─────────────────────────────────────────────────────────┤
│ Stage 1: Lexing                                         │
│ Stage 2: Parsing → AST                                  │
│ Stage 3: AST → HIR                                      │
│ Stage 4: HIR Resolver → uses resolver::ModuleResolver   │ ← HIR-level
│ Stage 5: Type Checking → TAST                           │
│ Stage 6: TAST → MIR                                     │
│ Stage 7: MIR → WASM                                     │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│              Legacy Pipeline (Being Deprecated)         │
├─────────────────────────────────────────────────────────┤
│ Lexing → Parsing → AST                                  │
│ SemanticAnalyzer → uses module::ModuleResolver          │ ← AST-level
│ (rest of pipeline deprecated)                           │
└─────────────────────────────────────────────────────────┘
```

### Module Search Paths

Both systems use similar search paths:

**resolver::ModuleResolver** (HIR-level):
```rust
search_paths: vec![
    PathBuf::from("./"),
    PathBuf::from("./lib/"),
    PathBuf::from("./stdlib/"),
]
```

**module::ModuleResolver** (AST-level):
```rust
module_paths: vec![
    PathBuf::from("./"),
    PathBuf::from("./modules/"),
    PathBuf::from("./lib/"),
    PathBuf::from("./stdlib/"),
]
```

### File Discovery

Both implementations search for modules with these patterns:

1. `{module_name}.cln`
2. `{module_name}/mod.cln`
3. `{module_name}/index.cln` (module::ModuleResolver only)

Additional extensions searched by module::ModuleResolver:
- `.clean` (legacy extension)
- `.cln` (current extension)

## Benefits

### 1. Token-Driven Parsing
Both implementations use the new token-driven parser:
```
Module Source → SpecificationLexer → TokenStream → SpecificationParser → AST/HIR
```

No source reconstruction or legacy parsing is used.

### 2. Lazy Loading
Modules are only loaded when first requested:
- HIR-level: Stored in `LoadedModule::hir` (Option)
- AST-level: Cached in `module_cache` HashMap

### 3. Export Discovery
**HIR-level exports:**
- Functions indexed by position
- SymbolId mapping for resolution

**AST-level exports:**
- Public functions (Visibility::Public)
- All classes
- Type exports (placeholder for future)

### 4. Dependency Tracking
HIR-level resolver tracks:
- Module dependencies (for cycle detection)
- Dependency order (topological sort)
- Recursive import resolution

## Testing

### Test Module Creation

Create a simple test module:

**File:** `./lib/Math.cln`
```clean
function add(a, b) returns integer
    return a + b

function multiply(a, b) returns integer
    return a * b
```

### Import Example

**File:** `main.cln`
```clean
import: Math

start()
    result = Math.add(5, 3)
    print(result)
```

### Expected Behavior

1. Parser encounters `import: Math`
2. Resolver calls `load_module_hir("Math")`
3. System searches paths for `Math.cln`
4. Found at `./lib/Math.cln`
5. Module is tokenized, parsed, and HIR built
6. Exports extracted: `{add: SymbolId(0), multiply: SymbolId(1)}`
7. Module cached for future imports
8. Symbol `Math.add` resolved to function in loaded module

## Known Limitations

### 1. Symbol Resolution
Current implementation uses function index as SymbolId. A proper symbol table should:
- Generate unique IDs across modules
- Handle classes, types, and constants
- Support nested scopes and qualified names

### 2. Circular Dependency Detection
HIR-level resolver has cycle detection implemented.
AST-level resolver does not check for cycles (legacy system).

### 3. Type Exports
Currently a placeholder. Future implementation should:
- Export type aliases
- Export interface definitions
- Support generic type parameters

### 4. Visibility Modifiers
AST-level: Only exports `Visibility::Public` functions
HIR-level: Exports all functions (visibility not yet enforced)

## Future Enhancements

1. **Remove AST-level ModuleResolver** when SemanticAnalyzer is fully deprecated
2. **Implement proper symbol tables** with cross-module unique IDs
3. **Add visibility enforcement** at HIR level
4. **Support re-exports** (e.g., `export: Math.sqrt`)
5. **Package manager integration** for third-party modules
6. **Module versioning** and compatibility checks

## Related Files

- `src/resolver/module_resolver.rs` - HIR-level module loading
- `src/module/mod.rs` - AST-level module loading (legacy)
- `src/resolver/mod.rs` - Stage 4 resolution orchestration
- `src/semantic/mod.rs` - Legacy semantic analyzer
- `src/lib.rs` - 7-stage pipeline integration

## Status

✅ **HIR-level module loading:** Implemented and integrated with Stage 4 resolver
✅ **AST-level module loading:** Implemented for legacy compatibility
✅ **Token-driven parsing:** Both systems use SpecificationParser
✅ **Export extraction:** Basic function exports working
✅ **Dependency tracking:** Implemented in HIR-level resolver

⏳ **Pending:** Symbol table integration, type exports, visibility enforcement
