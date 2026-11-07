# Clean Language Semantic Rules

**Authority:** This file is the single source of truth for semantic rules (Principle 3).
**Version:** 1.1.0
**Date:** 2026-04-19

Every rule has a numbered code. When the compiler rejects code for a semantic reason, the error message must include the rule code. A diagnostic without a code is incomplete (Principle 4).

---

## Error Code Ranges

| Range | Category | Phase |
|-------|----------|-------|
| SYN001–SYN008 | Syntax (parser) | Parsing |
| SEM001–SEM010 | Semantic (type, scope, access) | HIR validation, resolver, type checker |
| SCOPE001–SCOPE005 | Scope rules | Resolver |
| FUNC001–FUNC011 | Function rules | HIR validation |
| CLASS001–CLASS006 | Class rules | HIR validation |
| IDX001–IDX004 | Index access rules | Type checker |
| STATE001–STATE005 | State rules | HIR validation, runtime |
| IMPORT001–IMPORT004 | Import rules | Module resolver |
| PLUGIN001–PLUGIN002 | Plugin rules | Plugin loader |
| COM001–COM006 | Compilation (codegen, module) | Code generation |
| RUN001–RUN005 | Runtime (execution) | WASM execution |

---

## Syntax Rules (SYN)

### SYN001 — Invalid Token
The lexer encountered a character that is not valid in any context.

### SYN002 — Unexpected Token
A token appears where the grammar does not allow it.

### SYN003 — Invalid Indentation
Indentation uses spaces instead of tabs, or the nesting level is inconsistent with the enclosing block.

### SYN004 — Unterminated Construct
A string literal, comment, or block was opened but not closed before end of file.

### SYN005 — Malformed Construct
A syntax structure is partially correct but missing required elements.

### SYN006 — Indentation Error
Tab/space mixing detected, or indentation does not match the expected level for the current block.

### SYN007 — Section Out of Order
The 5 core top-level sections must appear in the order defined by
`foundation/docs/Clean Language Specification.md` §"File Structure":

```
import:  →  start:  →  state:  →  class  →  functions:
```

Each section is optional, but when present they must follow this order.  Auxiliary
sections (`tests:`, `external:`, `watch:`, `screen:`, framework blocks, `apply`,
`public:`) may appear at any position and are not subject to this rule.

**Example (fails — SYN007):**
```clean
functions:
    integer add(integer a, integer b)
        return a + b

start:                              // error: 'start:' must appear before 'functions:'
    print(add(2, 3))
```

**Exception:** Parsers invoked with lenient section ordering (used for plugin-
generated code) skip this rule.  User-written `.cln` source always has the rule
applied.

### SYN008 — Invalid Print Block

A `print:` block contains no expressions, or contains non-expression statements.

**Condition:** A `print:` block is parsed but the indented body contains no
expressions, or one of the items in the body is not a valid expression.

**Example (fails — SYN008):**
```clean
print:
    // no expressions here
```

**Message:** `"print: block must contain at least one expression"`

---

### SYN100 — Missing Spec Path

The `spec` AI-metadata statement was parsed but the required string-literal path
is missing or is not a string literal.

The `spec` statement links a function (or other declaration) to its
specification document. The argument must be a string literal naming the
relative or absolute path of the spec; identifiers, numbers, and unquoted
paths are not accepted.

**Condition:** The `spec` keyword token is consumed, and the next non-whitespace
token is not a string literal.

**Example (fails — SYN100):**
```clean
functions:
    void greet()
        spec       // ← missing string literal path
        print("hi")
```

**Example (passes):**
```clean
functions:
    void greet()
        spec "documentation/greet.md"
        print("hi")
```

**Message:** `"Expected string literal after 'spec'"`

---

### SYN101 — Missing Intent Description

The `intent` AI-metadata statement was parsed but the required string-literal
description is missing or is not a string literal.

The `intent` statement records a natural-language description of a function's
purpose so AI tooling can reason about callers without re-reading the body.
The argument must be a string literal; identifiers, numbers, and unquoted
descriptions are not accepted.

