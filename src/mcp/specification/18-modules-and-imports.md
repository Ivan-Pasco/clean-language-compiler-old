# 18. Modules and Imports

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


**Clean Language supports multi-file programs through a module system.** Each `.cln` file is a module that can import and use code from other modules.

**Important:** The `import:` statement is **exclusively for Clean Language source files** (`.cln` files). Plugins (frame.data, frame.server, frame.ui, etc.) are declared in `main.cln` using the `plugins:` block — they are never imported. See the [Plugin System](./22-plugin-system.md) section for details.

### Module Definition

Every `.cln` file is implicitly a module. The module name is derived from the filename (without the `.cln` extension).

```clean
// file: utils.cln
// This file defines the "utils" module

functions:
    public:
        integer add(integer a, integer b)
            return a + b

        integer multiply(integer a, integer b)
            return a * b
```

### Importing Modules

Use the `import:` block to import other modules. What an import brings in depends on its form. Importing a whole module brings in its classes and its public functions and state variables (see [Visibility Model](#visibility-model)); the functions and state are used **through the module name**:

```clean
// file: main.cln
import:
    utils

start:
    integer sum = utils.add(5, 3)
    integer product = utils.multiply(4, 2)
    print(sum)
    print(product)
```

The short name alone is not in scope. `add(5, 3)` here is error **SEM007** "Function 'add' not found", with the help "write `utils.add`, or import `utils.add`".

#### Import Block Syntax

The import block uses indentation to list imported modules:

```clean
import:
    utils           // Import the utils module
    math_helpers    // Import the math_helpers module
    data.models     // Import from nested path (data/models.cln)
```

#### Import Forms

| Import | Brings in | Called as |
|--------|-----------|-----------|
| `utils` | the whole module | `utils.add()`, `utils.count` |
| `utils as U` | the whole module, under another name | `U.add()`, `U.count` (`utils.add()` and `add()` are not in scope) |
| `data.models` | the module in `data/models.cln` | `models.describe()` (the last part of the path) |
| `utils.add` | one public function or state variable | `add()` (no other `utils` member is in scope) |
| `utils.add as plus` | one public function or state variable, renamed | `plus()` |
| `"helpers.cln"` | the file, joined to this one | `double()` (short names, see [File Path Imports](#file-path-imports)) |

A dotted name `a.b` means the module file `a/b.cln` when that file exists; otherwise it is the symbol `b` of module `a`.

```clean
// file: data/models.cln
functions:
    public:
        string describe(integer id)
            return "model " + id.toString()
```

```clean
// file: main.cln
import:
    utils as U
    data.models
    utils.multiply as times

start:
    print(U.add(1, 2))              // 3
    print(models.describe(4))       // model 4
    print(times(3, 4))              // 12
```

**Classes stay unqualified.** The class names of an imported module are used by their own name, whatever the import form: after `import: shapes`, write `Circle c = Circle(2.0)`, not `shapes.Circle`. There is no `module.Type` syntax.

#### File Path Imports

In addition to module-name imports, an `import:` block can list **file paths** as string literals. This is useful for importing files from specific locations without relying on module resolution.

```clean
// Import files using their relative paths
import:
    "app/data/models.cln"
    "../lib/utils.cln"
    "./helpers.cln"
```

**One project, one namespace.** A module import brings in code from outside the file's own project, so its functions keep their origin in the name (`utils.add()`). Files joined by a file path import, or by the `shared:` folders of a package manifest (`package:` in `main.cln`), are parts of **one** program: together they form one namespace, and their public functions and state are used by their short names (`double(21)`). This is the Go package model: inside a package, names are short; from another package, they carry the package name.

Because every module keeps its own names, two modules imported by name may each declare the same public function or state:

```clean
import:
    greetings
    farewells

start:
    print(greetings.message())   // greetings' own message()
    print(farewells.message())   // farewells' own message()
```

Inside one namespace the names must be unique: two files joined by file path imports or `shared:` folders that both declare a public `message` are a redefinition (SEM003).

**Key differences from module imports:**

| Feature | Module Import | File Path Import |
|---------|---------------|------------------|
| Syntax | `import:` + `module_name` | `import:` + `"path/to/file.cln"` |
| Resolution | Search paths (`./`, `./lib/`, etc.) | Relative to importing file |
| Nested paths | `data.models` → `data/models.cln` | `"data/models.cln"` (explicit) |
| Names | `module.fn()` | `fn()` (one namespace) |

**Path Resolution:**

File path imports are resolved **relative to the directory of the importing file**, not the project root:

```
project/
├── main.cln              # imports "app/data/models.cln"
├── app/
│   ├── data/
│   │   └── models.cln    # imports "../../lib/utils.cln"
│   └── services/
│       └── api.cln
└── lib/
    └── utils.cln
```

```clean
// file: main.cln
import:
    "app/data/models.cln"     // Resolves to ./app/data/models.cln

start:
    integer result = squareDouble(3)
    print(result)             // 36
```

```clean
// file: app/data/models.cln
import:
    "../../lib/utils.cln"     // Resolves to ./lib/utils.cln (relative to app/data/)

functions:
    public:
        integer double(integer x)
            return x * 2

        integer squareDouble(integer x)
            integer doubled = double(x)
            return square(doubled)    // from lib/utils.cln (public there)
```

```clean
// file: lib/utils.cln
functions:
    public:
        integer square(integer x)
            return x * x
```

**Chained Imports:**

File path imports can be chained - imported files can import other files:

```clean
// main.cln imports models.cln which imports utils.cln
// All functions from all three files are available in the final WASM
```

**When to use each import style:**

- **Module imports** (`import: utils`): for modules brought in from outside the file's project, such as libraries in `./lib/`; their members carry the module name
- **File path imports** (`import:` + `"path/file.cln"`) and `shared:` folders: for the files of one project, which share short names

### Module `start:` Blocks

A module may have its own `start:` block. When the program runs, the `start:` blocks of imported modules run first, in dependency order (see [Dependency Graph](#dependency-graph)), and the entry file's `start:` runs last:

```clean
// file: utils.cln
start:
    print("utils start")

functions:
    public:
        integer add(integer a, integer b)
            return a + b
```

```clean
// file: main.cln
import:
    utils

start:
    print("main start")
    print(utils.add(1, 2))
```

Output:
```
utils start
main start
3
```

### Multi-File Compilation

The compiler automatically discovers and compiles all imported modules (both module-name and file path imports). Both `build` and `compile` commands support multi-file compilation:

```bash
# Build a multi-file project (resolves all imports)
cln build main.cln

# Compile also supports multi-file imports
cln compile main.cln -o app.wasm

# Build with custom output path
cln build main.cln -o app.wasm

# Build with library search paths (for module-name imports)
cln build main.cln -L ./lib -L ./modules

# Build with optimization level
cln build main.cln -O3
```

**Note:** File path imports (`"path/file.cln"` in an `import:` block) are resolved relative to the importing file and do not use `-L` search paths. Module-name imports (`import: module`) use the search paths.

#### Module Resolution

When resolving an import, the compiler searches in the following order:

1. **Current directory** - Same directory as the importing file
2. **./lib/** - Library directory
3. **./modules/** - Modules directory
4. **./src/** - Source directory
5. **Custom paths** - Paths specified with `-L` flag

For each search path, the compiler tries these file patterns:
- `{module}.cln` (e.g., `utils.cln`)
- `{module}/mod.cln` (e.g., `utils/mod.cln`)
- `{module}/index.cln` (e.g., `utils/index.cln`)

#### Dependency Graph

The compiler builds a dependency graph of all modules and compiles them in topological order (dependencies before dependents). This ensures that when a module is compiled, all its dependencies are already available.

```
main.cln → math_helpers.cln → utils.cln
         ↘ data.cln
```

In this example, `utils.cln` is compiled first, then `math_helpers.cln` and `data.cln`, and finally `main.cln`.

#### Circular Dependencies

Circular dependencies are detected and reported as errors:

```clean
// file: a.cln
import:
    b  // a imports b

// file: b.cln
import:
    a  // b imports a - CIRCULAR DEPENDENCY ERROR!
```

### Built-in Modules

The following modules are built into the language and don't need to be imported from files:

| Module | Description |
|--------|-------------|
| `math` | Mathematical functions (sin, cos, sqrt, etc.) |
| `string` | String manipulation functions |
| `list` | List operations |
| `file` | File I/O operations |
| `http` | HTTP client functions |
| `json` | JSON parsing (`textToData`) and serialization (`dataToText`) |
| `console` | Console I/O |

Built-in modules are automatically available when imported:

```clean
import:
    math
    string
    list

start:
    number pi = math.pi
    string upper = "hello".toUpperCase()
    list<integer> nums = list.range(1, 10)
```

### Visibility Model

**Private by default.** Everything a file declares is private to that file. The only visibility construct is a `public:` sub-section; there is no `private:` keyword. A member becomes visible to other files only when it is declared under `public:`:

| Where | Made visible by |
|-------|-----------------|
| Module functions | the `public:` sub-section of `functions:` |
| Module state variables | the `public:` sub-section of `state:` |
| Class fields | the class's `public:` field sub-section |
| Class methods | the `public:` sub-section of the class's `functions:` block |

Using a private member from another file is error **SEM005**:

```clean
// file: mymodule.cln
state:
    integer calls = 0          // private
    public:
        integer limit = 10     // visible to importers

functions:
    integer helperFunc()       // private
        return 42

    public:
        integer publicFunc()
            return helperFunc() * 2

// file: main.cln
import:
    mymodule

start:
    integer x = mymodule.publicFunc()   // OK
    print(mymodule.limit)               // OK
    integer y = mymodule.helperFunc()   // ERROR SEM005: 'helperFunc' is private and cannot be accessed from outside 'mymodule'
```

**Plugin-generated code.** Code that a plugin generates into a module (for example functions expanded from a framework block) may call that module's private functions.

**Classes.** Private class members are not inherited: a subclass that reads, writes or calls its parent's private field or method gets SEM005, and so does `ClassName.method()` on a method that is not under `functions: public:` when written outside the class. See [Classes and Objects](./15-classes-and-objects.md).

### Example: Multi-File Project

Here's a complete example of a multi-file Clean Language project:

```clean
// file: utils.cln
functions:
    public:
        integer add(integer a, integer b)
            return a + b

        integer multiply(integer a, integer b)
            return a * b

        integer double_value(integer n)
            return n * 2
```

```clean
// file: math_helpers.cln
import:
    utils

functions:
    public:
        integer square(integer n)
            return utils.multiply(n, n)

        integer quadruple(integer n)
            return utils.double_value(utils.double_value(n))
```

```clean
// file: main.cln
import:
    utils
    math_helpers

start:
    // Use functions from utils
    integer sum = utils.add(10, 5)
    print(sum)  // Output: 15

    // Use functions from math_helpers
    integer sq = math_helpers.square(4)
    print(sq)  // Output: 16

    // Combined usage
    integer result = utils.multiply(sq, 2)
    print(result)  // Output: 32
```

Compile with:
```bash
cln build main.cln -o app.wasm
```

---

[← Previous: Method-Style Syntax](./17-method-style-syntax.md) | [Index](./README.md) | [Next: Asynchronous Programming](./19-async.md) →
