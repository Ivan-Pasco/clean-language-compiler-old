# 16. Standard Library

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Clean Language provides built-in utility classes for common operations. All standard library classes follow the compiler instructions:

- All methods are in `functions:` blocks
- Method calls require parentheses
- No `Utils` suffix in class names
- Use `any` for generic operations

### Math Module

The math module follows Clean Language's "one way to do things" principle. Basic arithmetic operations use operators, while advanced mathematical functions use methods.

**Basic Arithmetic - Use Operators:**
- Addition: `a + b` (not `math.add(a, b)`)
- Subtraction: `a - b` (not `math.subtract(a, b)`)
- Multiplication: `a * b` (not `math.multiply(a, b)`)
- Division: `a / b` (not `math.divide(a, b)`)
- Exponentiation: `a ^ b` (not `math.pow(a, b)`)

**Advanced Mathematics - Use Functions:**

**Available functions:**

| Function | Returns | Description |
|----------|---------|-------------|
| `math.sqrt(x)` | number | Square root |
| `math.abs(x)` | number or integer | Absolute value |
| `math.max(a, b)` | number | Larger of two values |
| `math.min(a, b)` | number | Smaller of two values |
| `math.floor(x)` | number | Round down to nearest integer |
| `math.ceil(x)` | number | Round up to nearest integer |
| `math.round(x)` | number | Round to nearest integer |
| `math.trunc(x)` | number | Remove decimal part |
| `math.sign(x)` | number | Returns -1, 0, or 1 |
| `math.sin(x)` | number | Sine (radians) |
| `math.cos(x)` | number | Cosine (radians) |
| `math.tan(x)` | number | Tangent (radians) |
| `math.asin(x)` | number | Arc sine |
| `math.acos(x)` | number | Arc cosine |
| `math.atan(x)` | number | Arc tangent |
| `math.atan2(y, x)` | number | Two-argument arc tangent |
| `math.ln(x)` | number | Natural logarithm (base e) |
| `math.log10(x)` | number | Base-10 logarithm |
| `math.log2(x)` | number | Base-2 logarithm |
| `math.exp(x)` | number | e raised to the power of x |
| `math.exp2(x)` | number | 2 raised to the power of x |
| `math.sinh(x)` | number | Hyperbolic sine |
| `math.cosh(x)` | number | Hyperbolic cosine |
| `math.tanh(x)` | number | Hyperbolic tangent |
| `math.pi()` | number | π ≈ 3.14159 |
| `math.e()` | number | Euler's number ≈ 2.71828 |
| `math.tau()` | number | τ = 2π ≈ 6.28318 |

**Examples:**

```clean
start:
	// Basic calculations — use operators for basic math, math.* for advanced operations
	number result = 5.0 + 3.0               // Use + operator, not math.add()
    number maximum = math.max(10.5, 7.2)    // Use math functions for advanced operations

    // Geometry - calculate circle area
    number radius = 5.0
    number area = math.pi() * (radius ^ 2.0)  // Use operators for basic arithmetic

    // Trigonometry - find triangle sides
    number angle = math.pi() / 4.0           // Use / operator, not math.divide()
    number opposite = 10.0 * math.sin(angle) // Use * operator, not math.multiply()
    number adjacent = 10.0 * math.cos(angle)

    // Rounding numbers for display
    number price = 19.99567
    number rounded = math.round(price)  // 20.0
    number floored = math.floor(price)  // 19.0

    // Logarithmic calculations
    number growth = math.exp(0.05)      // e^0.05 for 5% growth
    number halfLife = math.log2(100.0)  // How many times to halve 100 to get 1

    // Distance calculations using Pythagorean theorem
    number dx = 3.0
    number dy = 4.0
    number distance = math.sqrt((dx ^ 2.0) + (dy ^ 2.0))  // Use + operator, not math.add()

    // Absolute values for different types
    number numberAbs = math.abs(-5.7)    // 5.7
    integer intAbs = math.abs(-42)     // 42
```

### String Module

