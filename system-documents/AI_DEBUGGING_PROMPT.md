# Clean Language Compiler - Comprehensive Debugging Analysis Request

## Project Context

I am working on a Rust-based compiler for Clean Language, a type-safe programming language that compiles to WebAssembly. The compiler uses a multi-stage pipeline: Lexer → Parser (Pest-based) → HIR → Resolver → Type Checker → MIR → WASM Codegen.

**Current Status**: 181/285 tests passing (63.5%)
**Goal**: Achieve 100% test pass rate (285/285)
**Gap**: 104 failing tests (36.5%)

## Architecture Overview

### Compilation Pipeline (7 Stages)
1. **Lexical Analysis**: Tokenization
2. **Parsing**: Pest grammar → AST (Abstract Syntax Tree)
3. **HIR Building**: AST → HIR (High-level Intermediate Representation)
4. **Resolution**: Symbol resolution and scope management
5. **Type Checking**: Type inference with constraint solving
6. **MIR Generation**: HIR → MIR (Medium-level IR with optimizations)
7. **WASM Code Generation**: MIR → WebAssembly binary

### Key Components

**Parser** (`src/parser/`):
- Uses Pest parser generator with `grammar.pest` PEG grammar
- Has dual parsing paths (CRITICAL ISSUE):
  - Grammar-based parser: Handles standalone functions correctly
  - Preprocessor (`src/parser/preprocessor.rs`): Handles class methods differently
- Grammar supports default parameters: `parameter = { parameter_type? ~ " "* ~ parameter_name ~ (" "* ~ "=" ~ " "* ~ logical_expression)? }`

**Type System**:
- Strong static typing with inference
- Primitive types: `integer`, `number`, `string`, `boolean`
- Complex types: `Array<T>`, `Matrix<T>`, `Class`
- Type inference uses constraint solving with unification

**HIR (High-level IR)** (`src/hir/mod.rs`):
```rust
pub struct HirFunction {
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: Option<HirType>,
    pub body: HirBlock,
    // ...
}

pub struct HirParameter {
    pub name: String,
    pub param_type: HirType,
    pub location: SourceLocation,
    // NOTE: No default_value field currently
}
```

**Resolved HIR** (`src/resolver/mod.rs`):
```rust
pub struct ResolvedHirParameter {
    pub name: String,
    pub symbol_id: SymbolId,
    pub param_type: HirType,
    pub default_value: Option<ResolvedHirExpression>, // Field exists but set to None
    pub is_variadic: bool,
    pub location: SourceLocation,
}
```

## Critical Issues Analysis

### Issue 1: Class Method Return Types (12 failing tests)

**Example Failure**: `tests/cln/debug/test_constructor_base_minimal.cln`

**Test Code**:
```clean
class Child is Base
    functions:
        string first()
            return "first"

        string second()
            return "second"
```

**Error**: `Type error: Cannot unify types: string and null`

**Detailed Root Cause Analysis**:

#### Evidence Chain
1. **Grammar Level** (`src/parser/grammar.pest`):
   - Line 589: `class_body_item = { class_functions_block | constructor | class_field }`
   - Line 463-464:
     ```pest
     functions_block = { "functions" ~ ":" ~ indented_functions_block }
     class_functions_block = { "functions" ~ ":" ~ class_indented_functions_block }
     ```
   - Grammar correctly defines and emits `Rule::class_functions_block`

2. **Parser Matching** (`src/parser/parser_impl.rs`):
   - Line 1263: Parser only matches `Rule::functions_block`:
     ```rust
     Rule::functions_block => {
         let block_methods = parse_functions_block(class_item)?;
         methods.extend(block_methods);
     }
     ```
   - `Rule::class_functions_block` is NOT matched → falls through to default case
   - Result: Class methods not parsed by grammar-based parser

3. **Function Parsing** (`src/parser/parser_impl.rs:1591-1640`):
   - Line 1603: Handles return types for standalone functions:
     ```rust
     Rule::function_type => {
         let parsed_type = parse_type(item)?;
         return_type = Some(parsed_type.clone());
     }
     ```
   - Grammar emits `Rule::function_return_type` for class methods (verified with debug output)
   - Parser does NOT match `Rule::function_return_type` → return type ignored

