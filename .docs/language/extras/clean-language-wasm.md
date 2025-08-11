# 🌟 Clean Language Specification

## 📝 Overview

Clean Language is a statically-typed programming language designed for readability and safety. It features classes, functions, and basic control structures with explicit type declarations.

## 📄 File Structure

- **Main Files**: `.cln` - Contains Clean Language code
- **Support Files**: 
  - `.html` - For web integration
  - `.css` - For styling
  - `.test.cln` - For testing

## 🚀 Program Entry Point

Every Clean Language program begins execution in the `start()` method:

* Only one `start()` method should exist per project
* It does not require a surrounding class
* The `start()` method is always invoked first
* This lets you focus on what objects *do*, not what they *are named*

**Example:**
```clean
start()
  print "Welcome to Clean Language!"
  print "This is the beginning of your program."
```

## 💨 Print Instructions

* `print`: prints with a newline
* `printl`: prints on the same line
* Strings are concatenated automatically; no `+` needed
* Use `{{ ... }}` for string interpolation

**Examples:**
```clean
print "Hello, world!"
printl "Loading..."
printl " Done."
print "Welcome, {{ name }}!"
print "Total: {{ price * quantity }} units."
```

## 🴢 Comments

* Single-line: `//`
* Multi-line: `/* ... */`

## 🔠 Data Types

**Signed vs Unsigned:**
* Signed: allows positive and negative values
* Unsigned: allows only positive values and zero

**Basic Types:**

| Type     | Description              | Default | Use Case               |
|----------|-------------------------|---------|------------------------|
| boolean  | true/false values       | false   | Conditions, flags      |
| string   | Text (UTF-8)           | ""      | Text handling          |
| byte     | 8-bit unsigned integer | 0       | Small counters, flags  |
| number   | 64-bit floating point  | 0.0     | Decimal calculations   |
| integer  | 32-bit signed integer  | 0       | Counting, indices      |

**Extended Types (Advanced Use):**

| Type     | Description              | Use Case                        |
|----------|-------------------------|--------------------------------|
| unsigned | 32-bit unsigned integer | Indexing, counters, crypto      |
| long     | 64-bit signed integer   | File offsets, large counters    |
| big      | 128-bit signed integer  | Precise calculations, crypto    |
| ulong    | 64-bit unsigned integer | Hashing, crypto, large indexing |
| ubig     | 128-bit unsigned integer| Cryptographic key operations    |

## 🧾 Variable Declarations

Every class and function begins with a setup block:

* `constants:` - Used for fixed values
* `input:` - For function parameters
* `description:` - Documents the purpose
* Variable declarations
* Always leave a blank line **after the setup block**

**Examples:**
```clean
class Config
  constants:
    string:
      - version = "1.0.0"
    number:
      - maxUsers = 100

class User
  public
    string:
      - name = "John"
      - email = "user@example.com"
    integer:
      - age = 25

functions:
  calculateBonus() returns number
    number:
      - base = 100
      - multiplier = 1.5

    return base * multiplier
```

## 🗓️ Lists

Lists can be single or multi-dimensional:

```clean
number[] numbers = 1,2,3,4
string[] letters = 'a','b','c'
print numbers[1]
print letters.count

number[][] matrix = [ [1, 2], [3, 4] ]
print matrix[1][2]

string[][] phrases = [ ["hi"], ["hello", "world"] ]
print phrases[2][1]
```

## ➕ Operators

* Left-to-right evaluation
* Parentheses required for expressions with multiple operators
* No implicit coercion (except in `print`)

**Helpers:**
```clean
string toString(number n)
number toNumber(string s)
```

**Operators:**
* Arithmetic: `+`, `-`, `*`, `/`
* Comparison: `<`, `>`, `<=`, `>=`, `is`, `not`
* Logical: `and`, `or`

## 🔄 Control Structures

```clean
if x is true
  print "OK"

a = from 1 to 9        // 1,2,3,4,5,6,7,8,9
b = from 1 to 9 step 2 // 1,3,5,7,9

iterate colors in c
  print c
```

## 🪩 Functions

Functions are defined inside `functions:` block with optional return types:

```clean
class Math
  public
    number base

  functions:
    setBase()
      description: Sets a new base value
      input:
        number b = 0

      number result = b
      base = result

    addToBase() returns number
      description: Adds a number to base and returns the result
      input:
        number x = 0

      number total = base + x
      return total
```

## 📅 Classes

* System-defined classes (e.g. `Global`, `Math`) must include `description:`
* Optional for user-defined classes
* Clean uses single inheritance (one base class only)
* Supports static duck typing

**Example:**
```clean
class Car
  description: Represents a car with attributes
  public
    string color = "red"
    unsigned wheels = 4
  private
    static integer serialNumber = 45

  constructor()
    input:
      string c = ""
      unsigned w = 0

    color = c
    wheels = w

  functions:
    accelerate()
      description: Increases speed

      if speed < 100
        speed++

    getSpeed() returns number
      description: Returns current speed

      number result = speed
      return result
```

**Inheritance Example:**
```clean
class Vehicle
  public
    number speed = 0

  functions:
    move()
      speed = speed + 10

class Bike basedOn Vehicle
  public
    string type = "mountain"
```

**Duck Typing Example:**
```clean
object something:
  string name = "Rubber Duck"
  quack()
    print "Quack!"

something.quack()  // Works because 'something' has a 'quack' method
``` 