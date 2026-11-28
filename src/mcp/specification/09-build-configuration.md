# 09. Build Configuration

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


When using state `rules:`, a `build:` block is required in your source file before `start:`. This block configures how the compiler handles rules checking.

**Basic structure:**
```clean
build:
    rules = true

start:
    count = 5

state:
    integer count = 0
    rules:
        count >= 0
```

**Rules options:**

| Value | Behavior |
|-------|----------|
| `true` | Always check rules |
| `false` | Never check rules (production optimization) |
| `"development"` | Check only in development builds (not with `--release`) |

These are the only values. Any other value (`rules = "production"`, `rules = 1`) is error BLD001.

**Example for production apps:**
```clean
build:
    rules = "development"

start:
    balance = 100

state:
    integer balance = 0
    rules:
        balance >= 0
```

**CLI behavior:**

Rules are checked when `rules = true`, or when `rules = "development"` and the build is not a release build. They are never checked when `rules = false`.

```bash
# with build: rules = "development"
cln compile app.cln              # Development build: rules ON
cln compile app.cln --release    # Release build: rules OFF

# with build: rules = true  -> rules ON in both builds
# with build: rules = false -> rules OFF in both builds
```

**Rules for `build:`:**
- Must appear at the top level, before `start:` — a `build:` block after `start:` is error SYN007
- Only one `build:` block per file — a second `build:` block is error SYN007
- Only required when using state `rules:`
- If you use `rules:` in your state but don't have a `build:` block, the compiler reports error BLD001

---

[← Previous: File Structure](./08-file-structure.md) | [Index](./README.md) | [Next: Functions](./10-functions.md) →