4. **HIR Building** (`src/hir/hir_builder.rs:156-197`):
   - Line 179: Creates `HirFunction` with return type:
     ```rust
     return_type: ast_function.return_type.as_ref().map(|t| self.build_type(t))
     ```
   - If `ast_function.return_type` is `None`, HIR function has `return_type: None`

5. **Type Inference** (`src/typechecker/type_inference.rs:938-962`):
   - Line 940-945: Registers function signature:
     ```rust
     let return_type = if let Some(ref rt) = function.return_type {
         self.hir_type_to_concrete(rt)
     } else {
         ConcreteType::Null  // Functions without explicit return type
     };
     ```
   - Class methods arrive with `return_type: None` → typed as `ConcreteType::Null`
   - Method body returns `string` → Unification error: "Cannot unify string and null"

#### Preprocessor Discovery

**File**: `src/parser/preprocessor.rs`

Investigation revealed that class methods may be parsed through a preprocessor path that strips return type information. The preprocessor is mentioned in comments at:
- `src/resolver/resolver_impl.rs:219`: "Default parameters handled at AST level"
- Parser has dual paths: Grammar vs Preprocessor

**Critical Files to Examine**:
- `src/parser/preprocessor.rs`: How does it parse class methods?
- `src/parser/parser_impl.rs:1263`: Why doesn't it match `Rule::class_functions_block`?
- `src/parser/parser_impl.rs:1603`: Why doesn't it match `Rule::function_return_type`?

#### Failed Fix Attempt

**What Was Tried**:
```rust
// At src/parser/parser_impl.rs:1603
// Added Rule::function_return_type to the match
Rule::function_type | Rule::function_return_type => {
    let parsed_type = parse_type(item)?;
    return_type = Some(parsed_type.clone());
}
```

**Result**: CRITICAL REGRESSION
- Baseline: 209/285 (73.3%)
- After change: 139/285 (48.8%)
- Lost: 70 tests (-24.6%)

**Why It Failed**:
- This code path is also used for standalone functions
- Change affects function signature parsing globally
- Broke standalone function return type extraction
- Indicates that `Rule::function_type` and `Rule::function_return_type` have different semantics in different contexts

#### Specific Code Locations Needing Investigation

1. **Grammar Rules** (`src/parser/grammar.pest`):
   ```pest
   function_type = { type_ }
   function_return_type = { type_ }
   ```
   - Are these rules semantically different?
   - When is each rule emitted?

2. **Class Parsing** (`src/parser/parser_impl.rs:1200-1350`):
   - Line 1263: Add `Rule::class_functions_block` match?
   - How are class methods currently reaching `parse_functions_block`?

3. **Function Parsing** (`src/parser/parser_impl.rs:1591-1640`):
   - Line 1603: How to safely handle `Rule::function_return_type`?
   - Why does adding it break standalone functions?

4. **Preprocessor** (`src/parser/preprocessor.rs`):
   - Does preprocessor handle `functions:` blocks in classes?
   - Does it extract return types?
   - Can we add return type extraction here?

#### Questions Requiring Answers

1. **Grammar Semantics**: What is the difference between `function_type` and `function_return_type` in grammar rules?
2. **Parsing Flow**: What is the exact call path for parsing class methods vs standalone functions?
3. **Preprocessor Role**: What does `preprocessor.rs` do and when is it invoked?
4. **Safe Fix**: How can we extract return types for class methods without breaking standalone functions?
5. **Architecture**: Should we eliminate the preprocessor and unify on grammar-only parsing?

### Issue 2: Three-Level Namespace Method Calls (9 failing tests)

**Example Failure**: `tests/cln/debug/test_simple_chain.cln`

**Test Code**:
```clean
functions:
    void test()
        integer a = 10
        integer b = 5
        boolean result = compare.integer.greaterThan(a, b)  // Three-level call
        print("result")
```

