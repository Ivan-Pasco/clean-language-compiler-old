# Clean Language Semantic Rules

**Authority:** This file is the single source of truth for semantic rules (Principle 3).
**Version:** 1.2.0
**Date:** 2026-11-26

Every rule has a numbered code. When the compiler rejects code for a semantic reason, the error message must include the rule code. A diagnostic without a code is incomplete (Principle 4).

---

## Error Code Ranges

These are the codes this file defines a rule for, by category. `error-codes.md` is the full registry; codes PLUGIN006–PLUGIN016 are defined in the typed-emission contract (`framework/contracts/typed-emission.md`), not here.

| Codes | Category | Phase |
|-------|----------|-------|
| SYN001–SYN008, SYN100–SYN101 | Syntax (parser) | Parsing |
| SEM001–SEM014 | Semantic (type, scope, access, capabilities) | HIR validation, resolver, type checker |
| SCOPE001–SCOPE005 | Scope rules | Resolver |
| FUNC001–FUNC012 | Function rules | HIR validation, resolver |
| CLASS001–CLASS009 | Class rules | HIR validation |
| IDX001–IDX004 | Index access rules | Type checker |
| STATE001–STATE005 | State rules | Type checker, runtime |
| IMPORT001–IMPORT004 | Import rules | Module resolver |
| PLUGIN001, PLUGIN002, PLUGIN018 | Plugin rules | Plugin loader, plugin expansion |
| COM001–COM006 | Compilation (codegen, module) | Code generation |
| BLD-LAYOUT, BLD-MANIFEST | Build (multi-file) | Project layout discovery |
| RUN001–RUN006 | Runtime (execution) | WASM execution |

---

## Syntax Rules (SYN)

### SYN001 — Invalid Token
The lexer encountered a character that is not valid in any context.

### SYN002 — Unexpected Token
A token appears where the grammar does not allow it.

It also covers a `+` in front of a value: Clean has no unary plus (`integer a = +5` is SYN002; write `5`).

It also covers `this`, which is a reserved word with no use: `this.x`, `this.m()` and a bare `this` are SYN002 — `` "`this` is not used in Clean: refer to the field or method by its name (`count`, `clear()`)" ``.

It also includes a type block (an apply block like `integer:` or `string:`
declaring variables) at the top level of a file. Apply blocks are statements; they
belong inside `start:`, `state:` or a function body.

**Message (top-level type block):** `"A type block like \`string:\` must be inside \`start:\`, \`state:\` or a function body"`

### SYN003 — Invalid Indentation
Indentation uses spaces instead of tabs, or the nesting level is inconsistent with the enclosing block.

### SYN004 — Unterminated Construct
A string literal, comment, or block was opened but not closed before end of file.

### SYN005 — Malformed Construct
A syntax structure is partially correct but missing required elements.

### SYN006 — Indentation Error
Tab/space mixing detected, or indentation does not match the expected level for the current block.

### SYN007 — Section Out of Order
The core top-level sections must appear in the order defined by
`foundation/docs/Clean Language Specification.md` §"File Structure":

```
import:  →  plugins:  →  start:  →  state:  →  class / can  →  functions:
```

Each section is optional, but when present they must follow this order; `import:`
comes before every other core section.  Auxiliary sections (`tests:`, `external:`, `watch:`, `screen:`,
`validate Name:`, framework blocks) may appear at any position and are not subject to this rule.

Two more sections have fixed places (specification §08 "File Structure"): `build:`
comes before `start:` and appears at most once per file (§09), and `source:` comes at
the top of the file (§21).

