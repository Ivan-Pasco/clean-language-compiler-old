# 04. Type System

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


### Core Types

| Type&nbsp;(keyword) | Description | Default Mapping | Literal Examples |
|---------------------|-------------|-----------------|------------------|
| `boolean`  | Logical value (`true` / `false`) | 1 bit | `true`, `false` |
| `integer`  | Whole numbers, signed | 32-bit | `42`, `-17` |
| `number`    | Decimal numbers | 64-bit | `3.14`, `6.02e23` |
| `string`   | UTF-8 text, dynamically sized | — | `"Hello"` |
| `void`     | No value / empty return type | 0 bytes | *(function return only)* |
| `none`     | The type of the `none` value (alias `null`) | — | `none` |

**Type System Philosophy:**
Clean Language uses platform-optimal defaults for all numeric types. The `integer` type is 32-bit and the `number` type is 64-bit, providing the best balance of performance and precision for most applications.

### Precision Control for Larger Numbers

Clean Language supports **precision modifiers** for both integers and numbers when you need larger ranges or different precision levels:

#### Integer Precision Modifiers

```clean
// Standard integer (32-bit, -2,147,483,648 to 2,147,483,647)
integer standard = 2147483647

// 8-bit integer (-128 to 127)
integer:8 small = 127

// 16-bit integer (-32,768 to 32,767)  
integer:16 medium = 32767

// 32-bit integer (same as standard integer)
integer:32 large = 2147483647

// 64-bit integer (-9,223,372,036,854,775,808 to 9,223,372,036,854,775,807)
integer:64 huge = 9223372036854775807

// Unsigned variants (positive numbers only)
integer:8u smallUnsigned = 255      // 0 to 255
integer:16u mediumUnsigned = 65535  // 0 to 65,535
integer:32u largeUnsigned = 4294967295  // 0 to 4,294,967,295
integer:64u hugeUnsigned = 18446744073709551615  // 0 to 18,446,744,073,709,551,615
```

#### Number Precision Modifiers

```clean
// Standard number (64-bit double precision)
number standard = 3.141592653589793

// 32-bit single precision (faster, less precision)
number:32 singlePrecision = 3.14

// 64-bit double precision (same as standard number)
number:64 doublePrecision = 3.141592653589793
```

#### When to Use Precision Modifiers

**Use Larger Precision When:**
- **`integer:64`**: Working with very large whole numbers (timestamps, IDs, big calculations)
- **`number:64`**: Scientific computing, financial calculations requiring high precision
- **Unsigned integers**: When you know values will always be positive (array indices, counts)

**Use Smaller Precision When:**
- **`integer:8`**: Small counters, flags, or when memory is critical
- **`integer:16`**: Medium-sized numbers with memory constraints
- **`number:32`**: Graphics programming, real-time applications where speed matters more than precision

**Examples:**
```clean
functions:
    void demonstratePrecision()
        // Large calculations
        integer:64 population = 8000000000
        integer:64 timestamp = 1640995200000
        
        // High-precision calculations
        number:64 pi = 3.141592653589793
        number:64 e = 2.718281828459045
        
        // Memory-efficient small numbers
        integer:8 counter = 0
        integer:16 portNumber = 8080
        
        // Graphics calculations (speed over precision)
        number:32 screenX = 1920.0
        number:32 screenY = 1080.0
```

**Performance Characteristics:**
- **Memory Usage**: Smaller precision types use less memory
- **Speed**: 32-bit operations are typically faster than 64-bit on most platforms
- **Precision**: 64-bit numbers provide ~15 decimal digits vs ~7 for 32-bit
- **Range**: 64-bit integers can handle numbers up to ~9 quintillion vs ~2 billion for 32-bit

### Composite & Generic Types

| Type syntax | What it is | Example |
|-------------|------------|---------|
| `list<T>`  | Homogeneous resizable list (T is the element type) | `list<integer>`, `list<string>` |
| `matrix<T>` | 2-D list of lists (T is the element type) | `matrix<number>`, `[[1.0, 2.0], [3.0, 4.0]]` |
| `pairs<K,V>`  | Key-value associative container | `pairs<string, integer>` |
| `any`         | Compile-time generic: compiler skips type checking for this value | Used when the type genuinely cannot be known at compile time (plugin returns, JSON, external data) |

#### Bare Generic Type Shorthand

When a generic composite type is written without its type arguments, the compiler applies these defaults:

| Bare form | Expands to | Rationale |
|-----------|------------|-----------|
| `list` | `list<any>` | Generic collection where element type is not yet known |
| `matrix` | `matrix<any>` | Generic 2-D collection |
| `pairs` | `pairs<string, any>` | Generic key-value bag (string keys are overwhelmingly the common case; JSON-derived rows fit this exactly) |

```clean
// These are equivalent:
list x = [1, 2, 3]              // ← shorthand
list<any> x = [1, 2, 3]         // ← explicit

pairs row = json.textToData("{\"a\":1}")     // ← shorthand
pairs<string, any> row = json.textToData("{\"a\":1}")   // ← explicit
```