**Error**: `Type error: Cannot unify types: null and boolean`

**Detailed Root Cause Analysis**:

#### Evidence Chain

1. **Grammar Level** (`src/parser/grammar.pest`):
   - Method calls parsed as chained property access
   - Grammar likely supports arbitrary chaining via recursion
   - Need to verify: How are multi-level property/method chains represented?

2. **AST Representation** (`src/ast/mod.rs`):
   - Expression enum should contain method call variants
   - Need to find: What variant represents `compare.integer.greaterThan(a, b)`?
   - Likely candidates:
     - `MethodCall { receiver, method, args }`
     - `PropertyAccess { object, property }`
     - Combination of both

3. **Parsing** (`src/parser/expression_parser.rs` or similar):
   - How are chained calls parsed?
   - Example: `compare.integer.greaterThan(a, b)` might parse as:
     - `MethodCall(PropertyAccess(PropertyAccess(compare, integer), greaterThan), [a, b])`
     - OR: `StaticMethodCall(compare.integer, greaterThan, [a, b])`
   - Need to trace: What AST structure is actually created?

4. **HIR Conversion** (`src/hir/mod.rs` and `src/hir/hir_builder.rs`):
   - How does AST method call convert to HIR expression?
   - HIR likely has: `HirExpression::StaticMethodCall { class_name, method_name, arguments }`
   - Problem: Only supports TWO levels (class.method), not THREE (namespace.class.method)

5. **Type Inference Failure** (`src/typechecker/type_inference.rs`):

   **Current Implementation** (Lines 2425-2460):
   ```rust
   fn infer_function_return_type(
       &self,
       function_symbol_id: SymbolId,
       arguments: &[TastExpression],
   ) -> Result<ConcreteType, CompilerError> {
       // ... handles regular function calls
   }
   ```

   **Static Method Type Inference** (Lines 2591-2666):
   ```rust
   fn infer_static_method_return_type(
       &mut self,
       class_name: &str,
       method_name: &str,
   ) -> Result<ConcreteType, CompilerError> {
       match (class_name, method_name) {
           // String static methods
           ("String", "fromCharCode") => Ok(ConcreteType::String),
           ("String", "concat") => Ok(ConcreteType::String),

           // Number static methods
           ("Number", "parseInt") => Ok(ConcreteType::Integer),
           ("Number", "parseFloat") => Ok(ConcreteType::Number),

           // Lines 2653-2663: Added namespace methods (NO EFFECT!)
           ("integer", "greaterThan") => Ok(ConcreteType::Boolean),
           ("number", "greaterThan") => Ok(ConcreteType::Boolean),
           // ... etc

           _ => Ok(ConcreteType::Unknown),
       }
   }
   ```

   **Why The Fix Failed**:
   - Added mappings like `("integer", "greaterThan")` at line 2656
   - This assumes `class_name = "integer"` and `method_name = "greaterThan"`
   - But for `compare.integer.greaterThan()`:
     - Actual `class_name` might be `"compare.integer"` (string concatenation)
     - OR it's not even reaching this function
   - Result: No improvement (still 209/285 tests passing)

6. **Expression Inference Entry Point** (`src/typechecker/type_inference.rs:1687-2350`):
   ```rust
   fn infer_expression(
       &mut self,
       expr: &ResolvedHirExpression,
   ) -> Result<TastExpression, CompilerError> {
       match expr {
           // ... many expression types
           ResolvedHirExpression::StaticMethodCall {
               class_name,
               method_name,
               arguments,
               location,
           } => {
               // Calls infer_static_method_return_type(class_name, method_name)
           }
           // Question: Is there a three-level variant?
       }
   }
   ```

#### Diagnostic Test Needed

To understand how three-level calls are represented, need to:

1. **Add Debug Output** to `infer_expression`:
   ```rust
   ResolvedHirExpression::StaticMethodCall { class_name, method_name, .. } => {
       eprintln!("DEBUG: StaticMethodCall - class='{}', method='{}'", class_name, method_name);
       // Continue processing...
   }
   ```