The string module provides powerful text manipulation capabilities. Whether you're processing user input, formatting output, or analyzing text data, string has all the tools you need for effective text handling.

**Method-style calls (act on a string value):**

| Call | Returns | Description |
|------|---------|-------------|
| `text.length()` | integer | Number of characters |
| `text.toUpperCase()` | string | All letters uppercase |
| `text.toLowerCase()` | string | All letters lowercase |
| `text.trim()` | string | Remove leading/trailing whitespace |
| `text.trimStart()` | string | Remove leading whitespace only |
| `text.trimEnd()` | string | Remove trailing whitespace only |
| `text.contains(search)` | boolean | Returns true if search string is found |
| `text.indexOf(search)` | integer | First position of search, or -1 |
| `text.lastIndexOf(search)` | integer | Last position of search, or -1 |
| `text.startsWith(prefix)` | boolean | True if text begins with prefix |
| `text.endsWith(suffix)` | boolean | True if text ends with suffix |
| `text.replace(old, new)` | string | Replace all occurrences of old with new |
| `text.split(delimiter)` | list\<string\> | Split into a list on the delimiter; `split("")` gives one element per character (byte), and `"".split("")` gives an empty list |
| `text.charAt(index)` | string | Character at position (0-based) |
| `text.charCodeAt(index)` | integer | Numeric code of character at position |
| `text.isEmpty()` | boolean | True if length is zero |
| `text.isBlank()` | boolean | True if empty or only whitespace |
| `text.padStart(length, pad)` | string | Pad beginning to reach length |
| `text.padEnd(length, pad)` | string | Pad end to reach length |
| `text.substring(start, end)` | string | Extract substring from start to end |

**Namespace-style calls (utility functions on the `string` module):**

| Call | Returns | Description |
|------|---------|-------------|
| `string.concat(a, b)` | string | Concatenate two strings |
| `string.join(parts, sep)` | string | Join a list of strings with a separator |

**Examples:**

```clean
start:
	// Basic text processing
    string userInput = "  Hello World!  "
    string cleaned = userInput.trim()              // "Hello World!"
    integer length = cleaned.length()              // 12

    // Case normalization for comparisons
    string email1 = "USER@EXAMPLE.COM"
    string email2 = "user@example.com"
    boolean same = email1.toLowerCase() == email2.toLowerCase()  // true

    // Text searching and validation
    string filename = "document.pdf"
    boolean isPdf = filename.endsWith(".pdf")      // true
    integer dotPos = filename.lastIndexOf(".")     // 8

    // URL processing
    string url = "https://api.example.com/users"
    boolean isHttps = url.startsWith("https://")  // true
    boolean hasApi = url.contains("api")          // true

    // Text parsing and reconstruction
    string csvLine = "John,Doe,25,Engineer"
    list<string> fields = csvLine.split(",")       // ["John", "Doe", "25", "Engineer"]
    string fullName = string.join([fields[0], fields[1]], " ")  // "John Doe"

    // Text replacement and cleaning
    string messyText = "Hello    World"
    string cleanedText = messyText.replace("    ", " ")    // "Hello World"

    // Formatting and padding
    string number = "42"
    string padded = number.padStart(5, "0")        // "00042"

    // Character-level operations
    string word = "Hello"
    string firstChar = word.charAt(0)              // "H"
    integer charCode = word.charCodeAt(0)          // 72 (ASCII for 'H')

    // Input validation
    string userField = "   "
    boolean isValid = !userField.isBlank()         // false
```

### List Module

The list module provides powerful data collection capabilities. Whether you're managing lists of items, processing data sets, or organizing information, list has all the tools you need for effective data manipulation.

**Method-style calls (act on a list value):**