**Condition:** The `intent` keyword token is consumed, and the next
non-whitespace token is not a string literal.

**Example (fails — SYN101):**
```clean
functions:
    void greet()
        intent     // ← missing string literal description
        print("hi")
```

**Example (passes):**
```clean
functions:
    void greet()
        intent "Print a friendly greeting to stdout."
        print("hi")
```

**Message:** `"Expected string literal after 'intent'"`

---

## Semantic Rules (SEM)

### SEM001 — Type Mismatch
The type of an expression does not match what the context requires.

**Applies to:**
- Variable assignment: RHS type must be assignable to LHS declared type
- Function return: return value type must match declared return type
- Function argument: argument type must match parameter type
- State variable initializer: initializer type must match declared state type
- Computed state body: computed body must return the declared type

**Example (passes):**
```clean
integer x = 42
string name = "Alice"
```

**Example (fails — SEM001):**
```clean
integer x = "hello"     // string is not assignable to integer
```

### SEM002 — Undefined Symbol
A variable, function, class, or module is referenced but has not been declared.

**Applies to:**
- Variable use before declaration
- Function call to undefined function
- Class instantiation of undefined class
- Import of undefined module or symbol
- Method call on undefined method

**Example (fails — SEM002):**
```clean
start:
    print(x)    // x not declared
```

### SEM003 — Symbol Redefinition
A symbol (variable, function, class) is declared more than once in the same scope.

**Applies to:**
- Variable declared twice in same scope
- Function defined twice at top level
- Class defined twice
- Duplicate import items

**Example (fails — SEM003):**
```clean
functions:
    integer add(integer a, integer b)
        return a + b
    integer add(integer x, integer y)    // redefinition
        return x + y
```

### SEM004 — Invalid Operation for Type
An operator or operation is applied to a type that does not support it.

**Example (fails — SEM004):**
```clean
string result = "hello" - "world"    // subtraction not valid for strings
```

### SEM005 — Access Violation

A private member is accessed from outside its allowed scope.

**Visibility model:** Members are **private by default**. The **only** visibility construct is a `public:` sub-section. There is no `private:` keyword. Any declaration NOT inside a `public:` block is private.

**Applies to:**
- A module function NOT inside a `public:` sub-section of `functions:` is called from an importing module
- A class field NOT inside the class's `public:` field sub-section is read or written outside the class
- A class method NOT inside a `public:` sub-section of the class's `functions:` block is called outside the class
- A state variable NOT inside a `public:` sub-section of `state:` is read or written from an importing module
- A screen-local state variable is referenced outside its screen block

**Example (fails — SEM005, module function):**
```clean
// file: mymodule.cln
functions:
    public:
        integer publicFunc()
            return helper()

    // Private helper — no `public:` prefix, so private by default
    integer helper()
        return 42

// file: main.cln
import:
    mymodule

start:
    integer x = helper()    // error: helper is private to mymodule
```

**Example (fails — SEM005, class field):**
```clean
class BankAccount
    // Private field — no `public:` prefix
    number balance = 0

    public:
        string owner

start:
    BankAccount acc = BankAccount("Alice", 100.0)
    number b = acc.balance    // error: balance is private to BankAccount
```

**Example (fails — SEM005, class method):**
```clean
class BankAccount
    functions:
        // Private helper — no `public:` prefix
        boolean isValid(number amount)
            return amount > 0

        public:
            void deposit(number amount)
                if isValid(amount)
                    balance = balance + amount

start:
    BankAccount acc = BankAccount("Alice", 100.0)
    boolean ok = acc.isValid(50.0)    // error: isValid is private to BankAccount
```

**Message:** `"'{name}' is private and cannot be accessed from outside '{scope}'"`

### SEM006 — Inheritance Error
A class inheritance declaration is invalid.

**Applies to:**
- Parent class does not exist
- Parent is not a class type (e.g., trying to inherit from a primitive)