**Message:** `"'{section}' section is out of order — it must appear before '{other}'"`

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
- Class field initializer: initializer type must match the field type
- `none` (alias `null`) stored into a plain type: only a `T?` or an `any` holds none (type-system.md §8) — at all the sites above and as an element of a list or pairs literal stored into a typed list or pairs
- A `T?` used as a `T`: stored into a plain `T`, or its field read, method called or element indexed without `!` / `default`
- The result of a call to a `void` function used as a value ("returns nothing (void)")
- The two sides of `==`, `!=` or `is` have unrelated types — the same check as for `<`, `>`, `<=`, `>=`. Compatible pairs are: the same type; types with an implicit conversion between them (`integer` and `number`, a child class and its parent, a class and a capability it claims); `T?` and `T`; `T?` and `none`; and `any` with anything. `true == 1`, `"5" == 5` and `[1] == 1` are SEM001 ("Cannot unify types: boolean and integer")

**Example (passes):**
```clean
integer x = 42
string name = "Alice"
string? nickname = none
```

**Example (fails — SEM001):**
```clean
integer x = "hello"     // string is not assignable to integer
string title = none     // none cannot be used as string: a plain string is never none
```

### SEM002 — Undefined Symbol
A variable, function, class, or module is referenced but has not been declared.

**Applies to:**
- Variable use before declaration
- Function call to undefined function
- Class instantiation of undefined class
- Import of undefined module or symbol
- Method call on undefined method
- `base.method()` naming a method no ancestor class defines — `"No parent class of '{class}' defines a method '{method}'"`
- A `messages:` key in a `validate` block that names no field of the block (and is not `default`) — `"messages: '{key}' names no field of validate {name}"`

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
- A top-level user function with the name of a function a plugin block emits (for example a frame.ui `events:` handler export). The error names the plugin and the block: `"Function '{name}' is already defined at {location}; plugin '{plugin}' also emits a function '{name}' for the '{block}:' block"`, with the help `Rename your function '{name}': the plugin's function keeps its name because the plugin's generated code calls it`.

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
- A subclass reads, writes or calls a private member of its parent class. Private members are not inherited; only `public:` members are visible to subclasses
- `ClassName.method()` names a method that is not under `functions: public:`, from outside the class

**Not a violation:** code generated by a plugin (for example a framework block expanded into functions) may call the private functions of the module it is generated into.

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

**Example (fails — SEM005, parent's private member in a subclass):**
```clean
class Base
    integer secret = 1

class Kid is Base
    functions:
        public:
            void reset()
                secret = 0    // error: secret is private to Base, not inherited by Kid
```

**Message:** `"'{name}' is private and cannot be accessed from outside '{scope}'"`

`base.method()` on a parent method that is private is also SEM005.

For a subclass, the help line reads: `` Private members are not inherited: `{subclass}` cannot use `{parent}`'s private field|method `{name}` ``

### SEM006 — Inheritance Error
A class inheritance declaration is invalid.

A missing parent class, or a parent that is not a class (such as `integer`), is CLASS001, not SEM006.

**Applies to:**
- A method that overrides a parent's public method has different parameter types or a different return type — `"Method '{method}' of class '{class}' overrides '{parent}.{method}' with a different signature"`
- A method that overrides a parent's public method is not under `public:` — `"Method '{method}' of class '{class}' overrides the public method '{parent}.{method}' but is private"`. (A subclass method named like a parent's *private* method is a new method, not an override.)

**Example (fails — SEM006):**
```clean
class Animal
	functions:
		public:
			string speak()
				return "..."

class Dog is Animal
	functions:
		public:
			integer speak()      // overrides Animal.speak with a different return type
				return 1
```

### SEM007 — Unresolved Name
A name cannot be resolved, or a check failed that has no more specific code.

The registry first described SEM007 as a generic-type error (bad type arguments, tuple sizes, union types). Those cases cannot happen: the language has no user-defined generics, tuples or union types (type-system.md §10). This section describes what the compiler emits SEM007 for today.