| Call | Returns | Description |
|------|---------|-------------|
| `items.length()` | integer | Number of elements |
| `items.get(index)` | element type | Element at index (0-based) |
| `items.set(index, value)` | void | Set element at index |
| `items.add(item)` | void | Add item to the end |
| `items.remove(index)` | element type | Remove and return element at index |
| `items.removeLast()` | element type | Remove and return the last element |
| `items.insert(index, item)` | void | Insert item at position |
| `items.contains(item)` | boolean | True if item is in the list |
| `items.indexOf(item)` | integer | First position of item, or -1 |
| `items.lastIndexOf(item)` | integer | Last position of item, or -1 |
| `items.slice(start, end)` | list | New list with elements from start to end |
| `items.reverse()` | list | New list in reverse order |
| `items.sort()` | list | New list sorted ascending |
| `items.isEmpty()` | boolean | True if length is zero |
| `items.isNotEmpty()` | boolean | True if length is non-zero |
| `items.first()` | element type | First element |
| `items.last()` | element type | Last element |
| `items.map(fn)` | list | New list with fn applied to each element |
| `items.filter(fn)` | list | New list with only elements where fn is true |
| `items.reduce(fn, init)` | any | Combine all elements into one value |
| `items.forEach(fn)` | void | Execute fn for each element |

**Namespace-style calls (utility functions on the `list` module):**

The `list.` namespace has only these four functions; every other list operation is a method on the list value (`items.add(x)`, `items.length()`, ...).

| Call | Returns | Description |
|------|---------|-------------|
| `list.concat(a, b)` | list | Combine two lists into one |
| `list.range(start, end)` | list\<integer\> | List of integers from start to end |
| `list.fill(size, value)` | list | New list of size filled with value |
| `list.join(items, sep)` | string | Join list elements into a string with separator |

**Examples:**

```clean
start:
	// Basic list operations
    list<integer> numbers = [1, 2, 3]
    integer count = numbers.length()              // 3
    integer first = numbers.get(0)                // 1
    numbers.set(1, 99)                            // [1, 99, 3]

    // Building and modifying lists
    list<string> fruits = ["apple", "banana"]
    fruits.add("orange")                          // ["apple", "banana", "orange"]
    string removed = fruits.remove(2)             // "orange", fruits becomes ["apple", "banana"]

    // Searching through data
    list<integer> scores = [85, 92, 78, 96, 88]
    boolean hasHighScore = scores.contains(96)    // true
    integer position = scores.indexOf(92)         // 1

    // Data processing and transformation
    list<integer> data = [1, 2, 3, 4, 5]
    list<integer> doubled = data.map(x => x * 2)          // [2, 4, 6, 8, 10]
    list<integer> evens = data.filter(x => x % 2 == 0)    // [2, 4]
    integer sum = data.reduce((total, x) => total + x, 0) // 15

    // List manipulation
    list<string> names1 = ["Alice", "Bob"]
    list<string> names2 = ["Charlie", "Diana"]
    list<string> allNames = list.concat(names1, names2)    // ["Alice", "Bob", "Charlie", "Diana"]
    list<string> reversed = allNames.reverse()             // ["Diana", "Charlie", "Bob", "Alice"]

    // Working with sections of lists
    list<integer> bigList = [10, 20, 30, 40, 50]
    list<integer> middle = bigList.slice(1, 4)             // [20, 30, 40]

    // Text processing with lists
    list<string> words = ["hello", "world", "from", "Clean"]
    string sentence = list.join(words, " ")                // "hello world from Clean"

    // Creating lists programmatically
    list<string> greetings = list.fill(3, "Hello")         // ["Hello", "Hello", "Hello"]
    list<integer> countdown = list.range(5, 1)             // [5, 4, 3, 2, 1]

    // Validation and utility
    boolean empty = [].isEmpty()                           // true
    string firstWord = words.first()                       // "hello"
    string lastWord = words.last()                         // "Clean"
```

### File Module

The file module makes working with files simple and straightforward. Whether you need to read configuration files, save user data, or process text documents, file has you covered with easy-to-use methods.

| Call | Returns | Description |
|------|---------|-------------|
| `file.read(path)` | string | Read entire file as a string |
| `file.lines(path)` | list\<string\> | Read file and return each line as a list element |
| `file.write(path, content)` | void | Write content to file (creates if missing, replaces if existing) |
| `file.append(path, content)` | void | Append content to end of file (creates if missing) |
| `file.exists(path)` | boolean | True if a file exists at path |
| `file.delete(path)` | void | Delete a file (does nothing if not found) |

