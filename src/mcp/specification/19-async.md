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

### When the Work Happens

A program runs one task at a time. "In the background" means *later*, not
*at the same time*:

- `background f(a, b)` and `later x = start f(a, b)` evaluate the receiver
  and the arguments right away, where the statement stands. The call itself
  does not run yet.
- A `later` call runs the first time its variable is read. Each later read
  gets the same value. Assigning to the variable first waits for the call,
  then replaces the value.
- Every call not yet run, `background` or unread `later`, runs in order once
  the current entry point returns. The entry point is `start:`, a request
  handler, or an event handler. A request's response is sent without
  waiting for them.
- Only a call is deferred: a function call, a method call or a static call,
  optionally followed by `onError`. Any other expression after `later` or
  `background` is evaluated where it stands.

```clean
start:
	later data = start fetchData("url")    // "url" evaluated now
	print("Working...")                    // runs first
	print(data)                            // fetchData runs here
	background logAction("login")          // runs after start: returns
	print("done")                          // printed before logAction runs
```

An error in a task stops that task only. A `later` variable whose call
failed reads the zero value of its type (`0`, `0.0`, `false`, `""`, or
`null`), unless an `onError` on the call gives a value:

```clean
	later count = start countUsers() onError 0
```

### Semantics Summary

- A `later` binding is a future — a placeholder for a value that will exist later.
- Accessing a `later` binding blocks the current task until the underlying computation finishes.
- Background tasks run on the runtime's scheduler and are independent of the calling task.
- Errors in background tasks do not propagate — attach `onError` at the point where `start` is used, not at the read site.
- Marking a function `background` is equivalent to prefixing every call to it with `background`.

---

[← Previous: Modules and Imports](./18-modules-and-imports.md) | [Index](./README.md) | [Next: Memory Management](./20-memory-management.md) →