**Example (fails — SEM006):**
```clean
class Child is NonExistentParent    // parent not defined
    integer x
```

### SEM007 — Generic Type Error
A generic or polymorphic type operation is invalid.

**Applies to:**
- Incompatible type arguments
- Tuple size mismatch
- Union type incompatibility

### SEM008 — Inheritance Cycle
A class inherits from itself directly or indirectly, creating a cycle.

**Example (fails — SEM008):**
```clean
class A is B
class B is A    // circular
```

### SEM009 — Invalid Type Specification
A type name does not refer to a valid type.

**Example (fails — SEM009):**
```clean
FooBar x = 42    // FooBar is not a defined type
```

---

### SEM010 — Unknown Pattern Name
The string literal passed to `.matches()` does not refer to a built-in pattern name or a name provided by an imported pattern pack.

**Condition:** The argument to `.matches()` must be a string literal (not a variable), and its value must be a known pattern name. This is validated at compile time; no runtime lookup is performed.

**Built-in names:** `"email"`, `"url"`, `"uuid"`, `"slug"`, `"numeric"`, `"alpha"`, `"phone"`, `"date"`.

**Extended names:** available after importing a pattern pack, e.g. `import: validate.patterns.financial` unlocks `"iban"`, `"ssn"`, `"creditCard"`.

**Example (fails — SEM010):**
```clean
boolean ok = someString.matches("postcode")   // "postcode" not in scope — missing import
boolean ok = someString.matches("notapattern") // no such built-in pattern
```

**Fix:** Use a built-in pattern name, or add the corresponding `import:` for the pattern pack that provides the name.

**Also fails — SEM010:** passing a variable instead of a string literal:
```clean
string pat = "email"
boolean ok = someString.matches(pat)   // variable not allowed — must be a literal
```

---

### SEM011 — Missing Capability Method

A class declares `can C` but does not implement one of C's required (non-default) methods.

**Example (fails — SEM011):**
```clean
can Draw:
    draw()

class Circle can Draw     // ERROR: missing method draw()
    public:
        number radius
```

**Fix:** Add the missing method to the class's `functions: public:` block, or remove the capability from the `can` clause.

---

### SEM012 — Undefined Capability

A `can` clause on a class, or a type reference in a parameter/return position, names a capability that has not been declared.

**Example (fails — SEM012):**
```clean
class Circle can Fly    // ERROR: capability Fly not defined
```

**Fix:** Declare the capability with a `can Fly:` block, or correct the spelling.

---

### SEM013 — Capability Method Signature Mismatch

A class implements a capability method, but its parameter types or return type do not match the capability's signature.

**Example (fails — SEM013):**
```clean
can Save:
    save(string path)

class Circle can Save
    functions:
        public:
            save(integer path)    // ERROR: expected string path, got integer
                print(path)
```

**Fix:** Change the class's method signature to match the capability exactly.

---

### SEM014 — Capability Default Calls Undefined Method

A default method body in a `can` block calls `this.otherMethod()`, but `otherMethod` is neither declared in the same capability nor guaranteed to exist on every class that claims the capability.

**Example (fails — SEM014):**
```clean
can Describe:
    describe() -> string
    tag() -> string
        return this.color            // ERROR: color is not part of Describe
```

**Fix:** Only call methods declared in the same capability from a default body.

---

## Scope Rules

### SCOPE001 — Variable Must Be Declared Before Use
Variables must be declared with an explicit type before being referenced in expressions or statements.

### SCOPE002 — Variable Cannot Be Redeclared in Same Scope
A variable name that already exists in the current scope cannot be redeclared.

### SCOPE003 — Maximum Scope Depth Exceeded
The scope nesting depth has exceeded the implementation limit. This indicates deeply nested code that should be refactored.

### SCOPE004 — Watch Target Must Reference State Variable
Watch block target identifiers must reference variables declared in a `state:` block.

### SCOPE005 — Screen State Access

Screen-local state is accessed outside the screen that declared it.