**Examples:**

```clean
start:
	// Read a configuration file
    string config = file.read("settings.txt")

    // Process a log file line by line (read content, then split by newline)
    string logContent = file.read("app.log")
    list<string> logLines = logContent.split("\n")

    // Save user data
    file.write("user_data.txt", "John Doe, 25, Engineer")

    // Add to a log file
    file.append("activity.log", "User logged in at 2:30 PM")

    // Check if a file exists before reading
    if file.exists("backup.txt")
        string backup = file.read("backup.txt")

    // Clean up temporary files
    file.delete("temp_data.txt")
```

### Http Module

The http module makes web requests simple and intuitive. Whether you're fetching data from APIs, submitting forms, or building web applications, http provides all the essential HTTP methods you need.

| Call | Returns | Description |
|------|---------|-------------|
| `http.get(url)` | string | Send a GET request; return response body |
| `http.post(url, body)` | string | Send a POST request with body; return response body |
| `http.put(url, body)` | string | Send a PUT request with body; return response body |
| `http.patch(url, body)` | string | Send a PATCH request with body; return response body |
| `http.delete(url)` | string | Send a DELETE request; return response body |

**Examples:**

```clean
start:
	// Fetch user data from an API
    string users = http.get("https://api.example.com/users")

    // Create a new user
    string newUser = "{\"name\": \"Alice\", \"email\": \"alice@example.com\"}"
    string response = http.post("https://api.example.com/users", newUser)

    // Update user information
    string updatedUser = "{\"name\": \"Alice Smith\", \"email\": \"alice.smith@example.com\"}"
    http.put("https://api.example.com/users/123", updatedUser)

    // Partially update user (just the email)
    string emailUpdate = "{\"email\": \"newemail@example.com\"}"
    http.patch("https://api.example.com/users/123", emailUpdate)

    // Remove a user
    http.delete("https://api.example.com/users/123")

    // Fetch weather data
    string weather = http.get("https://api.weather.com/current?city=London")
```

### JSON Module

The json module provides functions for parsing JSON text into Clean Language data structures and serializing data back to JSON text. This is essential for working with web APIs, configuration files, and data exchange.

```clean
// Core operations
json.textToData(text), json.dataToText(data)
json.tryTextToData(text), json.prettyDataToText(data)
```

#### Parsing JSON

| Call | Returns | Description |
|------|---------|-------------|
| `json.textToData(text)` | any | Parse a JSON string into a Clean value. Throws on invalid JSON. |
| `json.tryTextToData(text)` | any | Parse a JSON string; returns `none` on invalid JSON instead of throwing. |

JSON types map to Clean types:
- JSON object → `pairs<string, any>`
- JSON array → `list<any>`
- JSON string → `string`
- JSON number → `number`
- JSON boolean → `boolean`
- JSON null → `none`

Both functions support nested structures with unlimited depth.

#### Accessing JSON Data

The `any` type returned by `json.textToData()` supports both dot notation and bracket notation for accessing nested data. **Dot notation is preferred** for its readability; use bracket notation only when dot notation cannot be used (dynamic keys, computed indices).

```clean
// Preferred: Dot notation for field access
any data = json.textToData(jsonString)
any fieldValue = data.fieldName         // Access field using dot notation

// Bracket notation for dynamic keys or indices
any arrayData = json.textToData(arrayJson)
any element = arrayData[0]              // Array element by integer index
any dynamicKey = data[keyVariable]      // Dynamic key from variable

// Chained access for nested structures
any nested = data.user.profile.name     // Preferred: dot notation chain
any item = data.items[0].id             // Mixed: dot notation with array index
```

**Dot Notation on `any` Type:**

Clean Language automatically converts dot notation on `any` type values to the equivalent bracket access. This enables clean, intuitive code when working with JSON objects:

