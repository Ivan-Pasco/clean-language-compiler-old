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

Use the `import:` block to import other modules. The imported module's classes and its public functions and state variables become available (see [Visibility Model](#visibility-model)).

```clean
// file: main.cln
import:
    utils

start:
    // Use functions from utils module
    integer sum = add(5, 3)
    integer product = multiply(4, 2)
    print(sum)
    print(product)
```

#### Import Block Syntax

The import block uses indentation to list imported modules:

```clean
import:
    utils           // Import the utils module
    math_helpers    // Import the math_helpers module
    data.models     // Import from nested path (data/models.cln)
```

#### Import Variations

```clean
import:
    Math                // whole module
    math.sqrt           // single symbol (import specific function)
    Utils as U          // module alias
    Json.decode as jd   // symbol alias
```

#### File Path Imports

In addition to module-name imports, Clean Language supports **direct file path imports** using string literals. This is useful for importing files from specific locations without relying on module resolution.

```clean
// Import a file using its relative path
import "app/data/models.cln"
import "../lib/utils.cln"
import "./helpers.cln"
```

**Key differences from module imports:**

| Feature | Module Import | File Path Import |
|---------|---------------|------------------|
| Syntax | `import: module_name` | `import "path/to/file.cln"` |
| Resolution | Search paths (`./`, `./lib/`, etc.) | Relative to importing file |
| Nested paths | `data.models` → `data/models.cln` | `"data/models.cln"` (explicit) |

**Path Resolution:**

File path imports are resolved **relative to the directory of the importing file**, not the project root:

```
project/
├── main.cln              # import "app/data/models.cln"
├── app/
│   ├── data/
│   │   └── models.cln    # import "../../lib/utils.cln"
│   └── services/
│       └── api.cln
└── lib/
    └── utils.cln
```

```clean
// file: main.cln
import "app/data/models.cln"  // Resolves to ./app/data/models.cln

start:
    integer result = double(21)
    print(result)
```

```clean
// file: app/data/models.cln
import "../../lib/utils.cln"  // Resolves to ./lib/utils.cln (relative to app/data/)

functions:
    public:
        integer double(integer x)
            return x * 2

        integer squareDouble(integer x)
            integer doubled = double(x)
            return square(doubled)  // From utils.cln (public there)
```

**Chained Imports:**

File path imports can be chained - imported files can import other files:

```clean
// main.cln imports models.cln which imports utils.cln
// All functions from all three files are available in the final WASM
```

**When to use each import style:**

- **Module imports** (`import: utils`): For project modules in standard locations
- **File path imports** (`import "path/file.cln"`): For explicit paths, external files, or non-standard project structures

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

**Note:** File path imports (`import "path/file.cln"`) are resolved relative to the importing file and do not use `-L` search paths. Module-name imports (`import: module`) use the search paths.

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
    integer x = publicFunc()   // OK
    print(limit)               // OK
    integer y = helperFunc()   // ERROR SEM005: 'helperFunc' is private and cannot be accessed from outside 'mymodule'
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
            return multiply(n, n)

        integer quadruple(integer n)
            return double_value(double_value(n))
```

```clean
// file: main.cln
import:
    utils
    math_helpers

start:
    // Use functions from utils
    integer sum = add(10, 5)
    print(sum)  // Output: 15

    // Use functions from math_helpers
    integer sq = square(4)
    print(sq)  // Output: 16

    // Combined usage
    integer result = multiply(sq, 2)
    print(result)  // Output: 32
```

Compile with:
```bash
cln build main.cln -o app.wasm
```

---

[← Previous: Method-Style Syntax](./17-method-style-syntax.md) | [Index](./README.md) | [Next: Asynchronous Programming](./19-async.md) →
