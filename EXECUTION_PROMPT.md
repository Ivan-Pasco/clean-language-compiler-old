# Clean Language Simplification - Execution Prompt

## Overview
You are tasked with implementing the Clean Language Simplification Strategy to restore the language's core principle: **"one way to do things: the easiest one"**. The goal is to reduce complexity by ~60% while maintaining 100% of essential functionality.

## Primary Objective
Transform Clean Language from a complex, feature-rich language (8.5/10 complexity) into a simple, elegant language (3.5/10 complexity) that preserves all essential power while eliminating redundancy and confusion.

## Implementation Strategy

### Phase 1: Standardize to Method-Style Syntax (Priority: HIGH)

#### Task 1.1: Standardize to Method-Style Syntax as Primary Pattern
**File**: `src/stdlib/method_style.rs` (~586 lines)
**Action**: 
1. Keep and enhance the method-style manager
2. Remove static method syntax from parser (`src/parser/expression_parser.rs`)
3. Update grammar to reject `Class.method()` patterns (keep `object.method()`)
4. Deprecate traditional function calls where method-style exists
5. Update all examples to use method-style syntax

**Validation**: Ensure `text.length()` works while `String.length(text)` is rejected

#### Task 1.2: Remove Static Method Syntax Manager  
**File**: `src/stdlib/static_methods.rs`
**Action**:
1. Remove static method call parsing (reject `Class.method()` patterns)
2. Update semantic analysis to only recognize lowercase namespaces
3. Ensure `math.add(a, b)` works while `Math.add(a, b)` is rejected

**Validation**: Only lowercase namespace access allowed

#### Task 1.3: Standardize to Lowercase Namespaces
**Files**: All standard library modules
**Action**:
1. Remove all capitalized namespace registrations
2. Update function registration to use only lowercase (`math`, `string`, `list`, etc.)
3. Update all test files to use lowercase syntax
4. Update documentation examples

**Validation**: Only `math.sqrt()` works, `Math.sqrt()` is rejected

### Phase 2: Simplify Type System (Priority: HIGH)

#### Task 2.1: Keep List Behaviors (Maintain Flexibility)
**Files**: 
- `src/stdlib/list_behavior.rs` (~400 lines to enhance)
- Documentation and examples

**Action**:
1. Keep list behavior switching mechanism
2. Improve documentation for each behavior type
3. Add better error messages and type hints
4. Ensure clear naming conventions for behavior strings

**Enhanced Usage**:
```clean
list<integer> taskQueue = []
taskQueue.type = "line"        // Clear FIFO queue behavior

list<integer> undoStack = []  
undoStack.type = "pile"        // Clear LIFO stack behavior
```

#### Task 2.2: Simplify Type System to Essential Types Only
**File**: `src/stdlib/type_precision.rs` (~898 lines to remove)
**Action**:
1. Remove type precision manager completely
2. Simplify to four core types: `integer`, `number`, `boolean`, `string`
3. Use platform-optimal defaults (integer=32-bit, number=64-bit)
4. Remove all precision modifiers from grammar and parser
5. Update WebAssembly code generation to use standard types

**Result**: Clean essential types without complexity

### Phase 3: Refine Syntactic Features (Priority: MEDIUM)

#### Task 3.1: Keep Apply-Blocks (Maintain Clean Syntax)
**Files**: 
- `src/parser/grammar.pest` (enhance apply-block rules)
- `src/parser/statement_parser.rs` (improve apply-block parsing)
- Documentation and examples

**Action**:
1. Keep apply-block parsing in grammar
2. Improve documentation with clear use cases
3. Ensure consistent indentation handling
4. Add better error messages for malformed blocks

**Enhanced Usage**: 
```clean
integer:           // Group related declarations
    count = 0
    maxSize = 100

string:            // Clean organization  
    name = "Alice"
    email = "alice@example.com"
```

#### Task 3.2: Simplify String Interpolation (Keep Core Value)
**File**: `src/stdlib/string_interpolation.rs` (~585 lines to simplify)
**Action**:
1. Limit interpolation to variable names and simple property access
2. Remove complex method call evaluation in strings
3. Keep property access (user.name) but remove method calls (messages.count())
4. Reduce string interpolation complexity by ~50%

**Before**: `"Hello {user.name}, you have {messages.count()} new messages"`
**After**: `"Hello {user.name}, you have {messageCount} new messages"`

### Phase 4: Consolidate Standard Library (Priority: MEDIUM)