```clean
// These are equivalent:
any name = data.name                    // Preferred - more readable
any name = data["name"]                 // Bracket notation - verbose

// Nested access:
any city = data.user.address.city       // Preferred
any city = data["user"]["address"]["city"]  // Bracket notation
```

**Access Notation Guidelines:**
- **Dot notation** (`data.field`): Preferred for known field names, returns `any`
- **String keys** (`data["key"]`): Use for dynamic/computed keys, returns `any`
- **Integer indices** (`data[0]`): Use for array element access, returns `any`
- **Mixed access**: Combine dot and bracket notation as needed (`data.items[0].name`)
- **Missing fields**: Returns `none` when field doesn't exist or index is out of bounds

```clean
start:
    string jsonText = '{"name": "Alice", "scores": [85, 92, 78], "profile": {"city": "NYC"}}'
    any data = json.textToData(jsonText)

    // Preferred: Dot notation for object fields
    any name = data.name              // Returns "Alice"
    any city = data.profile.city      // Returns "NYC"
    any missing = data.unknown        // Returns none

    // Bracket notation for array access
    any scores = data.scores
    any first = scores[0]             // Returns 85
    any outOfBounds = scores[100]     // Returns none

    // Mixed notation for complex structures
    any firstScore = data.scores[0]   // Returns 85

    // Use default operator for fallback values
    string userName = data.name default "Guest"
    integer score = data.scores[0] default 0
```

#### Serializing to JSON

| Call | Returns | Description |
|------|---------|-------------|
| `json.dataToText(data)` | string | Compact JSON string from a Clean value |
| `json.prettyDataToText(data)` | string | Formatted, human-readable JSON string |

#### Usage Examples

```clean
start:
    // Parse JSON from an API response
    string apiResponse = http.get("https://api.example.com/user/123")
    any userData = json.textToData(apiResponse)

    // Access parsed data (userData is a pairs<string, any>)
    string name = userData["name"] default "Unknown"
    integer age = userData["age"] default 0

    // Safe parsing with tryTextToData
    string maybeJson = getUserInput()
    any parsed = json.tryTextToData(maybeJson)
    if parsed == none
        print("Invalid JSON provided")
    else
        print("Successfully parsed JSON")

    // Parse nested objects with escape sequences
    string nestedJson = "{\"user\": {\"name\": \"Alice\", \"age\": 25, \"address\": {\"city\": \"NYC\"}}}"
    any data = json.textToData(nestedJson)

    // Access nested fields using dot notation
    string userName = data.user.name        // Returns "Alice"
    integer userAge = data.user.age         // Returns 25
    string city = data.user.address.city    // Returns "NYC"

    // Parse nested arrays
    string matrixJson = "{\"matrix\": [[1, 2, 3], [4, 5, 6], [7, 8, 9]]}"
    any matrixData = json.textToData(matrixJson)
    any firstRow = matrixData.matrix[0]     // Returns [1, 2, 3]
    any element = matrixData.matrix[0][1]   // Returns 2

    // Parse arrays of objects
    string usersJson = "[{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]"
    any users = json.textToData(usersJson)
    any firstUser = users[0]                // Returns {"id": 1, "name": "Alice"}
    string firstName = users[0].name        // Returns "Alice"
    integer secondId = users[1].id          // Returns 2

    // Complex nested structure
    string complexJson = "{\"users\": [{\"id\": 1, \"tags\": [\"admin\", \"staff\"]}, {\"id\": 2, \"tags\": [\"user\"]}]}"
    any complex = json.textToData(complexJson)
    any adminTags = complex.users[0].tags   // Returns ["admin", "staff"]
    string firstTag = complex.users[0].tags[0]  // Returns "admin"

    // Create data and serialize to JSON
    pairs<string, any> user = {}
    user["name"] = "Bob"
    user["email"] = "bob@example.com"
    user["active"] = true

    string jsonString = json.dataToText(user)
    // Result: {"name":"Bob","email":"bob@example.com","active":true}

    // Pretty-print for debugging or config files
    string prettyJson = json.prettyDataToText(user)
    // Result:
    // {
    //   "name": "Bob",
    //   "email": "bob@example.com",
    //   "active": true
    // }

    // Working with JSON arrays
    list<any> items = [1, 2, 3, "four", true, none]
    string arrayJson = json.dataToText(items)
    // Result: [1,2,3,"four",true,null]

    // Nested structures
    pairs<string, any> config = {}
    config["database"] = {}
    config["database"]["host"] = "localhost"
    config["database"]["port"] = 5432
    config["features"] = ["auth", "logging", "caching"]

    file.write("config.json", json.prettyDataToText(config))
```

