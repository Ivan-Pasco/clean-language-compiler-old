# Clean Language Developer Guide: Plugins, Memory & Type Inference

## Welcome to Clean Language's Advanced Features

Clean Language makes programming easier with smart features that work behind the scenes. This guide explains how to use our plugin system, automatic memory management, and type inference to build better applications faster.

## Understanding the Plugin System

### What Are Plugins?

Think of plugins as optional toolkits that add functions to your Clean Language programs. Want to work with files? Enable the File plugin. Need advanced math? Turn on the Math plugin. This modular approach means your programs only include what they actually use, making them faster and smaller.

### Available Plugin Modules

Clean Language comes with several built-in plugin modules you can use:

#### 🧠 **Memory Plugin** (Essential)
**What it does**: Handles all memory operations automatically - you don't need to worry about it!
**Functions**: Automatic memory allocation and cleanup
**When you need it**: Always enabled (required by other plugins)

#### 🔢 **Math Plugin** 
**What it does**: Provides mathematical functions and operations
**Functions**: `sin()`, `cos()`, `sqrt()`, `pow()`, logarithms, and more
**When you need it**: Calculations, scientific computing, graphics
**Options**: Choose between single or double precision

#### 📝 **String Plugin**
**What it does**: Text processing and manipulation
**Functions**: `concat()`, `length()`, `split()`, `replace()`, formatting
**When you need it**: Working with text, parsing, string operations
**Options**: Support for different text encodings (UTF-8 is default)

#### 📋 **List Plugin**
**What it does**: Array and list operations
**Functions**: `push()`, `pop()`, `sort()`, `filter()`, `map()`
**When you need it**: Working with collections of data
**Options**: Set default list size and growth behavior

#### 💻 **Console Plugin**
**What it does**: Input and output to the terminal/console
**Functions**: `print()`, `println()`, `input()`, colored output
**When you need it**: Displaying results, debugging, user interaction
**Options**: Buffered or immediate output modes

## How to Use Plugins in Your Programs

### Using Default Plugins (Recommended)

Most Clean Language programs work great with the default plugin setup:

```clean
// Your Clean Language program automatically has access to:
start()
    list<integer> numbers = [1, 2, 3, 4, 5]    // List plugin
    integer sum = numbers.reduce(+)             // Math plugin  
    println("Total: " + sum.toString())         // Console + String plugins
```

### Customizing Which Plugins to Use

Clean Language automatically loads the plugins you need based on the functions you use:

```clean
functions:
    number calculateArea(number radius)
        return 3.14159 * (radius ^ 2)   // Uses math plugin automatically

start()
    number result = calculateArea(5.0)
    println("Area: " + result.toString())  // Uses string + console plugins automatically
```

### Plugin Configuration Options

Most plugins work great with their default settings, but Clean Language automatically optimizes based on your usage:

```clean
// Clean Language automatically detects when you need:
functions:
    void mathematicalCalculation()
        // High precision math - automatically uses double precision when needed
        number precise = math.sqrt(2.0)
        
    void displayResults()
        // Console output - automatically optimized for your terminal
        println("Result ready")
```

### What Happens Behind the Scenes

Don't worry about the technical details - Clean Language handles plugin management automatically:

- **Smart Loading**: Plugins are loaded in the right order (memory first, then others)
- **No Conflicts**: The system prevents different plugins from having the same function names
- **Efficient**: Your program only includes the functions you actually use
- **Safe**: All plugins are tested to work well together

## Automatic Memory Management - No More Worries!

### You Don't Need to Think About Memory

One of Clean Language's best features is that you never have to manually manage memory. Here's what that means for you:

**✅ What Clean Language Handles For You:**
- Automatically allocates memory when you create variables
- Automatically frees memory when variables are no longer needed  
- Prevents memory leaks and crashes
- Protects against buffer overflows and corruption
- Ensures your program runs safely and efficiently

**❌ What You Don't Have To Do:**
- Call `malloc()` or `free()` functions
- Worry about memory leaks
- Debug segmentation faults
- Calculate buffer sizes manually

### Simple Memory Usage Examples

