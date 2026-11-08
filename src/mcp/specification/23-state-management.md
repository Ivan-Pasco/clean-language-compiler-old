# 23. State Management

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


State is a first-class concept in Clean Language. It provides persistent memory that outlives function calls, with built-in observability and sequential update guarantees.

### Core Principles

1. **Persistent Memory**: State stores values beyond function execution. Variables are temporary; state is remembered.
2. **Mutable**: State values are updated using normal assignment.
3. **Observable**: The runtime detects all state changes and can react to them.
4. **Explicit Scope**: State is declared in known scopes (app or screen level).
5. **Sequential Updates**: State mutations are processed in order, preventing race conditions.
6. **Background Compatible**: Background operations update state on completion, following sequential rules.
7. **In-Memory Default**: State lives in memory. Persistence is optional via plugins.
8. **First-Class**: State is recognized by the compiler and enforced by the runtime.

### State Declaration

State scope is determined by where it's declared. Use the `state:` block to define state variables.

**App-level state** — declared at module level (persists for application lifetime):

```clean
state:
    integer count = 0
    string username = ""
    boolean isLoggedIn = false
```

**Screen-level state** — declared inside a screen (persists for screen lifetime):

```clean
screen Home:
    state:
        string searchQuery = ""
        list<string> results = []
```

**Rules:**
- State can be declared at **two levels**: top-level (`state:` at module level) for app-wide state, and inside a `screen:` block for screen-local state.
- Top-level `state:` creates app-scoped state — accessible from any function or screen in the module.
- `state:` inside `screen:` creates screen-scoped state — accessible only within that screen block. Screen-local state is destroyed when the screen is removed.
- Initial values are required for all state variables.
- App-level names and screen-level names are **separate namespaces** — a screen can declare a variable with the same name as an app-level variable without conflict. Within each scope, names must be unique.

### State Rules

Use `rules:` inside a `state:` block to declare persistent invariants that must always be true. Rules are checked at **operation boundaries** — predictable points where your code completes a logical unit of work.

**When rules are checked:**
- After `start:` finishes executing
- After each `frame:` finishes (if present)
- After a handler finishes (e.g., HTTP endpoint, job handler, command handler)

This approach is simple and predictable: rules are verified when your code hands control back to the runtime.

**Syntax:**
```clean
state:
    integer balance
    integer limit

    rules:
        balance >= 0
        limit > 0
```

**Example:**
```clean
state:
    integer count = 0
    integer maxCount = 100

    rules:
        count >= 0
        count <= maxCount

start:
    count = 50
    count = count + 10
    // rules checked here, after start: completes
```

**Rules for `rules:`:**
- Can only appear inside a `state:` block
- Must appear after all state variable declarations
- Each rule must be a boolean expression
- If a rule becomes false, execution traps with a contract failure

**Error on violation:**
```
Rule violation: rule failed in state block
  Rule: balance >= 0
  Actual: balance = -50
```

**Why operation boundaries?** Checking rules after every assignment would be slow and could catch intermediate states that are valid in context. By checking at operation boundaries, you get meaningful guarantees without performance overhead.

### State Access

**Access state directly by name.** Within a scope (app-level or screen-level), state variable names are unique, so no prefix is ever needed.

```clean
state:
    integer count = 0
    string username = ""

functions:
    void showStatus()
        integer current = count
        string name = username
        print("User: " + name + ", Count: " + current.toString())
```

### State Mutation

Mutate state with standard assignment.

```clean
state:
    integer count = 0

functions:
    void increment()
        count = count + 1

    void reset()
        count = 0
```

### Observing State Changes

Use `watch:` to react when state changes. The block runs automatically after the state is updated.

```clean
state:
    integer count = 0

watch count:
    print("Count changed to: " + count.toString())

functions:
    void increment()
        count = count + 1    // Triggers the watch block
```

**Watching multiple state variables:**

```clean
state:
    string firstName = ""
    string lastName = ""

watch (firstName, lastName):
    print("Name changed")
```

### Computed State

**Computed state is a read-only derived value that is automatically re-evaluated when its dependencies change.** Declare computed values inside a `computed:` block within `state:`.

```clean
state:
	string firstName = ""
	string lastName = ""

	computed:
		string fullName
			return firstName + " " + lastName

functions:
	void setName(string first, string last)
		firstName = first
		lastName = last

start:
	setName("Alice", "Smith")
	print(fullName)    // Prints: Alice Smith
```