#### JSON Type Mapping

| JSON Type | Clean Type | Example |
|-----------|------------|---------|
| object | `pairs<string, any>` | `{"key": "value"}` → `pairs` |
| array | `list<any>` | `[1, 2, 3]` → `list<any>` |
| string | `string` | `"hello"` → `"hello"` |
| number | `number` | `3.14` → `3.14` |
| boolean | `boolean` | `true` → `true` |
| null | `none` | `null` → `none` |

**Note on Nested Structures**: The JSON parser fully supports nested objects and arrays. When parsing nested structures, inner objects and arrays are recursively parsed and stored as `any` values within the parent structure.

```clean
// Nested object example
string json = "{\"user\": {\"name\": \"Alice\", \"age\": 25}}"
any data = json.textToData(json)
// data type: pairs<string, any>
// data.user type: any (contains a pairs<string, any>)
// data.user.name type: any (contains a string)

// Nested array example
string arrayJson = "{\"matrix\": [[1, 2], [3, 4]]}"
any matrix = json.textToData(arrayJson)
// matrix type: pairs<string, any>
// matrix.matrix type: any (contains a list<any>)
// matrix.matrix[0] type: any (contains a list<any>)
// matrix.matrix[0][0] type: any (contains a number)
```

#### Nested Structure Support

The JSON parser uses a **recursive architecture** to support arbitrary nesting depth:

**Capabilities:**
- **Unlimited nesting depth**: Parse structures with any level of nesting
- **Mixed structures**: Combine objects and arrays at any depth
- **Complex data**: Handle real-world JSON from APIs and config files

**Implementation:**
The parser uses three mutually recursive helper functions:
- `__json_parse_value`: Dispatcher for all JSON value types
- `__json_parse_object`: Recursive object parser
- `__json_parse_array`: Recursive array parser

**Examples of supported structures:**

```clean
// Deep object nesting (5 levels)
string deepJson = "{\"a\": {\"b\": {\"c\": {\"d\": {\"e\": 42}}}}}"
any deep = json.textToData(deepJson)
integer value = deep.a.b.c.d.e  // Returns 42

// Deep array nesting
string arrayJson = "[[[[[1]]]]]"
any arrays = json.textToData(arrayJson)
integer val = arrays[0][0][0][0][0]  // Returns 1

// Real-world API response
string apiResponse = "{\"data\": {\"users\": [{\"id\": 1, \"profile\": {\"name\": \"Alice\", \"tags\": [\"admin\", \"user\"]}}]}}"
any response = json.textToData(apiResponse)
string name = response.data.users[0].profile.name  // Returns "Alice"
string tag = response.data.users[0].profile.tags[0]  // Returns "admin"
```

**Performance:** The recursive parser is optimized for WebAssembly execution and can efficiently handle typical JSON structures from web APIs (up to ~50 nesting levels, limited by WebAssembly stack depth).

#### Error Handling

```clean
start:
    // textToData throws on invalid JSON
    string badJson = "{ invalid json }"
    any data = json.textToData(badJson) onError none

    // Or use tryTextToData for none-based error handling
    any safeData = json.tryTextToData(badJson)
    if safeData == none
        print("JSON parsing failed")

    // Use default operator for fallback values
    any result = json.tryTextToData(maybeJson) default {}
```

---

[← Previous: Classes and Objects](./15-classes-and-objects.md) | [Index](./README.md) | [Next: Method-Style Syntax](./17-method-style-syntax.md) →