```clean
// Clean Language automatically manages all of this:
start()
    string name = "Alice"                      // Memory allocated for string
    list<integer> numbers = [1, 2, 3, 4, 5]   // Memory allocated for list
    // Note: Object syntax would use classes in Clean Language
    
functions:
    void processData()
        // When variables go out of scope, memory is automatically freed
        string tempData = loadLargeFile()   // Memory allocated
        process(tempData)
        // tempData memory automatically freed when function ends
```

### Advanced Memory Settings (Optional)

Most developers never need to change these settings - Clean Language handles memory automatically based on your program's needs:

```clean
// Clean Language automatically optimizes memory for your program
start()
    // For large data processing, Clean Language automatically allocates more memory
    list<string> largeDataSet = loadBigFile("data.cln")
    
    // Memory is automatically managed and optimized
    processLargeData(largeDataSet)
```

**When you might want to adjust memory settings:**
- **Large Data Processing**: Increase initial heap size for better performance
- **Embedded Systems**: Reduce memory footprint for resource-constrained devices
- **Development**: Enable debug mode to catch memory issues early

## Smart Type Detection - Write Less, Get More Safety

### What is Type Inference?

Clean Language can automatically figure out what type your variables should be, so you don't have to specify them manually. This gives you the safety of typed languages with the convenience of dynamic ones.

**The Best of Both Worlds:**
- **Safe**: Catches type errors before your program runs
- **Convenient**: Less typing, cleaner code
- **Smart**: Figures out complex types automatically
- **Flexible**: Add explicit types when you need them

### How Type Inference Works in Practice

```clean
// Clean Language figures out all these types automatically:
start()
    integer x = 42                          // Clean Language knows: x is integer  
    number y = x + 3.14                     // Clean Language knows: y is number
    list<string> names = ["Alice", "Bob"]   // Clean Language knows: names is list<string>

functions:
    // Functions with automatic type detection:
    any add(any a, any b)              // Clean Language figures out types from usage
        return a + b                   // and that the function returns appropriate type

start()
    number result = add(10, 20.5)      // Clean Language knows: result is number
```

### When You Might Want Explicit Types

Most of the time, type inference works perfectly. But sometimes you want to be explicit:

```clean
// Explicit types for clarity in public APIs:
functions:
    ProcessResult processUserData(integer userId, UserData data)
        // Function signature is clear to other developers
        return processData(userId, data)

start()
    // Explicit types to catch errors early:
    integer userCount = getUserCount()  // Ensures function returns integer
    
    // Note: Complex generics would use Clean Language's container syntax
    // pairs<string, list<User>> cache = createCache()
```

### Smart Type Checking Features

Clean Language's type system automatically understands what operations are valid on different types:

**✅ Automatic Operation Validation:**
- **Numbers**: Can use `+`, `-`, `*`, `/`, comparison operators
- **Strings**: Can concatenate, get length, slice, compare
- **Lists**: Can add/remove items, iterate, access by index
- **Objects**: Can access properties, call methods
- **Functions**: Can be called with the right parameters

**Example of Automatic Type Checking:**
```clean
start()
    integer age = 25
    string name = "Alice"
    list<integer> scores = [85, 92, 78]

    // ✅ These work - Clean Language knows the types support these operations:
    integer nextYear = age + 1          // Numbers support addition
    string greeting = "Hello " + name   // Strings support concatenation  
    integer firstScore = scores.get(0)  // Lists support indexing

    // ❌ These cause helpful errors before your program runs:
    // integer invalid = name + scores[0]  // Error: Cannot add string and integer
    // string invalidAccess = name.get(100)  // Error: strings don't have get() method
```

## Helpful Error Messages - Debug with Confidence

### Clear, Actionable Error Messages

Clean Language provides detailed error messages that help you fix problems quickly:

**🎯 What You Get:**
- **Exact Location**: Shows exactly where the error occurred
- **Clear Explanation**: Tells you what went wrong in plain English  
- **Fix Suggestions**: Suggests how to solve the problem
- **Related Issues**: Points out related errors that might help

### Examples of Helpful Errors

```clean
// Type mismatch error:
start()
    string age = "25"
    integer nextYear = age + 1  // Error here

// Error message you'll see:
// Error at line 3, column 17:
// Cannot add string and integer
// 
// Suggestion: Convert the string to a number first:
//   integer nextYear = age.toInteger() + 1
// Or declare age as an integer:
//   integer age = 25
```