The shorthand is optional; the explicit form remains legal and preferred when the element/value type is actually known. Use the shorthand only when the type genuinely cannot be constrained (JSON parsing, plugin returns, external data — same rationale as `any`).

Lists in Clean are zero-indexed by default (list[0] is the first element).
For readability, you can access elements starting from 1 using:

list.at(index)
This returns the element at position index - 1.

### List Behaviors — Collection Behavior Modifiers

**List behaviors change how a list handles insertions and removals without changing its type.** Clean Language extends the core `list<T>` type with dot-notation behavior modifiers. This provides a unified, consistent approach to different collection patterns while maintaining type safety and simplicity.

#### Behavior Type Syntax

The type is declared inline in the variable declaration using dot notation:

```clean
list<integer>.line numbers = []          // FIFO queue of integers
list<string>.unique visitors = []        // Set of strings (no duplicates)
list<string>.line.unique taskQueue = []  // FIFO queue with uniqueness
```

The canonical behavior suffixes are:
- `.line` — FIFO queue behavior
- `.pile` — LIFO stack behavior
- `.unique` — set behavior (no duplicates)
- `.line.pile` — FIFO + LIFO combined
- `.line.unique` — FIFO queue with uniqueness
- `.pile.unique` — LIFO stack with uniqueness
- `.line.unique.pile` — all three behaviors

#### Supported Properties

**`.line` — Queue Behavior (FIFO)**

**First-In-First-Out behavior: elements are added to the back and removed from the front.**

```clean
functions:
	void processTaskQueue()
		list<string>.line tasks = []

		// Add tasks (to back)
		tasks.add("Task 1")
		tasks.add("Task 2")
		tasks.add("Task 3")

		// Process tasks (from front)
		iterate i in 1 to 3
			string currentTask = tasks.remove()  // Gets "Task 1", then "Task 2", etc.
			print("Processing: " + currentTask)
```

**Modified Operations**:
- `add(item)` → Adds to the **back** of the list
- `remove()` → Removes from the **front** of the list
- `peek()` → Views the **front** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`.pile` — Stack Behavior (LIFO)**

**Last-In-First-Out behavior: elements are added and removed from the same end (top).**

```clean
functions:
	void undoSystem()
		list<string>.pile actions = []

		// Perform actions (add to top)
		actions.add("Create file")
		actions.add("Edit text")
		actions.add("Save file")

		// Undo actions (remove from top)
		iterate i in 1 to 3
			string lastAction = actions.remove()  // Gets "Save file", then "Edit text", etc.
			print("Undoing: " + lastAction)
```

**Modified Operations**:
- `add(item)` → Adds to the **top** of the list
- `remove()` → Removes from the **top** of the list
- `peek()` → Views the **top** element without removing
- Standard list operations (`get(index)`, `size()`) remain unchanged

**`.unique` — Set Behavior (Uniqueness Constraint)**

**Only unique elements are stored; duplicate additions are silently ignored.**

```clean
functions:
	void trackUniqueVisitors()
		list<string>.unique visitors = []

		// Add visitors (duplicates ignored)
		visitors.add("Alice")    // Added
		visitors.add("Bob")      // Added
		visitors.add("Alice")    // Ignored (duplicate)
		visitors.add("Charlie")  // Added

		print("Unique visitors: " + visitors.length().toString())  // Prints: 3

		if visitors.contains("Alice")
			print("Alice has visited")
```

**Modified Operations**:
- `add(item)` → Adds only if `item` is not already present
- `remove()` → Removes from default position (front)
- `contains(item)` → Optimized for membership testing
- Standard list operations remain available

#### Behavior Combinations

**Behaviors combine by chaining dot suffixes on the type declaration.**

```clean
// Unique queue — FIFO with no duplicates
list<string>.line.unique uniqueQueue = []

// Unique stack — LIFO with no duplicates
list<integer>.pile.unique uniqueStack = []

// All three behaviors combined
list<integer>.line.unique.pile allFeatures = []
```

#### Available Methods

All list types support these methods regardless of behavior:

**Core Methods:**
- `add(item)` → Adds an item to the list (behavior determines position)
- `remove()` → Removes and returns an item (behavior determines which item)
- `peek()` → Views the next item to be removed without removing it
- `contains(item)` → Returns `true` if the item exists in the list
- `size()` → Returns the number of items in the list

**Standard List Methods:**
- `get(index)` → Gets item at specific index (0-based)
- `set(index, item)` → Sets item at specific index
- `isEmpty()` → Returns `true` if list is empty
- `isNotEmpty()` → Returns `true` if list contains items

**Behavior Declaration:**
- Behavior is declared as part of the type at variable declaration time: `list<T>.line`, `list<T>.pile`, `list<T>.unique`, or combinations thereof.

#### Performance Characteristics
- `.line`: O(1) add, O(1) remove, O(1) peek
- `.pile`: O(1) add, O(1) remove, O(1) peek
- `.unique`: O(1) add/contains (hash-based), O(1) remove

#### Advantages

