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
    public:
        string color

    constructor(string colorParam)
        color = colorParam          // Implicit context - the field by its name

    functions:
        public:
            string getColor()
                return color        // Direct field access

class Circle is Shape
    number radius

    constructor(string colorParam, number radiusParam)
        base(colorParam)            // Call parent constructor with 'base'
        radius = radiusParam        // Implicit context

    functions:
        public:
            number area()
                return 3.14159 * radius * radius

            string getInfo()
                return color + " circle"    // Access inherited public field directly
```

#### Inheritance Features

- **Syntax**: Use `class Child is Parent` to inherit from a parent class
- **Base Constructor**: Use `base(args...)` to call the parent constructor
- **Implicit Context**: Fields and methods are referred to by their bare names; Clean has no `this` or `self`
- **Name Safety**: Parameters must have different names than fields to prevent conflicts
- **Method Inheritance**: Child classes inherit all public methods from parent classes
- **Field Inheritance**: Child classes inherit all public fields from parent classes
- **Method Overriding**: Child classes can override parent methods by defining methods with the same name
- **Private members are not inherited**: a child class that reads, writes or calls its parent's private field or method gets SEM005 (`Private members are not inherited: \`Kid\` cannot use \`Base\`'s private field \`secret\``)

#### Method Overriding

When a child class defines a method with the same name as a parent method, the object's own class decides which method runs. This holds however the value is typed — a variable, parameter, list element, field or return value typed as the parent still runs the child's method:

```clean
start:
    Shape s = Circle("blue", 1.0)
    print(s.describe())          // circle
    show(Shape("green"))         // shape

class Shape
    public:
        string color

    constructor(string colorParam)
        color = colorParam

    functions:
        public:
            string describe()
                return "shape"

class Circle is Shape
    number radius

    constructor(string colorParam, number radiusParam)
        base(colorParam)
        radius = radiusParam

    functions:
        public:
            string describe()
                return "circle"

functions:
    show(Shape x)
        print(x.describe())      // runs the method of x's own class
```

Private methods are not overridden. A parent's call to its own private method always runs the parent's version, even when the object is a child that declares a method with the same name.

**Override rules (SEM006):**
- An override takes the same parameter types and returns the same type as the parent method (`Method 'area' of class 'Square' overrides 'Shape.area' with a different signature`).
- An override of a public method must itself be under `public:` (`Method 'area' of class 'Square' overrides the public method 'Shape.area' but is private`).

#### Calling the Parent's Method with `base`

Inside a child method, `base.method(args)` runs the implementation of the nearest ancestor that defines `method`, on the same object. It is never dispatched to an override, so the parent's code runs even when the object belongs to a deeper subclass:

```clean
start:
    Shape s = Circle("red", 2.0)
    print(s.describe())          // [shape]

class Shape
    public:
        string color

    constructor(string colorParam)
        color = colorParam

    functions:
        public:
            string describe()
                return "shape"

class Circle is Shape
    number radius

    constructor(string colorParam, number radiusParam)
        base(colorParam)
        radius = radiusParam

    functions:
        public:
            string describe()
                return "[" + base.describe() + "]"
```

`base.method()` naming a method no ancestor defines is SEM002 (`No parent class of 'Circle' defines a method 'missing'`); naming a private method of the ancestor is SEM005.

#### Default `toString()`

Every class has a `toString()`. Unless the class declares or inherits its own, it returns the name of the object's own class — also through a parent-typed variable or list: a `Dog` held in a `list<Animal>` prints `Dog`. `print`, `printl` and `"{x}"` use it.

#### Implicit Context Rules

Inside a class, the object's own fields and methods are referred to by their bare names:

- ✅ `color = colorParam` (field assignment)
- ✅ `return color` (field access)
- ✅ `radius = radiusParam` (works in child classes too)
- ✅ `render()` (call one of the object's own methods)
- ❌ `this.name`, `this.render()` or a bare `this` — Clean has no `this`: SYN002 `` `this` is not used in Clean: refer to the field or method by its name (`count`, `clear()`) ``
- ❌ Parameter names cannot match field names (compiler enforced, CLASS007) — except in a constructor without a body, whose parameters name the fields they set: `constructor(integer x, integer y)` stores both arguments

Because a name is never reused for a parameter and a field, a bare name is never ambiguous.

**Name lookup inside a method or constructor.** A bare name is looked up in this order:

1. local variables and parameters;
2. the class's own members — its fields, and its methods including inherited public methods and capability methods;
3. top-level functions and state.

So a method named like a top-level function is called by the bare name inside the class.

### Capabilities (`can`)

A **capability** describes something a class can do. It is a named contract of methods that any class may claim to fulfil. When a class declares `can Draw`, it promises to provide every method the `Draw` capability requires, and becomes usable anywhere a value of type `Draw` is expected.

Capabilities solve the "several things that play the same role" problem: instead of writing one function per concrete type, you write one function that accepts the capability.

#### Declaring a capability

```clean
can Draw:
    draw()

can Describe:
    string describe()
    string tag()
        return "[" + describe() + "]"   // default body
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
            string describe()
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

Implementation: each class instance carries a 4-byte class-id header. When you call a method on a capability-typed value, the compiler emits a runtime switch on that class-id that routes to the correct concrete method. The same header drives method overriding: a call to an overridden method through a parent-typed value switches on the class-id to run the object's own class's method.

#### Interaction with inheritance

- A class inherits every capability its parent has. If `Shape can Draw`, then `Circle is Shape` also `can Draw` — no need to redeclare.
- A child class may claim additional capabilities beyond its parent's.
- A capability method implemented on the parent satisfies the capability for the child, unless the child overrides it.
- A method that fulfils a capability must be declared under `functions: public:`. A private implementation is SEM011 (`Method 'area' on class 'Square' fulfils capability 'Shape' but is private`), even when every call goes through the capability type.

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
        public:
            number add(number a, number b)
                return a + b

            number max(number a, number b)
                return if a > b then a else b

class DatabaseService
    functions:
        public:
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
- `ClassName.method()` has no object to run on, so the method must not need one: it may not read or write a field of the class (its own or inherited), call a method of the object that does, or be `toString()`. Otherwise it is CLASS008 — ``'get()' needs a Box object: create one (`Box b = Box()`) and call `b.get()` ``. A method that uses no field or method of the object stays callable through the class name, whatever fields the class has
- All methods must be in `functions:` blocks
- From outside the class, `ClassName.method()` reaches only methods under `functions: public:`; calling a private one is SEM005 (`'square' is private and cannot be accessed from outside 'Calc'`)
- Method calls require parentheses: `MathUtils.add()` not `MathUtils.add`
- Ideal for helpers, services, utilities, and database access functions

**Example - Mixed Static and Instance Methods:**
```clean
class User
    string name
    integer age
    
    constructor(string name, integer age)
    
    functions:
        public:
            // Instance method - accesses fields
            string getInfo()
                return "User: {name}, Age: {age}"

            // Static method - no field access
            boolean isValidAge(integer years)
                return years >= 0 and years <= 150

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