**Rules (STATE003):**
- Computed state is **read-only** — it cannot be assigned to directly.
- **Dependency tracking is performed by static analysis at compile time.** The compiler inspects which state variables appear in the computed block's body and registers them as dependencies.
- External function calls inside a computed body are treated as opaque — their internal dependencies cannot be tracked. If a computed value depends on an external function, it is conservatively re-evaluated on every state change.
- **Circular dependencies between computed state variables are a compile error** (SEM001 / STATE003). For example, if `fullName` references `displayName` and `displayName` references `fullName`, compilation fails.
- The return type of the computed body must match the declared type of the computed variable (STATE003).

### State Guards

**Guards validate a proposed new value before it is written to a state variable.** If the guard condition is false, the assignment is rejected and the state variable retains its current value.

```clean
state:
	integer count = 0
		guard value >= 0 else "Count cannot be negative"

	string email = ""
		guard isValidEmail(value) else "Invalid email format"

functions:
	void decrement()
		count = count - 1    // Throws error if result < 0 (STATE001)
```

**Rules (STATE001):**
- Guards are evaluated **before** the state variable is updated.
- If the guard condition is `false`, the update is rejected — the state variable stays unchanged.
- If the guard condition is `true`, the new value is written.
- `value` inside the guard expression refers to the **proposed new value**, not the current value.
- Guard conditions must be **pure boolean expressions** — side effects in guard conditions are not allowed.
- The `else` clause is a **string literal** containing the error message shown on rejection. It is not a statement.

### State Reset

Reset state to its initial value using the `reset` keyword.

```clean
state:
    integer count = 0
    string username = ""

functions:
    void clearCount()
        reset count          // count returns to 0

    void clearAll()
        reset state          // all state returns to initial values
```

### State with Background Tasks

**Functions run with `background` can update state when they complete. Updates remain sequential, preventing race conditions.**

Errors in background tasks do not propagate to the caller — always attach an `onError` handler when a function run with `background` updates state, so cleanup happens even on failure:

```clean
state:
	string username = ""
	boolean isLoggedIn = false
	boolean loading = false

functions:
	void fetchUser(integer id)
		loading = true
		any user = start api.getUser(id) onError:
			loading = false
			error("Failed to fetch user")
		username = user.name
		isLoggedIn = true
		loading = false
```

### State in Screens

Screens have their own state scope. Screen state is destroyed when the screen is removed.

```clean
state:
    string currentUser = ""    // App-level state

screen Home:
    state:
        integer homeCount = 0      // Screen-level state
        boolean homeLoading = false

    watch homeCount:
        print("Home count: " + homeCount.toString())

    functions:
        void increment()
            homeCount = homeCount + 1

        void showUser()
            // Access app-level state directly (unique names)
            print("Logged in as: " + currentUser)

screen Settings:
    state:
        boolean settingsDarkMode = false    // Different screen, different state

    functions:
        void toggleDarkMode()
            settingsDarkMode = not settingsDarkMode
```

### Complete Example

```clean
// App-level state
state:
    string user = ""
    string theme = "light"

    computed:
        string greeting
            return "Hello, " + user

// React to user changes
watch user:
    print(greeting)

// Core functions
functions:
    void setUser(string name)
        user = name

    void setTheme(string newTheme)
        theme = newTheme

    void loadProfile()
        any profile = start fetchProfile()
        user = profile.name

// Home screen with its own state
screen Home:
    state:
        integer visitCount = 0
            guard value >= 0 else "Cannot be negative"
        list<string> recentItems = []

    watch visitCount:
        print("Visits: " + visitCount.toString())

    functions:
        void recordVisit()
            visitCount = visitCount + 1

        void addItem(string item)
            recentItems.add(item)

        void clearHistory()
            reset recentItems

// Entry point
start:
    setUser("Alice")       // Triggers watch, prints "Hello, Alice"
```

### Summary

| Syntax | Purpose |
|--------|---------|
| `state:` (top-level) | App-scoped state |
| `state:` (in screen) | Screen-scoped state |
| `fieldName` | Access state by name |
| `fieldName = value` | Mutate state |
| `watch fieldName:` | React to state changes |
| `watch (a, b):` | React to multiple state changes |
| `computed:` | Define derived state |
| `guard condition else msg` | Validate before mutation |
| `reset fieldName` | Reset to initial value |
| `reset state` | Reset all state in scope |

### State vs Variables

| Aspect | Variables | State |
|--------|-----------|-------|
| Lifetime | Function scope | App or screen lifetime |
| Observability | Not observable | Observable via `watch:` |
| Persistence | Lost after function returns | Persists until reset or scope ends |
| Validation | None | Optional guards |
| Declaration | `integer x = 0` | Inside `state:` block |

---

[← Previous: Plugin System](./22-plugin-system.md) | [Index](./README.md)
