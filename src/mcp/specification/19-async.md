# 19. Asynchronous Programming

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean uses `later` and `background` to put work off until later:
- `later name = start f()` defers the call `f()` and binds its future result to `name`
- The call runs the first time `name` is read
- `background f()` defers the call `f()` and keeps no result
- A program runs one task at a time: deferred work runs *later*, never *at the same time* as other code

### Two Uses of the `start` Keyword

The `start` keyword has two distinct meanings in Clean Language:

| Context | Syntax | Purpose |
|---------|--------|---------|
| Entry point | `start:` | Block that marks where your program begins (top-level only) |
| Deferred call | `later name = start functionCall()` | Marks the call that `later` puts off |

These are completely separate features that happen to share a keyword. The compiler tells them apart by context.

### Start Expression

`start` marks the call in a `later` declaration:

```clean
start:
    later data = start fetchData("url")
    print("Working...")
    print(data)          // fetchData runs here, the first time data is read

    background logAction("login")    // runs after start: finishes; result ignored
```

- `later name = start f()` — puts off `f()` and binds a future of its result to `name`. The code after it keeps running.
- Reading `name` runs `f()` if it has not run yet, then gives its result.
- `background f()` puts off `f()` and discards the result — no future is created.

Only `later` and `background` put work off. Anywhere else, `start f()` is just the call `f()`: it runs right away, where it stands. `integer n = start double(4)` is the same as `integer n = double(4)`.

### Background Task Error Handling

**Errors in deferred calls do not reach the code that deferred them.** If a deferred call raises an error, that call stops and the rest of the program goes on. Handle the error inside the function, or give a `later` call a fallback value with `onError` (see [When the Work Happens](#when-the-work-happens)).

The `loading` pattern — setting a flag before a slow operation and clearing it after — must clear the flag on failure too. Handle the error where it can happen:

```clean
start:
    background fetchUser(42)

state:
    string username = ""
    boolean loading = false

functions:
    void fetchUser(integer id)
        loading = true
        string name = loadName(id) onError:
            loading = false          // cleanup runs even when loadName fails
            return
        username = name
        loading = false

    string loadName(integer id)
        if id <= 0
            error("Failed to fetch user")
        return "user" + id.toString()
```

Without the `onError` block, if `loadName(id)` fails, `loading` would stay `true` and the UI would be stuck in a loading state.

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
`none`), unless an `onError` on the call gives a value:

```clean
	later count = start countUsers() onError 0
```

### Semantics Summary

- A `later` binding is a future — a placeholder for the result of a call that has not run yet.
- Reading a `later` binding runs its call if it has not run yet; every read gets the same value.
- Calls not yet run (`background` calls and unread `later` calls) run in order after the current entry point returns.
- A program runs one task at a time. Deferred calls run later, not in parallel, so they never change state at the same time as other code.
- An error in a deferred call stops that call only. It does not reach the code that deferred it: handle it inside the function, or attach `onError` to the call in the `later` declaration.

---

[← Previous: Modules and Imports](./18-modules-and-imports.md) | [Index](./README.md) | [Next: Memory Management](./20-memory-management.md) →