```clean
// Function parameter error:
functions:
    string greet(string name)
        return "Hello " + name

start()
    greet()  // Missing argument

// Error message you'll see:
// Error at line 6, column 1:
// Function 'greet' expects 1 argument, but 0 were provided
//
// Expected: greet(name)
// Got: greet()
```

### Error Prevention Features

Clean Language catches many errors before your program runs:

- **Syntax Errors**: Catches typos and formatting issues
- **Type Errors**: Ensures operations match data types
- **Logic Errors**: Warns about unreachable code or unused variables
- **Runtime Prevention**: Prevents common crashes like null pointer access

### Writing Error-Safe Code

```clean
// Clean Language helps you handle errors gracefully:
functions:
    number safeDivide(number a, number b)
        if b == 0
            error("Cannot divide by zero - please provide a non-zero divisor")
        return a / b

start()
    number result = safeDivide(10, 0) onError 0
    println("Math result: " + result.toString())
    // Program continues running instead of crashing
```

## What's New in Clean Language

### Modern Type System Features

Clean Language now includes advanced type features that make your code safer and more expressive:

#### 🔄 **Automatic Type Detection**
Variables and functions get their types automatically figured out:
```clean
start()
    integer userId = 12345                   // Automatically: integer
    string userName = fetchName(userId)      // Automatically: string (based on function)
    list<User> users = [user1, user2, user3]  // Automatically: list<User>
```

#### 🧬 **Generic Functions** 
Write functions that work with any type:
```clean
// This function works with any list type:
functions:
    any firstItem(list<any> items)
        return items.get(0)

start()
    // Use it with different types:
    integer firstNumber = firstItem([1, 2, 3])        // Returns integer
    string firstName = firstItem(["A", "B", "C"])     // Returns string
```

#### 🎭 **Flexible Type Options**
Handle data that can be multiple types using `any`:
```clean
functions:
    void processId(any id)
        // Clean Language handles different types automatically
        string idStr = id.toString()
        println("Processing ID: " + idStr)

start()
    processId(12345)        // Works with integer
    processId("USER123")    // Works with string
```

#### 🏷️ **Custom Class Types**
Create meaningful types using classes:
```clean
class UserId
    integer id
    constructor(integer value)
        id = value

class Email  
    string address
    constructor(string addr)
        address = addr

functions:
    User createUser(UserId id, Email email, integer age)
        // Clear what each parameter should be
        return User(id.id, email.address, age)
```

## Getting Started with Clean Language's Advanced Features

### Quick Start Guide

1. **Write Normal Code** - Clean Language handles the complex stuff automatically:
```clean
start()
    list<User> users = loadUsersFromFile("users.cln")  // Memory managed automatically
    list<User> activeUsers = users.filter(u => u.isActive)  // Type inferred automatically
    println("Found " + activeUsers.length().toString() + " active users")  // Plugins loaded automatically
```

2. **Use Standard Syntax** - Clean Language automatically optimizes:
```clean
functions:
    void processData()
        // Clean Language automatically uses the right plugins and optimization
        number result = math.sqrt(16.0)    // Math plugin loaded automatically
        println("Result: " + result.toString())  // String + Console plugins loaded automatically
```

3. **Add Explicit Types for APIs** - Make your intentions clear:
```clean
functions:
    void processUserData(integer userId, User userData)
        // Other developers know exactly what this function expects
        processUser(userId, userData)
```

### All Available Plugin Categories

Clean Language organizes functions into logical groups:

| Category | What It Does | Example Functions |
|----------|-------------|------------------|
| **Math** | Numbers and calculations | `sin()`, `cos()`, `sqrt()`, `pow()` |
| **String** | Text processing | `concat()`, `split()`, `replace()`, `format()` |
| **List** | Collections and arrays | `push()`, `pop()`, `sort()`, `filter()`, `map()` |
| **Console** | Terminal input/output | `print()`, `println()`, `input()`, `clear()` |
| **File** | File system access | `read_file()`, `write_file()`, `exists()` |
| **Network** | Web and HTTP | `fetch()`, `post()`, `download()` |
| **Async** | Asynchronous programming | `await`, `Promise`, `timeout()` |
| **Type** | Type conversions | `parseInt()`, `toString()`, `typeof()` |
| **Error** | Error handling | `try`, `catch`, `throw`, `assert()` |
| **Test** | Testing utilities | `test()`, `assert_equal()`, `mock()` |