1. **Unified Type System**: Single `list<any>` type instead of multiple collection types
2. **Consistent API**: All lists share the same base methods
3. **Flexible Behavior**: Properties can be changed at runtime if needed
4. **Type Safety**: Full generic type support with compile-time validation
5. **Simplicity**: Easier to learn and remember than separate collection classes
6. **Interoperability**: All property-modified lists are still `list<any>` types

#### Complete Example

```clean
start:
	// Test line behavior (FIFO queue)
	list<integer>.line lineList = []
	lineList.add(1)
	lineList.add(2)
	lineList.add(3)

	integer first = lineList.remove()   // Returns 1 (first in, first out)
	integer second = lineList.remove()  // Returns 2

	// Pile behavior (LIFO stack)
	list<integer>.pile pileList = []
	pileList.add(10)
	pileList.add(20)
	pileList.add(30)

	integer top = pileList.remove()     // Returns 30 (last in, first out)

	// Unique behavior (set)
	list<integer>.unique uniqueList = []
	uniqueList.add(100)
	uniqueList.add(200)
	uniqueList.add(100)  // Ignored (duplicate)

	boolean hasHundred = uniqueList.contains(100)  // Returns true
	integer listSize = uniqueList.length()            // Returns 2 (no duplicates)

	print("List demonstrates flexible behavior via type declaration")
```

### Nullable Types

Any type `T` can be made nullable by appending `?`. The resulting type `T?` means "either a `T` or `none`".

```clean
string? maybe = "hello"     // holds a string OR none
string? empty = none        // OK
string plain = none         // ERROR: plain `string` rejects none
```

Only a `T?` holds `none`. Every plain type rejects it — `integer`, `number`, `boolean`, `string`, `list<T>`, `pairs<K, V>`, `matrix<T>` and class types alike. `any` is the exception: it holds any value, `none` included. `null` is an accepted alias of `none` everywhere below.

**Rules:**

- **Assignment.** `none` is assignable to any `T?`, never to a plain `T`: at a declaration, an assignment, an argument, a return, a field initializer, or an element of a typed list or pairs literal. `T` is assignable to `T?` (widening). A `T?` value is NOT assignable to plain `T` without the `!` assertion operator (or a `default` fallback).
- **Members.** A `T?` may be none, so its fields, methods and elements are reached through `!`: `page!.title`, `names!.length()`, `scores!["a"]`.
- **`!` (required assertion).** `value!` traps at runtime if `value` is none; otherwise returns the underlying `T`. Use to move from `T?` to `T`.
- **`default` (none-coalescing).** `nullable default fallback` returns `nullable` when it is not none, otherwise `fallback`. The result has the type of `fallback` (without the `?` of `nullable`), so `integer n = maybe default 0` is a plain integer.
- **Equality.** `x == none`, `x != none` and `x is none` are the canonical none tests. After a successful `x != none` check, `x` is not narrowed automatically — use `!` if you need the underlying `T`.
- **Function parameters and return types.** Both may be declared nullable: `void greet(string? name)`, `Page? findPage(integer id)`. A function that returns `none` on some path declares a `T?` return type; `return none` from a plain `T` is an error.
- **Class fields.** Fields typed `T?` default to `none` when uninitialized. Fields typed `T` MUST be initialized before use.
- **`none` as a type.** `none` is also the type of the `none` value itself. It exists mainly so that `none` can be reasoned about in generic and inference contexts. It is assignable to every `T?`.

**Examples:**

```clean
functions:
	Page? findPage(integer id)
		return Database.first(Page)

start:
	Page? p = findPage(42)
	if p != none
		printl(p!.title)               // ! moves T? → T
	string? subtitle = none
	string shown = subtitle default "untitled"    // none-coalescing to a plain string
```

### Type Annotations and Variable Declaration

Variables use **type-first** syntax:

```clean
// Basic variable declarations
integer count = 0
number temperature = 23.5
boolean isActive = true
string name = "Alice"

// Uninitialized variables
integer sum
string message
```

### Type Conversion

**Implicit conversions (safe widening):**
- `integer` → `number` (with precision loss warning)
- Same-sign, wider types → OK
- `list<integer>` → `list<number>`: the elements are copied into a new list of numbers, so later changes to one list do not affect the other

**Explicit conversions (all require parentheses):**
```clean
value.toInteger()   // convert to integer
value.toNumber()    // convert to floating-point
value.toString()    // convert to string
value.toBoolean()   // convert to boolean
```

**Examples:**
```clean
integer num = 42
number numFloat = num.toNumber()      // ✅ Works: converts 42 to 42.0
integer piInt = 3.14.toInteger()      // ✅ Works: converts 3.14 to 3 (truncated)
boolean flag = 0.toBoolean()          // ✅ Works: converts 0 to false
boolean nonZero = 5.toBoolean()       // ✅ Works: converts 5 to true
```

---

[← Previous: Lexical Structure](./03-lexical-structure.md) | [Index](./README.md) | [Next: Apply-Blocks](./05-apply-blocks.md) →
