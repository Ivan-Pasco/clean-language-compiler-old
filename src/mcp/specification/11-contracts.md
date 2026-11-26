# 11. Contracts

> **Part of the Clean Language Specification.** See [README](./README.md) for the full index.


Contracts state what must be true while a program runs. Clean has three:

| Contract | Where | Checked |
|----------|-------|---------|
| `require` | Top of a function or method body | When the function is called, before its body runs |
| `ensure` | Top of a function or method body, after any `require` | When the function returns |
| `always:` | Inside a class | When a constructor or method of the class finishes |

If a contract is false, execution stops with a contract violation error. `onError` does not catch it: a broken contract is a bug in the program, not an expected failure.

### `require` — Preconditions

Use `require` to declare preconditions that must be true for a function to execute.

**Syntax:**
```clean
require <boolean_expression>
```

**Example:**
```clean
functions:
    integer divide(integer a, integer b)
        require b != 0
        return a / b

    void setAge(integer age)
        require age >= 0
        require age <= 150
        // implementation
```

**Rules for `require`:**
- Can only appear inside functions or class methods
- Must appear before other statements in the function body
- Multiple `require` statements are allowed
- Always checked at runtime (cannot be disabled)

**Error on violation:**
```
Contract violation: require failed at divide:2
  Expression: b != 0
```

### `ensure` — Postconditions

Use `ensure` to declare what must be true when a function returns. Inside the condition, `result` is the value being returned.

**Syntax:**
```clean
ensure <boolean_expression>
```

**Example:**
```clean
start:
    print(half(10))        // 5
    print(half(3))         // stops: 1 * 2 is not 3

functions:
    integer half(integer a)
        require a > 0
        ensure result * 2 == a
        return a / 2
```

**Rules for `ensure`:**
- Can only appear inside functions or class methods
- Must come after any `require` lines and before other statements (CLASS005)
- Multiple `ensure` statements are allowed
- Checked every time the function returns, on every return path
- Always checked at runtime, like `require`: `--release` does not remove it

### `always:` — Class Invariants

Use an `always:` block inside a class to list conditions that must hold for every object of the class. Each line is one boolean expression (CLASS006).

**Example:**
```clean
start:
    Account acc = Account(5)
    print(acc.withdraw(2))     // 3
    print(acc.withdraw(10))    // stops: balance would be -7

class Account
    public:
        integer balance

    constructor(integer opening)
        balance = opening

    always:
        balance >= 0

    functions:
        public:
            integer withdraw(integer amount)
                balance = balance - amount
                return balance
```

**Rules for `always:`:**
- Goes in the class body, next to the fields, constructor and `functions:`
- Each line must be a boolean expression (CLASS006)
- Checked when the constructor finishes and when each method of the class finishes
- Removed by `--release` (`cln compile app.cln --release`): release builds do not check invariants

---

[← Previous: Functions](./10-functions.md) | [Index](./README.md) | [Next: Testing](./12-testing.md) →