## Performance Benefits

### Why Clean Language is Fast

**🚀 Plugin System Benefits:**
- **Fast Function Calls**: Functions are looked up instantly
- **Small Program Size**: Only includes functions you actually use  
- **Quick Startup**: Plugins load only when needed
- **Efficient Memory**: Each plugin reports exactly how much memory it needs

**⚡ Type Inference Benefits:**
- **Compile-Time Checking**: Catches errors before running, no runtime type checks needed
- **Optimized Code**: Compiler knows exact types, generates faster WebAssembly
- **Smart Analysis**: Only re-analyzes code that changed during development

**💾 Memory Management Benefits:**
- **Zero Overhead**: Memory operations are as fast as manual management
- **No Garbage Collection Pauses**: Reference counting happens immediately
- **Predictable Performance**: No surprise slowdowns from garbage collection
- **Optimized Allocation**: Uses memory pools for common allocation patterns

## Best Practices for Clean Language Developers

### Writing Efficient Code

```clean
// ✅ Good: Let types be inferred for cleaner code
start()
    list<User> users = fetchUsers()
    integer activeCount = users.filter(u => u.active).length()

functions:
    // ✅ Good: Use explicit types for public APIs  
    number calculateTax(number income, number rate)
        return income * rate

// ✅ Good: Clean Language automatically loads only needed plugins
// No manual plugin configuration needed - it's automatic!

// ❌ Avoid: Unnecessary explicit types in simple cases
start()
    integer count = 0  // Good, but could also use: count = 0 with inference
```

### Debugging Tips

```clean
// Clean Language provides automatic debugging help:
functions:
    // Use explicit types to catch errors early:
    number criticalCalculation(number value)
        // Ensures value is definitely a number
        return value * 2.0

start()
    // Take advantage of helpful error messages:
    number result = riskyOperation() onError 0.0
    println("Result: " + result.toString())  // Clean Language provides detailed messages automatically
```

## Common Questions and Solutions

### "My program seems slow to start"
**Solution**: Clean Language automatically optimizes startup time. If it's still slow, you might be loading large data files:
```clean
start()
    // Load only what you need initially
    integer userCount = getUserCount()  // Fast operation
    // Load full data only when needed
    // list<User> allUsers = loadAllUsers()  // Slower operation
```

### "I'm getting type errors I don't understand"  
**Solution**: Add explicit types to clarify your intent:
```clean
start()
    string result = someFunction()  // Make your expectations clear
```

### "My program uses too much memory"
**Solution**: Check if you're creating large data structures unnecessarily:
```clean
start()
    // Use smaller, more specific data structures
    list<integer> userIds = getUserIds()  // Instead of full User objects
    // Process data in smaller chunks when possible
```

### "I want to use a function that doesn't exist"
**Solution**: Check if you're using the right syntax for built-in functions:
```clean
start()
    // Math functions:
    number result = math.sqrt(16.0)
    
    // String operations:  
    string upper = text.toUpperCase()
    
    // List operations:
    numbers.add(42)
```

## What's Coming Next

Clean Language is continuously improving. Here's what we're working on:

### 🔮 **Future Features**
- **Hot Reloading**: Change code without restarting your program
- **Better IDE Support**: Smarter autocomplete and error highlighting  
- **More Plugins**: Database access, image processing, machine learning
- **Performance Boosts**: Even faster compilation and execution
- **Advanced Types**: More sophisticated type checking for complex programs

### 🛠️ **How You Can Help**
- **Try Clean Language**: Use it for your projects and tell us what works
- **Report Issues**: Let us know if something doesn't work as expected
- **Suggest Features**: Tell us what plugins or features you'd like to see
- **Share Examples**: Show us the cool things you build with Clean Language

---

## Summary

Clean Language makes programming enjoyable by handling the complex stuff automatically:

- **🔌 Plugin System**: Only include the functions you need
- **🧠 Smart Types**: Automatic type detection with helpful error messages  
- **💾 Safe Memory**: Never worry about memory leaks or crashes
- **⚡ Fast Performance**: Compiled to efficient WebAssembly
- **🎯 Clear Errors**: Helpful messages that actually help you fix problems

**Ready to get started?** Just write your code - Clean Language will handle the rest!