**Condition:** A state variable declared inside a `screen:` block is referenced
in a context that is not lexically contained within that `screen:` block. Each
screen's state is its own namespace and cannot be accessed by other screens or
by top-level code.

**Example (fails — SCOPE005):**
```clean
screen Home:
    state:
        integer homeCount = 0

start:
    integer x = homeCount    // error: homeCount is local to screen Home
```

**Message:** `"State variable '{name}' is local to screen '{screen}' and cannot be accessed here"`

**Screen namespace:** A screen's functions are scoped under the screen's name
(developer decision 2026-10-31). Inside the screen, its state and functions are
used by their plain names (local variables and parameters shadow them); from
outside, a screen function is called as `Screen.fn(...)`. Two screens may
therefore declare the same state or function names (e.g. both declare
`render()`). A screen holds one copy of its state, initialized to the declared
values when the program starts.

```clean
screen Counter:
    state:
        integer count = 0

    functions:
        string render()
            return "Count: " + count.toString()

        void increment()
            count = count + 1

start:
    Counter.increment()
    print(Counter.render())    // Count: 1
```

---

## Function Rules

### FUNC001 — Function Must Be Defined Before Use
Functions must be defined (in a `functions:` block or as a class method) before being called.

### FUNC002 — Argument Count Must Match Parameter Count
The number of arguments in a function call must match the number of parameters in the function signature.

**Example (fails — FUNC002):**
```clean
functions:
    integer add(integer a, integer b)
        return a + b

start:
    integer result = add(1, 2, 3)    // expects 2 args, got 3
```

### FUNC003 — Cannot Call Non-Function Type
Only function-typed symbols can be invoked with parentheses.

### FUNC004 — Missing Return in Non-Void Function
A function with a non-void return type must have a return statement on all execution paths. (Warning)

### FUNC005 — Empty Return in Non-Void Function
A return statement in a function with a non-void return type must provide a value. (Warning)

### FUNC006 — Start Block Cannot Have Parameters
The `start:` entry point block does not accept parameters.

### FUNC007 — Start Block Should Return Void
The `start:` entry point should not return a value. (Warning)

### FUNC008 — Named Argument Label Must Match Parameter
The identifier in a named argument must exactly match a parameter name of the called function or constructor.

**Example (fails — FUNC008):**
```clean
functions:
    integer add(integer a, integer b)
        return a + b

start:
    integer result = add(x: 1, b: 2)    // "x" does not match any parameter name
```

### FUNC009 — No Duplicate Named Arguments
Each parameter name may appear at most once in a call's argument list.

**Example (fails — FUNC009):**
```clean
integer result = add(a: 1, a: 2)    // "a" appears twice
```

### FUNC010 — Positional Arguments Must Precede Named Arguments
In a mixed call, all positional arguments must come before any named arguments.

**Example (fails — FUNC010):**
```clean
integer result = add(a: 1, 2)    // named before positional
```

**Valid mixed call:**
```clean
integer result = add(1, b: 2)    // positional first, then named
```

### FUNC011 — All Parameters Must Be Covered
Every parameter must be satisfied by exactly one positional or named argument. A parameter cannot be provided both positionally and by name.

**Example (fails — FUNC011):**
```clean
integer result = add(1, a: 1, b: 2)    // "a" covered twice (positionally and by name)
```

---

## Class Rules

### CLASS001 — Parent Class Must Exist
When a class uses `is ParentName` for inheritance, the parent class must be defined.

### CLASS002 — Duplicate Field in Class
Field names within a class must be unique.

### CLASS003 — Duplicate Method in Class
Method names within a class must be unique.

### CLASS004 — Constructor Must Exist for Instantiation
A class that is instantiated must have a constructor (explicit or implicit).

### CLASS005 — `ensure` Must Appear After `require` and Before Other Statements
`ensure` statements must appear at the top of a function body, after any `require` statements, and before any other logic.

**Example (fails — CLASS005):**
```clean
functions:
    integer divide(integer a, integer b)
        require b != 0
        integer result = a / b
        ensure result * b == a    // error: must appear before logic
        return result
```

