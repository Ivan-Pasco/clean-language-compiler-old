# 15. Classes and Objects

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Classes are the primary mechanism for expressing domain behavior and responsibility in Clean Language.

**Design philosophy:**
- Long-lived behavior and stateful logic should be expressed as class methods
- Application behavior (update, render, interaction, I/O coordination) naturally lives in classes
- Entry blocks (`start:`) should delegate to classes rather than implement domain logic directly
- Clean does not require all logic to be object-oriented, but encourages classes as the natural home for complex or evolving behavior

### Class Definition

**All class methods must be declared within a `functions:` block:**

```clean
class Point
	integer x
	integer y

	constructor(integer x, integer y)

	functions:
		integer distanceFromOrigin()
			return sqrt(x * x + y * y)

		void move(integer dx, integer dy)
			x = x + dx
			y = y + dy
```

### Generic Classes with `any`

Clean Language uses `any` for generic class fields and methods:

```clean
class Container
	any value

	constructor(any value)

	functions:
		any get()
			return value

		void set(any newValue)
			value = newValue
```

### Inheritance

Clean Language supports single inheritance using the `is` keyword. Child classes inherit all public fields and methods from their parent class.

```clean
class Shape
    string color
    
    constructor(string colorParam)
        color = colorParam          // Implicit context - no 'this' needed
    
    functions:
        string getColor()
            return color            // Direct field access

class Circle is Shape
    number radius
    
    constructor(string colorParam, number radiusParam)
        base(colorParam)            // Call parent constructor with 'base'
        radius = radiusParam        // Implicit context
    
    functions:
        number area()
            return 3.14159 * radius * radius
        
        string getInfo()
            return color + " circle"    // Access inherited field directly
```

#### Inheritance Features

- **Syntax**: Use `class Child is Parent` to inherit from a parent class
- **Base Constructor**: Use `base(args...)` to call the parent constructor
- **Implicit Context**: No need for `this` or `self` - fields are directly accessible
- **Name Safety**: Parameters must have different names than fields to prevent conflicts
- **Method Inheritance**: Child classes inherit all public methods from parent classes
- **Field Inheritance**: Child classes inherit all public fields from parent classes
- **Method Overriding**: Child classes can override parent methods by defining methods with the same name

#### Implicit Context Rules

Clean Language uses implicit context for accessing class fields:

- ✅ `color = colorParam` (field assignment — implicit `this`)
- ✅ `return color` (field access — implicit `this`)
- ✅ `radius = radiusParam` (works in child classes too)
- ✅ `this.render()` (explicit self-method call)
- ✅ `this.name` (explicit field access — equivalent to just `name`)
- ❌ Parameter names cannot match field names (compiler enforced)

Fields can be accessed directly by name (implicit `this`) or with explicit `this.field`. The `this` keyword is available inside all class methods and refers to the current instance. Explicit `this` is useful only for self-method calls.

### Capabilities (`can`)

A **capability** describes something a class can do. It is a named contract of methods that any class may claim to fulfil. When a class declares `can Draw`, it promises to provide every method the `Draw` capability requires, and becomes usable anywhere a value of type `Draw` is expected.

Capabilities solve the "several things that play the same role" problem: instead of writing one function per concrete type, you write one function that accepts the capability.

#### Declaring a capability

```clean
can Draw:
    draw()

can Describe:
    describe() -> string
    tag() -> string
        return "[" + this.describe() + "]"   // default body
```

Each item in a `can:` block is a method signature. A signature may include an optional default body indented beneath it. Methods without a body are **required** — every class that claims the capability must implement them. Methods with a body are **defaults** — classes inherit the body unless they override it.

#### Claiming a capability

A class claims one or more capabilities using `can` after its (optional) `is` clause:

```clean
class Circle is Shape can Draw, Describe
    public:
        number radius

    functions:
        public:
            draw()
                print("circle")
            describe() -> string
                return "circle"
            // tag() not written — Circle inherits Describe's default
```

Order: `class Name [is Parent] [can C1, C2, ...]`. When both clauses are present, `is` comes first.

Capabilities are **nominal**: a class that happens to have a `draw()` method does *not* have the `Draw` capability. It must say `can Draw`.

**Naming convention.** Because the keyword is `can`, the capability name should read as an action the class can perform. Prefer bare verbs: `can Draw`, `can Print`, `can Close`, `can Serialize`, `can Compare`, `can Iterate`. `class Circle can Draw` reads naturally in English; adjective forms like `can Drawable` do not. This is a stylistic convention (not enforced by the compiler) but every example in this specification follows it.