**Applies to:**
- A variable that is not declared — `"Undefined variable '{name}'"` (also inside string interpolation)
- A call to a function that is not declared — `"Function '{name}' not found"`
- A call to a function a module does not define — `"Function '{fn}' not found in module '{module}'"`
- `req`, `res` or `session` used outside a frame.server endpoint handler
- An unknown pattern name in a `validate` block `match:` constraint — `"Unknown pattern name '{name}' in validate match constraint. Valid patterns: …"`
- Any other validation error raised without its own code

Several of these overlap SEM002 (undefined symbol) and SEM010 (unknown pattern name). The compiler is expected to move them to those codes (review finding M-12); until then they are reported as SEM007.

**Example (fails — SEM007):**
```clean
start:
	print(y)    // Undefined variable 'y'
```

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
The string literal passed to `.matches()` does not refer to a built-in pattern name.

**Condition:** The argument to `.matches()` must be a string literal (not a variable), and its value must be a known pattern name. This is validated at compile time; no runtime lookup is performed.

**Built-in names:** `"email"`, `"url"`, `"uuid"`, `"phone"`, `"date"`, `"integer"`, `"number"`, `"alphanumeric"`.

There are no other pattern names. Pattern packs (`import: validate.patterns.financial`, mentioned in `grammar.ebnf` §8) are not implemented: that import is a syntax error (finding A8-16).

**Example (fails — SEM010):**
```clean
boolean ok = someString.matches("postcode")   // "postcode" is not a built-in pattern
boolean ok = someString.matches("notapattern") // no such built-in pattern
```

**Fix:** Use a built-in pattern name.

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

A method that fulfils a capability must be declared under `functions: public:`. A private implementation is also SEM011, even when every call goes through the capability type.

**Example (fails — SEM011, private capability method):**
```clean
can Shape:
    integer area()

class Square can Shape
    functions:
        integer area()        // ERROR: fulfils Shape but is private
            return 4
```

**Message:** `"Method '{method}' on class '{class}' fulfils capability '{capability}' but is private"` — help: `` capability methods belong under `functions: public:` ``

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

### SEM014 — Capability Default Uses an Unknown Member

A capability default body names a member that is not a method of the same capability (e.g. `return color`); a default body may use its parameters, locals, top-level names and the capability's own methods.

**Example (fails — SEM014):**
```clean
can Describe:
    string describe()
    string tag()
        return color            // ERROR: color is not part of Describe
```

**Message:** `"The default body of '{capability}.{method}' uses '{name}', but '{name}' is not a method of capability '{capability}'"`

**Fix:** Only call methods declared in the same capability (by bare name, e.g. `describe()`) from a default body.

---

## Scope Rules

### Name Lookup Inside a Class

Inside a class method or constructor, a bare name is resolved in this order:

1. local variables and parameters;
2. the class's own members — fields, and methods including inherited public methods and capability methods;
3. top-level functions and state.

Clean has no `this`: an object's own members are always named by their bare name (`count`, `clear()`). Ambiguity between a parameter and a field cannot arise because a parameter may not reuse a field's name (CLASS007).

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
A function or method with a non-void return type must return a value on every execution path. This is an error (developer decision 2026-11-08).

A path ends properly when it reaches a `return` with a value, an `error(...)` call, or a loop it never leaves (`while true` with no `break` for that loop). Also accepted:
- a body whose last statement is an expression (automatic return, specification §10);
- an `if` whose `then` and `else` branches both end properly, and `if true` whose body does;
- an `onError` region whose protected statements and handler both end properly.