### CLASS006 — `always:` Expressions Must Be Boolean
Every expression inside an `always:` block must evaluate to `boolean`.

**Build modes:**
- `ensure` and `always:` checks are included in debug builds and stripped in release builds (`--release` flag).
- Use `--contracts` to force contract checks in release builds.

---

## Index Access Rules

### IDX001 — Array Index Must Be Integer
Array/list bracket access requires an integer index.

### IDX002 — Matrix Index Must Be Integer
Matrix bracket access requires an integer index.

### IDX003 — Pairs Key Must Be String
Pairs/map bracket access requires a string key.

### IDX004 — Cannot Index Non-Indexable Type
Only list, matrix, pairs, and any types support bracket access.

---

## State Rules

### STATE001 — Guard Condition Must Be Pure Boolean

A guard expression on a state variable is not a pure boolean expression, or contains side effects.

**Condition:** The expression after `guard` must be a pure boolean expression.
It may reference `value` (the proposed new value) and any currently-in-scope
identifiers, but must not contain side-effecting operations such as function
calls that perform I/O or mutate state.

**Example (fails — STATE001):**
```clean
state:
    integer count = 0
        guard print("checking") else "no side effects allowed"
```

**Message:** `"Guard condition must be a pure boolean expression"`

**Runtime behavior:** If the guard condition evaluates to `false` at runtime,
the state update is rejected and the state variable retains its previous value.
The error message from the `else` clause is reported (STATE002).

### STATE002 — Guard Rejection (Runtime)

A state update is rejected at runtime because a guard condition evaluated to false.

**Condition:** This is a runtime rule, not a compile-time error. It is raised
when an assignment to a guarded state variable evaluates the guard expression
and receives `false`.

**Example (triggers — STATE002 at runtime):**
```clean
state:
    integer count = 0
        guard value >= 0 else "Count cannot be negative"

functions:
    void decrement()
        count = count - 1    // runtime rejection if count is already 0
```

**Message:** `"State update rejected: {guard_message}"`

### STATE003 — Computed State Return Type Must Match

The body of a computed state declaration must produce a value matching the declared type.

**Applies to:**
- The last expression (or explicit `return`) in a computed state body must be
  assignable to the computed variable's declared type.
- Circular dependencies between computed state variables are also detected here.

**Example (fails — STATE003, type mismatch):**
```clean
state:
    computed:
        integer total    // declared integer
            return "not an integer"    // error: string is not assignable to integer
```

**Example (fails — STATE003, circular dependency):**
```clean
state:
    computed:
        string a
            return b    // a depends on b
        string b
            return a    // b depends on a — circular
```

**Message (type mismatch):** `"Computed state '{name}' body returns {actual_type}, expected {declared_type}"`

**Message (circular dependency):** `"Circular dependency in computed state: '{name}' depends on itself"`

### STATE004 — Computed State Assignment

Code attempts to assign a value directly to a computed state variable.

**Condition:** Computed state is read-only. Any assignment statement whose
left-hand side is a computed state variable is rejected at compile time.

**Example (fails — STATE004):**
```clean
state:
    string firstName = ""
    string lastName = ""
    computed:
        string fullName
            return firstName + " " + lastName

functions:
    void badAssign()
        fullName = "Alice Smith"    // error: fullName is computed, cannot assign
```

**Message:** `"Cannot assign to computed state variable '{name}': it is read-only"`

### STATE005 — Rules Expression Must Be Boolean

An expression inside a `state: rules:` block does not evaluate to a boolean.

**Condition:** Every expression listed under `rules:` must be a boolean
expression. Non-boolean expressions (e.g., integer arithmetic with no
comparison) are a compile-time error.

**Example (fails — STATE005):**
```clean
state:
    integer count = 0

    rules:
        count + 1    // error: not a boolean expression
```

**Message:** `"State rule expression must be a boolean expression, got {type}"`

---

## Import Rules

### IMPORT001 — Circular Dependency
Modules cannot have circular import dependencies.