2. **Compile test file** `test_simple_chain.cln`:
   - Expected output: `DEBUG: StaticMethodCall - class='compare.integer', method='greaterThan'`
   - OR: `DEBUG: StaticMethodCall - class='compare', method='integer'` (wrong!)
   - OR: Different expression variant entirely

#### Specific Code Locations to Investigate

1. **AST Expression Variants** (`src/ast/mod.rs` - around Expression enum):
   ```rust
   pub enum Expression {
       // Find variants related to method calls
       MethodCall { ... },
       StaticMethodCall { ... },
       PropertyAccess { ... },
       // How are these nested for three-level calls?
   }
   ```

2. **Expression Parsing** (`src/parser/expression_parser.rs`):
   - Function that parses method calls
   - How does it handle chained property access?
   - Does it flatten `a.b.c()` or nest it?

3. **HIR Expression** (`src/hir/mod.rs`):
   ```rust
   pub enum HirExpression {
       StaticMethodCall {
           class_name: String,  // Is this "compare.integer" or just "compare"?
           method_name: String, // Is this "greaterThan" or "integer"?
           arguments: Vec<HirExpression>,
       },
       // Other variants?
   }
   ```

4. **Resolved HIR Expression** (`src/resolver/mod.rs`):
   ```rust
   pub enum ResolvedHirExpression {
       StaticMethodCall {
           class_name: String,
           method_name: String,
           arguments: Vec<ResolvedHirExpression>,
           location: SourceLocation,
       },
       // How does resolver handle three-level calls?
   }
   ```

#### Proposed Solutions (Need Verification)

**Option A: Extend Existing StaticMethodCall**
```rust
// In HIR/Resolved HIR:
StaticMethodCall {
    namespace: Option<String>,  // "compare"
    class_name: String,          // "integer"
    method_name: String,         // "greaterThan"
    arguments: Vec<...>,
}
```

**Option B: Add New ThreeLevelMethodCall Variant**
```rust
// In HIR Expression enum:
ThreeLevelMethodCall {
    namespace: String,    // "compare"
    class_name: String,   // "integer"
    method_name: String,  // "greaterThan"
    arguments: Vec<...>,
}
```

**Option C: Namespace Method Registry**
```rust
// In type inference:
fn infer_namespace_method_return_type(
    &mut self,
    namespace: &str,
    class_name: &str,
    method_name: &str,
) -> Result<ConcreteType, CompilerError> {
    match (namespace, class_name, method_name) {
        ("compare", "integer", "greaterThan") => Ok(ConcreteType::Boolean),
        ("compare", "integer", "lessThan") => Ok(ConcreteType::Boolean),
        ("compare", "integer", "equal") => Ok(ConcreteType::Boolean),
        // ... comprehensive mappings
    }
}
```

#### Questions Requiring Answers

1. **AST Structure**: How is `compare.integer.greaterThan(a, b)` currently represented in AST?
2. **Parsing**: Where in the parser code are multi-level property accesses handled?
3. **HIR Conversion**: How does the HIR builder convert three-level calls from AST?
4. **Current Representation**: What value does `class_name` have when processing `compare.integer.greaterThan`?
5. **Type Inference Entry**: Which `match` arm in `infer_expression` handles this call?
6. **Stdlib Organization**: How should namespace methods be organized in type registry?

### Issue 3: Default Parameters (3 failing tests)

**Example Failure**: `tests/cln/debug/test_simple_default_params.cln`

**Test Code**:
```clean
functions:
    string greet(string name = "World")
        return "Hello, " + name

start()
    string result = greet()  // Call with 0 arguments
    print(result)
```

**Error (Initial)**: `Type error: Function requires at least 1 arguments, got 0`

**Detailed Pipeline Analysis**:

#### Stage 1: Grammar ✓ WORKING
**File**: `src/parser/grammar.pest`
**Line 452**:
```pest
parameter = { parameter_type? ~ " "* ~ parameter_name ~ (" "* ~ "=" ~ " "* ~ logical_expression)? }
```
- Grammar correctly supports optional default value expression
- Syntax: `string name = "World"` parses successfully
- Default value is parsed as `logical_expression` (allows any expression)

