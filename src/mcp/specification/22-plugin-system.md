# 22. Plugin System

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


The Clean Language Plugin System allows you to extend the language with custom Domain-Specific Language (DSL) blocks. Plugins transform DSL syntax into standard Clean Language code before compilation.

### Important: Plugins vs Imports

**`import:` is only for `.cln` source files. Plugins are never imported — they are declared in the project configuration.**

| Mechanism | What it is for | Where it goes |
|-----------|---------------|---------------|
| `import:` | Other `.cln` source files from your project or external Clean Language modules | At the top of each `.cln` file that needs them |
| `plugins:` | Framework plugins (frame.data, frame.server, frame.ui, etc.) | In `main.cln` at the project root |

**Plugins work differently from modules:**
- Plugins are declared once at the **project level** in `main.cln` — they are never declared in individual `.cln` files.
- Plugin-generated functions and types are **automatically available** across the project when the plugin is declared — no import statement is needed in individual files.
- Plugins cannot export `.cln` modules that are then imported via `import:`.

```clean
// main.cln — declare plugins here, once, for the whole project
plugins:
	frame.server
	frame.ui
	frame.data
```

```clean
// app.cln — use plugin blocks and functions directly, no import needed
endpoints:
	GET "/users" -> listUsers
```

```clean
// ❌ WRONG — never import a plugin
import:
	frame.server

// ✅ CORRECT — import another .cln file you wrote
import:
	utils
	models
```

### Overview

Plugins operate during the compilation pipeline, transforming custom blocks (like `endpoints:`, `data:`, `component:`) into standard Clean Language AST. This enables powerful abstractions without modifying the core language.

```
Source → Lexer → Parser → [PLUGIN EXPANSION] → HIR → TypeChecker → MIR → WASM
                              ↑
                      Plugins transform here
```

### Project Configuration

Every Clean Language project that uses plugins must have a `main.cln` file in the project root:

```clean
// main.cln
project:
    name: "my-web-app"
    version: "1.0.0"

plugins:
    frame.server       // Enables endpoints: blocks
    frame.ui        // Enables component: blocks
    frame.data      // Enables schema: blocks
```

The compiler reads this configuration and loads the specified plugins before compilation.

### Plugin Auto-Detection (v0.30.15+)

The compiler can automatically detect and load plugins based on the file's location in the project structure. This eliminates the need for explicit `plugins:` declarations in most cases.

**Auto-Detection Rules:**

| File Path Pattern | Auto-Loaded Plugins |
|-------------------|---------------------|
| `app/server/api/`, `app/server/`, `app/server/middleware/` | `frame.server`, `frame.data`, `frame.auth` |
| `app/data/`, `app/data/models/`, `app/data/queries/`, `app/data/migrations/` | `frame.data` |
| `app/auth/` | `frame.auth` |
| `app/canvas/`, `app/canvas/scenes/`, `app/canvas/sprites/`, `app/canvas/audio/` | `frame.canvas` |
| `app/ui/`, `app/ui/components/`, `app/ui/pages/`, `app/ui/layouts/` | `frame.ui` |

**Example - No explicit plugin declaration needed:**

```clean
// File: app/server/api/users.cln
// Plugins auto-detected: frame.server, frame.data, frame.auth

functions:
    string getUsers()
        string result = _db_query("SELECT * FROM users", "[]")
        return result

start:
    integer s = _http_route("GET", "/users", 0)
```

**Precedence:**
1. Explicit `plugins:` block declarations are always respected
2. Auto-detected plugins are merged with explicit declarations
3. No duplicates are loaded

This feature is especially useful for Frame Framework projects where folder conventions are standard.

### Framework Blocks

Framework blocks are custom DSL blocks defined by plugins. They follow the apply-block syntax:

```clean
// Custom DSL block
blockname:
    DSL content here
    Each plugin defines its own syntax
```

### Example: HTTP Endpoints Plugin

With `frame.server` declared in `main.cln`, the `endpoints:` block becomes available:

```clean
// main.cln
plugins:
    frame.server

// app.cln
endpoints:
    GET "/users" -> listUsers
    GET "/users/{id}" -> getUser
    POST "/users" -> createUser
    PUT "/users/{id}" -> updateUser
    DELETE "/users/{id}" -> deleteUser

functions:
    list<User> listUsers()
        return database.query("SELECT * FROM users")

    User getUser(string id)
        return database.findById("users", id)

    User createUser()
        // Handle POST body
        return database.insert("users", request.body)
```

This expands to route registration code that integrates with your web framework.

### Block Attributes

Plugins can support attributes that modify behavior:

```clean
@version("v2")
@auth
@cache(ttl: 300)
endpoints:
    GET "/api/users" -> listUsers
```

Attributes are passed to the plugin and can affect code generation.

### Plugin Categories

| Category | Example Blocks | Purpose |
|----------|---------------|---------|
| **Web** | `endpoints:`, `routes:` | HTTP API definition |
| **Data** | `schema:`, `model:` | Database models |
| **UI** | `component:`, `view:` | UI components |
| **Config** | `config:`, `settings:` | Configuration DSLs |

### Handler Parameter Type (v0.30.39+)