**Example (fails — FUNC004):**
```clean
functions:
    integer sign(integer n)
        if n > 0
            return 1
        else if n < 0
            return -1
        // n == 0 falls through without a value
```

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
Every parameter must be satisfied by exactly one positional or named argument. A parameter cannot be provided both positionally and by name. A parameter with a default value is satisfied by its default when a named-argument call leaves it out — at the end of the list or in the middle. This holds for functions, constructors and methods (a method call is matched against the receiver's own class).

**Example (fails — FUNC011):**
```clean
integer result = add(1, a: 1, b: 2)    // "a" covered twice (positionally and by name)
```

**Valid — defaulted parameters left out:**
```clean
functions:
    integer mid(integer a, integer b = 2, integer c = 0)
        return a * 100 + b * 10 + c

start:
    print(mid(a: 1, c: 3))    // b takes its default: 123
    print(mid(1, c: 5))       // 125
```

### FUNC012 — Method-Style Call on a Standalone Function
A function declared in a `functions:` block is called with method syntax, `value.fn(args)`. Standalone functions are called as `fn(value, args)`; method syntax is only for built-in type methods and class methods.

**Message:** `` "`{name}` is a standalone function — call it as `{name}(value, args)` not `value.{name}(args)`" ``

**Example (fails — FUNC012):**
```clean
start:
	Box b = Box()
	print(b.twice())     // twice is a top-level function, not a Box method

class Box
	integer n = 1

functions:
	integer twice(Box b)
		return 2
```

Not yet implemented for receivers of a built-in type: `4.twice()` with a top-level `twice` is reported as FUNC001 ("Method 'twice' not found on a integer value"). See finding A8-3.

---

## Class Rules

### CLASS001 — Parent Class Must Exist
When a class uses `is ParentName` for inheritance, the parent must be a class defined in the program. A parent that is not a class, such as a primitive type, is reported the same way.

**Message:** `"Parent class '{name}' is not defined"`

**Example (fails — CLASS001):**
```clean
class Child is NonExistentParent    // parent not defined
	integer x = 0

class Count is integer              // integer is not a class
	integer x = 0
```

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

### CLASS007 — Parameter Must Not Share a Field's Name
A parameter of a method, or of a constructor with a body, must not have the same name as a field of its class, including fields inherited from a parent class. Inside the class a field is used by its plain name, so such a parameter would hide the field.

A constructor **without a body** is the exception: its parameters name the fields they set. `constructor(integer x, integer y)` stores its two arguments in the fields `x` and `y`.

**Example (fails — CLASS007):**
```clean
class Account
    integer balance

    constructor(integer balance)
        print("opening")            // a body: `balance` would hide the field

    functions:
        void deposit(integer balance)   // a method parameter named like the field
            print(balance)
```

**Fix:** give the parameter a different name (`constructor(integer startBalance)` + `balance = startBalance`), or drop the constructor body when it only stores its parameters.

### CLASS008 — Method Called Through the Class Name Needs an Object
`ClassName.method()` has no object to run on, so it may only call a method that needs none. A method needs an object when its body reads or writes a field of the class (its own or inherited), or calls a method of the object that needs one; `toString()` always needs one. Methods that use no field or method of the object stay callable through the class name, whatever fields the class has.

**Message:** `"'{method}()' needs a {Class} object: create one (`{Class} {c} = {Class}()`) and call `{c}.{method}()`"` — `{c}` is the class name's first letter in lower case.

**Example (fails — CLASS008):**
```clean
start:
    print(Box.get())              // get() reads the field size

class Box
    integer size

    functions:
        public:
            integer get()
                return size
```

**Fix:** create an object and call the method on it: `Box b = Box()` then `b.get()`.

**Build modes:**
- `require` and `ensure` are always checked, in every build. `--release` does not remove them.
- `always:` checks are included in normal builds and removed in release builds (`cln compile --release`).
- `always:` is checked when a constructor of the class finishes and when each method of the class finishes. A false invariant stops execution (RUN005).

### CLASS009 — Child Constructor Must Call `base(...)` When the Parent's Constructor Needs Arguments
A child constructor runs its parent's constructor before its own body. When the child constructor calls `base(args...)`, that call runs the parent constructor. When it does not, and the parent's constructor takes no arguments, the parent constructor runs automatically, as if `base()` were the first statement. When it does not, and the parent's constructor needs arguments, the compiler cannot supply them: CLASS009.

**Message:** `"Constructor of '{child}' must call base(...): the constructor of '{parent}' needs arguments ({params})"`

**Example (passes — parent constructor runs automatically):**
```clean
class Shape
	public:
		string color

	constructor()
		color = "red"

class Circle is Shape
	number radius

	constructor(number radiusParam)
		radius = radiusParam        // Shape() runs first
```

**Example (fails — CLASS009):**
```clean
class Labeled
	public:
		string label

	constructor(string labelParam)
		label = labelParam

class Badge is Labeled
	integer level

	constructor(integer levelParam)
		level = levelParam          // error: Labeled's constructor needs a string
```

**Fix:** call `base(...)` with the parent's arguments: `base("gold")`.

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
		guard value + 1 else "must be boolean"         // not boolean

	integer size = 0
		guard file.exists("x.txt") else "no I/O"       // I/O call in a guard
```

**Messages:**
- `"Guard condition for state variable '{name}' must be boolean, found {type}"`
- `"Guard expression must be pure — found I/O call '{call}'. Guards cannot have side effects."` A call to a `file.`, `db.`, `http.` or `console.` function counts as I/O.

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
A structural typed-emission bridge (`_emit_function`, `_emit_class_full`, `_emit_stmt_from_source`, `_emit_expr_from_source`, `_emit_statement_into_start`, `_emit_statement_inline`, `_emit_external`, `_emit_state_block`, `_emit_route`, `_emit_helpers_batch`, `_emit_class_methods_from_spec`, or any `batch.*` builder) was called from inside a plugin's `assemble_typed` hook. Inside `assemble_typed`, only `_inject_source_file` is legal. See [`framework/contracts/assemble.md`](framework/contracts/assemble.md) §6.2 for the full illegal-bridge list and the rationale. Emitted at the first illegal call site; the sticky-error flag (typed-emission.md §3.15) prevents cascading duplicates.

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

Integer `/` and `%` (any integer size) with a zero divisor raise the error
`"division by zero"`. It is an ordinary catchable error: `onError` handles it
(`integer q = a / b onError 0`); uncaught, it stops the program like an uncaught `error(...)`.
Number (decimal) division does not raise: it follows IEEE 754 (`1.0 / 0.0` is
infinity, `0.0 / 0.0` is NaN).

Integer arithmetic never wraps. On `integer`, `integer:64`, `integer:32u` and `integer:64u`, `+`, `-`, `*`,
unary `-` whose result is outside the type (for the unsigned types, also below
`0`), and `MIN / -1`, raise the catchable error `"integer overflow"`. Arithmetic
on the 8- and 16-bit types is computed in 32 bits; storing a result outside the
type raises the "does not fit" error below.
Number arithmetic follows IEEE 754 and never raises.

A run-time value stored into an integer type it does not fit (declaration,
assignment, argument, return, field, list/pairs element, conversion) raises the
catchable error `"<value> does not fit in <type>"`, e.g.
`300 does not fit in integer:8`.

When the value is a compile-time constant, both cases are compile-time errors
(SEM001): `300 does not fit in integer:8 (-128 to 127)` and
`integer overflow: this constant expression is outside integer (-2147483648 to 2147483647)`.

### RUN004 — Reference Error
A none or invalid reference was accessed at runtime (e.g. `value!` when `value` is none).

### RUN005 — Assertion Failure
A contract was false at runtime: a `require` when the function was called, an `ensure` when it returned, or an `always:` invariant when a constructor or method finished. Contract failures stop execution; `onError` does not catch them.

### RUN006 — List Index Out of Range
A list was read, written or shrunk at an index outside `0` to `length() - 1`: `items[i]`, `items[i] = v`, `items.get(i)`, `items.set(i, v)` or `items.remove(i)`. It is an ordinary catchable error: `onError` handles it (`integer e = xs[5] onError -1`); uncaught, it stops the program like an uncaught `error(...)`.

**Message:** `"index {i} is out of range for a list of length {n}"`