#### Stage 2: AST ✓ WORKING
**File**: `src/ast/mod.rs`
**Lines 125-129**:
```rust
pub struct Parameter {
    pub name: String,
    pub type_: Type,
    pub default_value: Option<Expression>,  // ✓ Field exists
}
```
- AST correctly stores default value expressions
- `default_value: Option<Expression>` properly designed

#### Stage 3: Parser ✓ WORKING
**File**: `src/parser/parser_impl.rs`
**Lines 959-1007** (`parse_parameter` function):
```rust
fn parse_parameter(pair: Pair<Rule>) -> Result<Parameter, CompilerError> {
    let mut param_type = None;
    let mut param_name = String::new();
    let mut default_value = None;  // ✓ Variable declared

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::parameter_type => { ... }
            Rule::type_ => { param_type = Some(parse_type(inner)?); }
            Rule::identifier | Rule::parameter_name => {
                param_name = inner.as_str().to_string();
            }
            Rule::expression | Rule::logical_expression => {
                // ✓ Correctly extracts default value
                default_value = Some(crate::parser::expression_parser::parse_expression(inner)?);
            }
            _ => {}
        }
    }

    Ok(Parameter {
        name: param_name,
        type_: param_type.unwrap_or(Type::Any),
        default_value,  // ✓ Passed through to AST
    })
}
```
- Parser correctly extracts default value from grammar
- Converts to AST Expression
- **Verified with debug output**: `AST has default_value: true`

#### Stage 4: HIR ✗ DROPS DEFAULT VALUES
**File**: `src/hir/mod.rs`
**Lines 42-48** (BEFORE attempted fix):
```rust
pub struct HirParameter {
    pub name: String,
    pub param_type: HirType,
    pub location: SourceLocation,
    // ✗ NO default_value field!
}
```

**File**: `src/hir/hir_builder.rs`
**Lines 227-236** (build_parameter function):
```rust
fn build_parameter(&mut self, param: &Parameter) -> Result<HirParameter, CompilerError> {
    let param_type = self.build_type(&param.type_)?;

    Ok(HirParameter {
        name: param.name.clone(),
        param_type,
        location: SourceLocation::default(),
        // ✗ param.default_value is IGNORED - not converted to HIR!
    })
}
```

**Problem**: Default values are dropped during AST → HIR conversion

#### Stage 5: Resolver ✗ SETS default_value TO None
**File**: `src/resolver/mod.rs`
**Lines 59-66**:
```rust
pub struct ResolvedHirParameter {
    pub name: String,
    pub symbol_id: SymbolId,
    pub param_type: HirType,
    pub default_value: Option<ResolvedHirExpression>,  // ✓ Field exists
    pub is_variadic: bool,
    pub location: SourceLocation,
}
```

**File**: `src/resolver/resolver_impl.rs`
**Lines 215-222** (resolving function parameters):
```rust
resolved_parameters.push(ResolvedHirParameter {
    name: param.name.clone(),
    symbol_id: param_symbol_id,
    param_type: param.param_type.clone(),
    default_value: None,  // ✗ Hardcoded to None with comment: "Default parameters handled at AST level"
    is_variadic: false,
    location: param.location.clone(),
});
```

**Also at lines**: 353, 470, 528 (constructor and method parameters)
**Problem**: Even though `ResolvedHirParameter` has the field, it's explicitly set to `None`

#### Attempted Fix (Then Reverted)

**Changes Made**:

1. **HIR Parameter** - Added field:
   ```rust
   pub struct HirParameter {
       pub name: String,
       pub param_type: HirType,
       pub default_value: Option<HirExpression>,  // ✓ Added
       pub location: SourceLocation,
   }
   ```

2. **HIR Builder** - Convert default expression:
   ```rust
   fn build_parameter(&mut self, param: &Parameter) -> Result<HirParameter, CompilerError> {
       let param_type = self.build_type(&param.type_)?;

       let default_value = if let Some(ref default_expr) = param.default_value {
           Some(self.build_expression(default_expr)?)  // ✓ Convert AST expr to HIR
       } else {
           None
       };

       Ok(HirParameter {
           name: param.name.clone(),
           param_type,
           default_value,  // ✓ Include in HIR
           location: SourceLocation::default(),
       })
   }
   ```

