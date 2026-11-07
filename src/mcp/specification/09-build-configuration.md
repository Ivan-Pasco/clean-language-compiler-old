# 09. Build Configuration

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


When using state `rules:`, a `build:` block is required in your source file before `start:`. This block configures how the compiler handles rules checking.

**Basic structure:**
```clean
build:
    rules = true

state:
    integer count = 0
    rules:
        count >= 0

start:
    count = 5
```

**Rules options:**

| Value | Behavior |
|-------|----------|
| `true` | Always check rules |
| `false` | Never check rules (production optimization) |
| `"development"` | Check only in development builds |

**Example for production apps:**
```clean
build:
    rules = "development"

state:
    integer balance = 0
    rules:
        balance >= 0

start:
    balance = 100
```

**CLI behavior:**
```bash
cln compile app.cln              # Development: rules ON
cln compile app.cln --release    # Production: follows build: setting
```

**Rules for `build:`:**
- Must appear at the top level, before `start:`
- Only one `build:` block per file
- Only required when using state `rules:`
- If you use `rules:` in your state but don't have a `build:` block, the compiler will show an error

---

[← Previous: File Structure](./08-file-structure.md) | [Index](./README.md) | [Next: Functions](./10-functions.md) →