Bridge functions in plugin.toml can declare a `"handler"` parameter type for callback dispatch. This allows developers to pass function names instead of numeric indices:

```toml
# plugin.toml
[[bridge.functions]]
name = "_ui_onEvent"
params = ["string", "string", "handler"]
returns = "integer"
expand_strings = true
```

In Clean Language source code, the developer passes a function name:

```clean
start:
    integer s = ui.onEvent(".btn", "click", loadUsers)

functions:
    loadUsers()
        print("Loading users...")
```

The compiler:
1. Verifies `loadUsers` exists in the `functions:` block
2. Assigns a unique handler index (auto-incrementing, starting from 0)
3. Passes the index as i32 to the bridge function
4. Exports the function as `handle_event_N` in the WASM module

The same function referenced in multiple handler parameters gets the same index. At the WASM level, `handler` compiles to `i32` — the runtime dispatches callbacks via `handle_event_{index}` exports.

Supported bridge parameter types: `"string"`, `"integer"`, `"number"`, `"boolean"`, `"void"`, `"handler"`.

### IDE Support

Plugins provide IDE integration through the Language Server:

- **Autocomplete**: Plugin keywords appear in completion lists
- **Hover Documentation**: Hover over keywords for documentation
- **Diagnostics**: Real-time error checking for DSL syntax
- **Syntax Highlighting**: Plugin keywords are colorized

### Creating Custom Plugins

Plugins are **Clean Language programs compiled to WebAssembly** — not Rust code. Each plugin ships a `plugin.toml` manifest and a `plugin.wasm` binary. Authoring a plugin is covered in [Plugin Authoring](../framework/plugin-authoring.md); the formal ABI is fixed by [`spec/framework/plugin-contract.md`](../../spec/framework/plugin-contract.md) — that document is authoritative when this summary and the contract disagree.

### Plugin `expand` WASM ABI

Each plugin exports a single WASM function (declared in `plugin.toml` under `[exports].expand`) with the signature:

```wat
(export "expand" (func (param i32 i32) (result i32)))
;; param 0: pointer to length-prefixed JSON input string
;; param 1: length of JSON input in bytes
;; result:  pointer to length-prefixed JSON result string
```

The compiler passes **one JSON string** containing the block name, raw content, and attributes. The plugin returns **one JSON string** — either a success envelope with generated Clean Language source, or an error envelope with an optional line/column.

**Input JSON:**
```json
{
  "name": "endpoints",
  "content": "GET \"/users\":\n\treturn json(users)",
  "attributes": [{ "name": "auth", "value": null }]
}
```

**Output JSON — success:**
```json
{ "statements": "<generated Clean Language source code>" }
```

**Output JSON — error:**
```json
{ "error": "Descriptive message", "line": 2, "column": 5 }
```

The `statements` value is re-parsed as Clean Language source and inserted at the block's location. **Generated code may contain nested framework blocks; the compiler recursively expands until no framework blocks remain.** See [`spec/framework/plugin-contract.md`](../../spec/framework/plugin-contract.md) §5.1 for the authoritative ABI.

> **Warning:** Older documents showed a Clean-level signature like `expand_block(name: string, attributes: string, body: string) -> string` or a Rust `impl FrameworkPlugin` trait. Both are incorrect. The real ABI is a single JSON string in, a single JSON string out.

### Security Model

Plugins run in a **WASM sandbox during compilation**: no filesystem access (except reading the plugin's own source), no network access, no system calls, memory isolated per plugin, and deterministic execution. Generated code goes through the full type-checking pipeline; plugins cannot bypass the type system or reach compiler internals.

| Source | Trust level | Notes |
|--------|-------------|-------|
| Official (`frame.*`) plugins | High | Shipped and versioned with the compiler |
| Registry plugins | Medium | Community reviewed |
| Local plugins | User-defined | Installed manually from a local path |

### Versioning & Compatibility

Plugins follow [Semantic Versioning](https://semver.org/):
- **MAJOR** — breaking change to block syntax, bridge function signatures, or exported types.
- **MINOR** — new blocks, bridge functions, or types added in a backward-compatible way.
- **PATCH** — bug fixes with no interface changes.

The `[compatibility].min_compiler_version` field in `plugin.toml` is enforced by the compiler: it refuses to load plugins whose `min_compiler_version` is higher than the running compiler. See [`spec/framework/plugin-contract.md`](../../spec/framework/plugin-contract.md) §7 for the full breaking-change list.

### `implicit_import` — What It Does and Does Not Control

`implicit_import = true` in a plugin's `plugin.toml` controls whether **individual files** in the plugin's owned folders skip per-file import statements. It does **not** make the plugin itself implicit — every project that uses a plugin must still declare it in `main.cln`'s `plugins:` block.

- Plugin declaration in `main.cln`: **always required**.
- Per-file import statement in owned folders: **implicit** when `implicit_import = true`.

### Key Benefits

- **Non-invasive**: Core language stays minimal
- **Type-safe**: Generated code is fully type-checked
- **IDE Support**: Full autocomplete and diagnostics
- **Composable**: Multiple plugins work together

---

[← Previous: AI Integration](./21-ai-integration.md) | [Index](./README.md) | [Next: State Management](./23-state-management.md) →