3. **Resolver** - Preserve defaults (4 locations updated):
   ```rust
   let resolved_default = if let Some(ref default_expr) = param.default_value {
       Some(self.resolve_expression(default_expr)?)  // ✓ Resolve expression
   } else {
       None
   };

   resolved_parameters.push(ResolvedHirParameter {
       name: param.name.clone(),
       symbol_id: param_symbol_id,
       param_type: param.param_type.clone(),
       default_value: resolved_default,  // ✓ Pass through
       is_variadic: false,
       location: param.location.clone(),
   });
   ```

**Result After These Changes**:
- Compiled successfully ✓
- Debug output showed: `"0 required (without defaults)"` ✓
- Argument count validation passed ✓
- **NEW ERROR**: `Type error: Cannot unify types: null and string`

#### Stage 6: Type Checker ✗ INCOMPLETE IMPLEMENTATION

**File**: `src/typechecker/type_inference.rs`

**Function Signature Registration** (Lines 954-962):
```rust
// Track minimum required parameters (those without defaults)
let required_count = function
    .parameters
    .iter()
    .filter(|p| p.default_value.is_none())
    .count();

self.required_param_counts
    .insert(function.symbol_id, required_count);
```
- This part WORKS CORRECTLY ✓
- Properly counts only non-default parameters as required

**Argument Validation** (Lines 2377-2400):
```rust
let required_count = self
    .required_param_counts
    .get(&function_symbol_id)
    .copied()
    .unwrap_or(parameters.len());

// Validate argument count against required parameters
if arguments.len() < required_count {
    return Err(CompilerError::type_error(
        &format!("Function requires at least {} arguments, got {}", required_count, arguments.len()),
        // ...
    ));
}
```
- This part WORKS CORRECTLY ✓
- Uses `required_count` instead of total parameter count
- Allows calls with fewer arguments than total parameters

**Type Checking Arguments** (Lines 2404-2412):
```rust
// Check argument types match parameters (only for provided arguments)
if !is_generic_list_fn {
    for (param_type, arg) in parameters.iter().zip(arguments.iter()) {
        self.add_constraint(TypeConstraint::Equality {
            left: arg.expr_type.clone(),
            right: param_type.clone(),
            location: location.clone(),
        });
    }
}
```
- **PROBLEM**: Uses `zip` which only processes PROVIDED arguments ✗
- For call `greet()` with 0 args:
  - `parameters = [ConcreteType::String]` (1 parameter)
  - `arguments = []` (0 arguments)
  - `zip` produces 0 pairs → NO type checking happens
  - Function signature expects parameters, but no values provided
  - Return type inference sees uninitialized parameter → type error

**What's Missing**: Synthesizing default value expressions for missing arguments

#### The Core Problem: TAST Synthesis Required

**TAST** (Typed AST) in `src/typechecker/mod.rs`:
```rust
pub struct TastExpression {
    pub expr_type: ConcreteType,
    pub kind: TastExpressionKind,
}

pub enum TastExpressionKind {
    // Existing variants for user-written expressions
    Literal(LiteralValue),
    Variable(SymbolId),
    FunctionCall { ... },
    // ✗ No variant for "synthetic" compiler-generated expressions
}
```

**What Needs to Happen**:
1. When processing call `greet()`:
   - User provides: 0 arguments
   - Function requires: 1 parameter (with default)
   - Compiler must SYNTHESIZE: `greet("World")`

2. Default value synthesis requires:
   - Taking `default_value: Option<ResolvedHirExpression>` from parameter
   - Type checking the default expression (in parameter context)
   - Creating `TastExpression` for the default
   - Inserting into arguments list before code generation

