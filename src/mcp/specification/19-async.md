# 19. Asynchronous Programming

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean uses `start`, `later`, and `background` for simple background execution:
- `start` begins a task in the background
- `later` declares that the result will be available in the future
- The value blocks only when accessed
- Use `background` to run a task without keeping the result
- Mark a function as `background` to always run it in the background

### Two Uses of the `start` Keyword

The `start` keyword has two distinct meanings in Clean Language:

| Context | Syntax | Purpose |
|---------|--------|---------|
| Entry point | `start:` | Block that marks where your program begins (top-level only) |
| Background expression | `start functionCall()` | Starts a function running in the background |

These are completely separate features that happen to share a keyword. The compiler tells them apart by context.

### Start Expression

Use `start` before a function call to run it in the background:

```clean
start:
    // Using 'start' for background — different from the entry block!
    later data = start fetchData("url")
    print("Working...")
    print(data)          // blocks here only

    background logAction("login")    // runs and ignores result
```

- `later T name = start f()` — begins `f()` in the background and binds a future of type `T` to `name`. The current thread continues.
- Reading `name` blocks until the background task completes.
- `background f()` runs `f()` and discards the result — no future is created.

### Background Functions

**Mark a function as `background` so every call to it runs in the background automatically.**

```clean
functions:
	void syncCache() background
		sendUpdateToServer()
		clearLocalTemp()

start:
	syncCache()    // runs in background automatically
```

### Background Task Error Handling

**Errors in background tasks do not propagate to the calling context.** If a background task throws, it fails silently unless an `onError` handler is attached to the expression that started it.

The `loading` pattern — setting a flag before an async operation and clearing it after — should always be wrapped in `onError` to ensure cleanup happens even on failure:

```clean
functions:
	void fetchUser(integer id) background
		loading = true
		any user = start api.getUser(id) onError:
			loading = false
			error("Failed to fetch user")
		username = user.name
		loading = false
```

Without the `onError` block, if `api.getUser(id)` fails, `loading` would stay `true` and the UI would be stuck in a loading state.

### Semantics Summary

- A `later` binding is a future — a placeholder for a value that will exist later.
- Accessing a `later` binding blocks the current task until the underlying computation finishes.
- Background tasks run on the runtime's scheduler and are independent of the calling task.
- Errors in background tasks do not propagate — attach `onError` at the point where `start` is used, not at the read site.
- Marking a function `background` is equivalent to prefixing every call to it with `background`.

---

[← Previous: Modules and Imports](./18-modules-and-imports.md) | [Index](./README.md) | [Next: Memory Management](./20-memory-management.md) →