#### Using a capability as a type

Capability names may appear anywhere a type is expected — parameters, return types, variable declarations, and generic type arguments:

```clean
functions:
    render(Draw thing)
        thing.draw()

    renderAll(list<Draw> things)
        iterate item in things
            render(item)
```

At the call site of a capability-typed value, method dispatch is **dynamic**: the actual class of the value at runtime determines which implementation runs. This lets one list hold several different classes that share the same capability.

Implementation: each class instance carries a 4-byte class-id header. When you call a method on a capability-typed value, the compiler emits a runtime switch on that class-id that routes to the correct concrete method.

#### Interaction with inheritance

- A class inherits every capability its parent has. If `Shape can Draw`, then `Circle is Shape` also `can Draw` — no need to redeclare.
- A child class may claim additional capabilities beyond its parent's.
- A capability method implemented on the parent satisfies the capability for the child, unless the child overrides it.

#### What capabilities are not

- Not classes: you cannot instantiate a capability.
- Not structural: a class must explicitly claim the capability.
- Not generic in v1: `can Comparable<T>` is not yet supported.
- Not composable in v1: one capability cannot extend another.

### Object Creation and Usage

```clean
start:
    // Create objects
    Point point = Point(3, 4)
    Circle circle = Circle("red", 5.0)

    // Call methods (parentheses required)
    integer distance = point.distanceFromOrigin()
    point.move(1, -2)

    // Access properties
    integer xCoord = point.x
    string color = circle.color
```

### Static Methods

You can call class methods directly on the class name if they don't use instance fields:

```clean
class MathUtils
    functions:
        number add(number a, number b)
            return a + b
        
        number max(number a, number b)
            return if a > b then a else b

class DatabaseService
    functions:
        boolean connect(string url)
            // implementation that doesn't use instance fields
            return true
        
        User findUser(integer id)
            // implementation that doesn't use instance fields
            return User.loadFromDatabase(id)

// Static method calls - ClassName.method()
start:
    number result = MathUtils.add(5.0, 3.0)
    number maximum = MathUtils.max(10.0, 7.5)
    boolean connected = DatabaseService.connect("mysql://localhost")
    User user = DatabaseService.findUser(42)
```

**Rules for Static Methods:**
- Use `ClassName.method()` syntax for static calls
- Only allowed if the method doesn't access instance fields (`field`)
- All methods must be in `functions:` blocks
- Method calls require parentheses: `MathUtils.add()` not `MathUtils.add`
- Ideal for helpers, services, utilities, and database access functions

**Example - Mixed Static and Instance Methods:**
```clean
class User
    string name
    integer age
    
    constructor(string name, integer age)
    
    functions:
        // Instance method - accesses fields
        string getInfo()
            return "User: {name}, Age: {age}"
        
        // Static method - no field access
        boolean isValidAge(integer age)
            return age >= 0 and age <= 150

// Usage
start:
    User user = User("Alice", 25)
    string info = user.getInfo()                    // Instance method call
    boolean valid = User.isValidAge(30)             // Static method call
```

### Design Philosophy: Flexible Organization

Clean Language supports both class-based organization and top-level functions, providing flexibility for different coding styles and project needs:

#### Class-Based Organization (Recommended for complex projects)
- **Better code organization**: Related functionality is grouped together
- **Namespace management**: No global function name conflicts  
- **Consistent syntax**: All method calls use the same `Class.method()` or `object.method()` pattern
- **Extensibility**: Easy to add related methods to existing classes

```clean
class Calculator
    functions:
        number calculateTax(number amount)
            return amount * 0.15
        
        string formatResult(number value)
            return "Result: " + value.toString()
```

#### Top-Level Functions (Suitable for simpler projects)
- **Direct approach**: Functions can be declared directly in `functions:` blocks
- **Simplicity**: No need for class wrapper when functionality is standalone
- **Scripting style**: Perfect for utility scripts and simple programs

```clean
start:
    number tax = calculateTax(100.0)
    string result = formatResult(tax)
    print(result)

functions:
    number calculateTax(number amount)
        return amount * 0.15

    string formatResult(number value)
        return "Result: " + value.toString()
```

**Both approaches are valid and can be mixed within the same program.** The choice depends on project complexity and developer preference.

---

[← Previous: Error Handling](./14-error-handling.md) | [Index](./README.md) | [Next: Standard Library](./16-standard-library.md) →
