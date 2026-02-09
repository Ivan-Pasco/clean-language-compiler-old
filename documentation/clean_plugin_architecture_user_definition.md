# Clean Plugin Usage: User‑Facing Definition

## 1. What Is a Clean Plugin? (Developer Mental Model)
A **Clean plugin** is the simplest way for a Clean Language developer to gain new capabilities in the language without learning advanced internals.

A plugin:
- Adds **new blocks** (DSL blocks) you can write inside `.cln` files.
- Expands those blocks into **normal Clean code** before compilation.
- Requires no knowledge of ASTs, WASM pipelines, or Rust.

**As a user:** you just install a plugin, import it, and start using its blocks.

Example:
```clean
configuration:
    project:
        name = "my-app"

    plugins:
    frame.web
    frame.data
```

This file lives in the **parent folder** of your Clean project (for example, above `src/`).

The compiler transforms `endpoints:` into regular Clean functions that register routes.

---

## 2. How Developers Use Plugins

### 2.1. Simple Workflow
1. **Install Clean + a plugin using `cleen`:**
```bash
cleen install latest
cleen plugin add frame.web
```

2. **Import the plugin in your `.cln` file:**
```clean
import:
    frame.web
```

3. **Use its new blocks:**
```clean
endpoints:
    GET "/users" -> listUsers
```

4. **Compile as usual:**
```bash
cln build
```

The plugin converts the DSL into normal Clean code, and compilation continues normally.

### 2.2. Types of Plugins
- **Web/API:** `endpoints:`, `routes:`
- **Data/ORM:** `data:`, `model:`
- **UI/Pages:** `component:`, `view:`
- **Utilities:** `config:`, `log:`, `jobs:`

Each plugin simply adds readable blocks that make Clean more expressive.

### 2.3. Plugin Auto-Detection (v0.30.15+)

The compiler can automatically load plugins based on your file's location. No explicit import needed!

**Auto-Detection Rules:**

| Your File Location | Plugins Auto-Loaded |
|--------------------|---------------------|
| `app/api/users.cln` | httpserver, data, auth |
| `app/data/User.cln` | data |
| `app/auth/config.cln` | auth |
| `app/canvas/game.cln` | canvas |
| `app/ui/Button.cln` | ui |

**Example - Zero configuration:**
```clean
// File: app/api/users.cln
// No import needed - plugins auto-detected!

functions:
    string getUsers()
        return _db_query("SELECT * FROM users", "[]")

start:
    integer s = _http_route("GET", "/users", 0)
```

This feature works with Frame Framework's folder conventions. Just put your files in the right folders!

---

## 3. Plugin Commands (Using `cleen`)
Plugins are managed through the version manager **cleen**, the same tool used to install Clean Language.

### 3.1. Existing Commands
```bash
cleen install latest
cleen use 1.2.3
cleen list
```

### 3.2. New Plugin Commands
#### Install a plugin
```bash
cleen plugin add frame.web
```

#### Remove a plugin
```bash
cleen plugin remove frame.web
```

#### List installed plugins
```bash
cleen plugin list
```

#### Search for plugins
```bash
cleen plugin search web
```

### 3.3. Project Plugin Configuration
A project may include a **Clean configuration file** named `configuration.cln` written in Clean-style syntax:
```clean
project:
    name = "my-app"

plugins:
    frame.web
    frame.data
```

Then you can run:
```bash
cleen plugin sync
```

This installs everything needed for the project.

---

## 4. How Plugins Work Internally (Simplified Explanation)
1. The compiler parses your `.cln` file.
2. When it encounters a DSL block like `endpoints:`, it hands that node to the plugin that registered that block.
3. The plugin expands the block into normal Clean functions.
4. The compiler continues normally (types, CIR, WASM).
5. The WASM module uses the Clean Frame host runtime for HTTP, DB, UI, etc.

For developers: *“I write a friendly DSL block; the plugin turns it into real code.”*

---

## 5. Clean Manager (`cleen`) as the Distribution Hub

### 5.1. Current Behavior
`cleen` installs compiler versions under:
```
~/.cleen/versions/<version>/
```
And exposes the active compiler via `~/.cleen/bin/cln`.

### 5.2. Extended Behavior for Plugins and Framework
`cleen` will now distribute:
- The **Clean compiler**.
- **Plugins**.
- The **Clean Framework (Frame)**.

### 5.3. Installing Framework Bundles
```bash
cleen install clean@1.2.3      # Pure compiler
cleen install frame@1.2.3      # Compiler + official plugins + runtime
```

### 5.4. Framework Commands
```bash
cleen framework new my-app      # Create a new project
cleen framework upgrade         # Update framework + plugins
```

---

## 6. Why This Design Is Friendly
- One tool (`cleen`) installs everything.
- Plugins feel like “new blocks you can write”.
- Simple vocabulary: `add`, `remove`, `sync`, `install`, `use`.
- Low learning curve.
- Easy onboarding: clone project → `cleen plugin sync` → code.

---

This is the high‑level user‑facing definition of the plugin system and its interaction with cleen. More detailed technical architecture, lifecycle hooks, plugin packaging, and host integration can be added as next steps.