3. Type checking default expressions:
   ```rust
   // Pseudo-code for what's needed:
   let mut complete_arguments = Vec::new();
   for (i, param) in function.parameters.iter().enumerate() {
       if i < arguments.len() {
           complete_arguments.push(arguments[i].clone());  // User-provided
       } else if let Some(ref default_expr) = param.default_value {
           let typed_default = self.infer_expression(default_expr)?;  // Type check default
           complete_arguments.push(typed_default);  // Compiler-synthesized
       } else {
           return Err("Missing required argument");
       }
   }
   // Type check complete_arguments against parameters
   ```

#### Specific Implementation Challenges

1. **Where to synthesize**: Type inference or code generation?
   - Type inference: Cleaner, maintains type safety
   - Code generation: Simpler, but loses type information

2. **Default expression scope**: Parameters can't reference other parameters
   - Need to validate: Default expressions are self-contained
   - No forward references to later parameters

3. **Evaluation timing**: Compile-time vs runtime
   - Compile-time: Only for literals (`"World"`, `42`, etc.)
   - Runtime: For computed defaults (`getTimestamp()`, etc.)

4. **TAST representation**:
   - Option A: Mark synthesized expressions with flag
   - Option B: Create separate `SynthesizedExpression` variant
   - Option C: Just treat them as normal expressions (simplest)

#### Questions Requiring Answers

1. **Architecture**: Should default value synthesis happen in type checker or code generator?
2. **TAST Design**: Do we need a special marker for compiler-synthesized expressions?
3. **Scope Rules**: How do we validate that default expressions don't reference other parameters?
4. **Evaluation**: Should we evaluate constant defaults at compile-time?
5. **Error Messages**: How do we provide good error messages for type errors in default values?
6. **Testing**: What edge cases need coverage (recursive defaults, complex expressions, etc.)?

### Issue 4: Error Handling - `onError` Syntax (10 failing tests)

**Example Failure**: `tests/cln/debug/test_onerror_simple.cln`

**Status**: Feature NOT implemented
**Grammar**: Already supports `onError` blocks:
```pest
on_error_expr = { base_expression ~ "onError" ~ base_expression }
on_error_block = { base_expression ~ "onError" ~ ":" ~ indented_block }
```

**Required Implementation**:
1. AST representation: `OnError { try_block, error_var, catch_block }`
2. HIR/MIR handling for error propagation
3. WASM code generation strategy (two options):
   - **Option A**: Result types `(result (tuple ...) (tuple i32))` - Recommended
   - **Option B**: WASM exception handling proposal (not yet standardized)

**Questions for Investigation**:
1. What is the cleanest AST representation for onError blocks?
2. Should error propagation use result types or exceptions?
3. How should error variables be scoped in HIR?
4. What WASM instruction sequences are needed for result-based error handling?

## Failure Breakdown by Category

```
Category Breakdown (104 failures):
  Class Methods (return types)     : 12 tests (11.5%)
  Three-Level Namespace Calls       :  9 tests ( 8.7%)
  Error Handling (onError)          : 10 tests ( 9.6%)
  Default Parameters                :  3 tests ( 2.9%)
  Integration Tests (compound)      : 15 tests (14.4%)
  Other (async, imports, etc.)      : 55 tests (52.9%)

High-Impact Fixes:
  1. Class method return types  → +12 tests (+4.2%)
  2. Error handling (onError)   → +10 tests (+3.5%)
  3. Three-level namespace      → + 9 tests (+3.2%)
  Total from top 3              → +31 tests (+10.9%)
```

## Previous Debugging Attempts

### Attempt 1: Class Method Return Type Extraction
**What**: Added `Rule::function_return_type` match to parser
**Location**: `src/parser/parser_impl.rs:1603`
**Result**: CRITICAL REGRESSION from 209/285 (73.3%) to 139/285 (48.8%) - LOST 70 tests!
**Why it failed**: Change affects standalone function parsing, breaking more than it fixes
**Lesson**: Grammar-based fix incompatible with dual parser architecture