### IMPORT002 — Module Not Found
The imported module does not exist in any search path.

### IMPORT003 — Symbol Not Found in Module
The specific symbol imported from a module is not exported by that module.

### IMPORT004 — Duplicate Import Item
The same item appears more than once in an import list.

---

## Plugin Rules

### PLUGIN001 — Plugin Not Found
A referenced plugin is not installed. Install with `cleen plugin add <name>`.

### PLUGIN002 — Plugin Registration Conflict
A plugin attempts to register a function or block handler that conflicts with an existing registration.

### PLUGIN018 — Assemble-Typed Illegal Bridge
A structural typed-emission bridge (`_emit_function`, `_emit_class_full`, `_emit_stmt_from_source`, `_emit_expr_from_source`, `_emit_statement_into_start`, `_emit_statement_inline`, `_emit_external`, `_emit_state_block`, `_emit_route`, `_emit_helpers_batch`, `_emit_class_methods_from_spec`, or any `batch.*` builder) was called from inside a plugin's `assemble_typed` hook. Inside `assemble_typed`, only `_inject_source_file` is legal. See [`plugins/contracts/assemble.md`](plugins/contracts/assemble.md) §6.2 for the full illegal-bridge list and the rationale. Emitted at the first illegal call site; the sticky-error flag (typed-emission.md §3.15) prevents cascading duplicates.

---

## Compilation Rules (COM)

### COM001 — WASM Generation Error
The code generator failed to produce valid WASM for a construct.

### COM002 — Optimization Error
An optimization pass produced invalid code.

### COM003 — Memory Layout Error
Memory allocation or layout calculation failed.

### COM004 — Module Resolution Error
Multi-file compilation failed to resolve module dependencies.

### COM005 — Target-Specific Error
The compilation target does not support a required feature.

### COM006 — Function Not Found During Compilation
A function that passed semantic analysis could not be located during code generation.

---

## Build Rules (BLD)

### BLD-LAYOUT — Obsolete Render Target Layout

The multi-file compiler discovers every project folder through each active
plugin's `[paths].owns` declaration (`spec/framework/plugin-contract.md` §1.2);
no folder name is hardcoded in the compiler. When the project tree still uses
the pre-frame.ui-2.7 `app/web/...` layout but no active plugin claims those
paths — i.e. the active `frame.ui` manifest already describes the nested
`app/ui/web/...` layout — the project must be reorganized before compilation
can continue.

**Trigger:**
1. `app/web/pages/` exists in the project tree.
2. `app/ui/web/pages/` does NOT exist.
3. At least one active plugin declares an `[paths].owns` entry under
   `app/ui/web` (proving the active plugin set targets the new layout).

**Resolution:** move `app/web/` to `app/ui/web/` and any cross-platform
content from `app/ui/` to `app/ui/shared/`, then update `entry:` in
`main.cln`. See the framework component's each plugin.toml [owned_folders] for the
authoritative layout description.

**Severity:** Error. Compilation does not proceed.

### BLD-MANIFEST — Code After Package Manifest

A file whose first line is `package:` is the project manifest: it holds build
configuration only, every setting indented under the `package:` line. The
compiler reads it for `entry:`, `shared:`, `target:` and `plugins:` and then
compiles the module named by `entry:`; the manifest itself is never compiled.

**Trigger:** the manifest file has a second top-level (unindented, non-comment)
line after `package:` — for example `start:` or `functions:`.

**Resolution:** move that code into the entry module or another module of the
project.

**Severity:** Error. Compilation does not proceed.

---

## Runtime Rules (RUN)

### RUN001 — Memory Violation
WASM execution attempted to access memory outside allocated bounds.

### RUN002 — Stack Error
WASM stack overflow or underflow during execution.

### RUN003 — Arithmetic Error
Division by zero or other invalid arithmetic operation at runtime.

### RUN004 — Reference Error
A null or invalid reference was accessed at runtime.

### RUN005 — Assertion Failure
A `require` statement evaluated to false at runtime.