#### Task 4.1: Keep Essential Functions Including Complete Math Support
**Files**: All stdlib modules (`src/stdlib/`)
**Action**:
1. Audit all 39+ standard library modules
2. Remove duplicate functionality (keep one implementation only)
3. Keep complete math support including trigonometric functions
4. Standardize naming conventions

**Keep Essential Functions with Math Completeness**:
```clean
// Complete Math (including trigonometric functions)
math.add, math.subtract, math.multiply, math.divide
math.sqrt, math.abs, math.max, math.min, math.round
math.sin, math.cos, math.tan, math.asin, math.acos, math.atan  // Keep trig functions
math.pi, math.e, math.ln, math.log10, math.exp               // Keep constants and log
math.floor, math.ceil, math.pow                              // Keep essential rounding

// String: Core text operations  
string.length, string.concat, string.contains, string.split
string.upper, string.lower, string.trim, string.replace

// List: Essential array operations
list.add, list.remove, list.get, list.length, list.contains
list.sort, list.reverse, list.join

// File: Basic I/O
file.read, file.write, file.exists

// Http: Core web requests
http.get, http.post
```

#### Task 4.2: Standardize Function Naming
**Action**:
1. Convert all function names to consistent pattern
2. Use `noun.verb` pattern: `string.length`, `list.add`, `math.sqrt`
3. Update all references in codebase and tests

### Phase 5: Preserve Essential Power Features

#### Keep These Features (High Value, Acceptable Complexity):
1. **Strong Type System**: Essential for safety
2. **Class Inheritance**: Core OOP functionality
3. **Generic Types with 'any'**: Enables code reuse  
4. **Error Handling**: Critical for robust programs
5. **Async Programming**: Modern necessity
6. **Test Framework**: Developer productivity

#### Simplify Implementation Without Removing Features:
1. **Type Inference**: Smarter defaults
2. **Constructor Auto-Assignment**: Reduce boilerplate
3. **Import System**: Keep simple module system

## Implementation Guidelines

### Development Process
1. **Feature Flag Development**: Use feature flags for each change
2. **Test-Driven Changes**: Ensure 100% test success rate maintained
3. **Incremental Implementation**: One phase at a time
4. **Migration Tools**: Create automatic code conversion tools
5. **Documentation Updates**: Update docs with each change

### Validation Criteria
1. **Functionality**: All essential features still work
2. **Simplicity**: Only one way to accomplish each task
3. **Performance**: No performance regression
4. **Test Success**: Maintain 100% test compilation rate

### Safety Measures
1. **Backup**: Create full codebase backup before starting
2. **Rollback Plan**: Ability to revert any change
3. **Gradual Deployment**: Test each change thoroughly
4. **User Validation**: Ensure changes improve developer experience

## Specific Implementation Commands

### Step 1: Start with Method-Style Standardization
```bash
# 1. Create feature branch
git checkout -b standardize-method-style

# 2. Keep and enhance method-style manager
# Edit src/stdlib/method_style.rs - enhance implementation

# 3. Remove static method syntax from parser
# Edit src/parser/expression_parser.rs - remove Class.method() parsing

# 4. Run tests to ensure functionality preserved
cargo test

# 5. Update examples to use method-style syntax
# Convert test files from String.length(text) to text.length()

# 6. Commit changes
git commit -m "Standardize to method-style syntax - remove static method patterns"
```

### Step 2: Standardize Namespaces
```bash
# 1. Update all stdlib modules to lowercase only
# Edit all files in src/stdlib/ - remove capitalized registrations

# 2. Update test files
# Convert Math.sqrt() to math.sqrt() in all test files

# 3. Run full test suite
cargo test

# 4. Commit changes  
git commit -m "Standardize to lowercase namespaces only"
```

### Continue with remaining phases...

## Success Criteria

### Completion Metrics
- [ ] Method-style syntax standardized as primary pattern
- [ ] Single lowercase namespace convention enforced  
- [ ] Essential type system implemented (4 core types only)
- [ ] Apply-blocks enhanced with better documentation
- [ ] String interpolation simplified (variables + simple property access only)
- [ ] Standard library consolidated with complete math support
- [ ] 100% test success rate maintained
- [ ] Documentation updated to show preferred method-style approach

### Quality Verification
1. **Simplicity Test**: New developers can be productive in days, not weeks
2. **Consistency Test**: Every operation has exactly one obvious way to accomplish it
3. **Performance Test**: No performance regression from simplification
4. **Migration Test**: Existing code can be automatically converted

## Final Goal
Transform Clean Language into a truly **clean** language that embodies its core principle: **simplicity without sacrifice of essential power**.

The result should be a language where developers never wonder "what's the right way to do this?" because there's only one obvious way - the Clean way.