### Attempt 2: Three-Level Method Type Inference
**What**: Added stdlib method types to `infer_static_method_return_type`
**Location**: `src/typechecker/type_inference.rs:2653-2663`
**Result**: NO IMPROVEMENT - still 209/285 (73.3%)
**Why it failed**: Three-level calls don't reach that code path
**Lesson**: AST/HIR structure doesn't support three-level namespace resolution

### Attempt 3: Default Parameter Full Implementation
**What**: Added default_value through entire pipeline (HIR → Resolver → Type Checker)
**Result**: PARTIAL SUCCESS
  - Argument count validation: ✓ Works
  - Type checking: ✗ Fails with "Cannot unify types"
**Why stopped**: Requires TAST modifications for value synthesis
**Lesson**: Default parameters need comprehensive design, not just pipeline additions

## Technical Constraints

**Language**: Rust 1.76.0
**Parser**: Pest 2.x (PEG parser generator)
**WASM Target**: wasm-encoder crate
**Type System**: Constraint-based type inference with unification

**Code Quality Requirements**:
- NO placeholder implementations (no `return 0`, `todo!()`, etc.)
- Production-ready code only
- All functions must be fully functional
- Prefer fixing root causes over workarounds

## Architectural Insights from Context7 Research

**From Pest Parser Documentation**:
- Use rule composition over preprocessing
- Grammar rules like `class_method = { function_signature ~ function_body }` preferred
- Silent rules (`_{ ... }`) for implementation details
- Error recovery with `@{ ... }` atomic rules

**From Rust Compiler (rustc-dev-guide)**:
- Separate parsing from type inference (don't mix concerns)
- HIR should faithfully represent source code
- Type normalization before unification
- Proper HIR → MIR lowering with optimization passes

**From WebAssembly/Wasmtime**:
- Validate WASM early with `wasm-tools validate`
- Use result types for error handling (more compatible)
- Add optimization passes with `wasm-opt`

## Request for Analysis

Please analyze these issues and provide:

### 1. Root Cause Solutions

For each of the 4 critical issues, please identify:
- **Exact root cause** in the codebase architecture
- **Specific code locations** that need modification
- **Step-by-step fix strategy** with minimal risk of regression
- **Dependencies between fixes** (what must be done first)

### 2. Implementation Guidance

For each fix:
- **Rust code patterns** to use (with examples)
- **Pest grammar modifications** if needed
- **Type system considerations** for type inference
- **Testing strategy** to validate without regressions

### 3. Architectural Recommendations

- Should we **eliminate the preprocessor** and unify on grammar-only parsing?
- How should **namespace resolution** be structured (2-level, 3-level, N-level)?
- What's the **cleanest TAST design** for default parameters?
- Best practices for **error handling** in a WASM-targeting compiler

### 4. Priority Ordering

Given the failure breakdown:
- Which fixes provide **highest ROI** (tests fixed / complexity)?
- Which fixes are **prerequisites** for others?
- What is the **optimal implementation sequence** to reach 100%?

### 5. Risk Assessment

For each proposed solution:
- **Regression risk level** (Low/Medium/High)
- **Validation strategy** to catch regressions early
- **Rollback plan** if issues arise

## Success Criteria

**Primary Goal**: 285/285 tests passing (100%)

**Quality Standards**:
- No compilation errors in Rust codebase
- All .cln test files compile to valid WASM
- All compiled WASM programs execute correctly
- Zero regressions from baseline (181/285)

**Timeline**: Prefer solutions that can be implemented incrementally with validation after each step

## Additional Context

**Testing Infrastructure**:
- Full test suite: `python3 scripts/run_full_test_suite.py`
- Tests organized in `tests/cln/` by category (353+ files)
- Compiled output: `tests/output/` directory
- Results: `tests/results/` with JSON logs

**Recent Baseline Regressions**:
- Had 209/285 (73.3%) but uncommitted changes caused drop to 181/285 (63.5%)
- Changes were reverted to establish clean baseline
- Demonstrates importance of incremental validation

**Project Guidelines**:
- Fix root causes, not symptoms
- Prefer proper fixes over workarounds
- Document architectural decisions
- Update specification when adding features

Thank you for your expertise in helping achieve 100% test pass rate!
