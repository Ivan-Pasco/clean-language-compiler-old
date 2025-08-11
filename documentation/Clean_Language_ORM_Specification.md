Here’s a **complete and friendly ORM syntax for Clean Language** , aligned with the Clean Language principles of simplicity and readability:

---

db.config:
    engine = "postgres"
    host = "localhost"
    port = 5432
    database = "mydb"
    user = "admin"
    password = "secret"

Default values:
| Key      | Default           |
| -------- | ----------------- |
| engine   | `"sqlite"`        |
| host     | `"localhost"`     |
| port     | depends on engine |
| user     | `""`              |
| password | `""`              |


## 🧼 Clean ORM Syntax – Using `data`

### 1. Define a Table

```clean
data User
    string name
    integer age
    string email
    boolean active = true

    functions:
        string toString()
            return name + " (" + email + ")"
```

### 2. Basic Operations

#### Create Table

```clean
User.createTable()
```

#### Insert New Record

```clean
User.insert:
    name = "Alice"
    age = 30
    email = "alice@example.com"
```

#### Query All

```clean
list<User> users = User.all()
```

#### Filter Records

```clean
list<User> adults = User.where(age > 18)
list<User> actives = User.where(active == true)
```

#### Find One

```clean
User alice = User.get(name == "Alice")
```

#### Update Record

```clean
alice.age = 31
alice.save()
```

#### Delete Record

```clean
alice.delete()
```

---

## 🔍 Filtering Syntax

Clean-style `where` uses **simple expressions**:

```clean
User.where(age > 25 and active == true)
```

You can also use **method chaining**:

```clean
list<User> results = User.where(active == true).orderBy(age).limit(10)
```

---

## 🛠 Utility Methods

```clean
User.count()
User.exists(email == "bob@example.com")
User.deleteWhere(age < 18)
```

---

## 🌱 Auto-Migrations

Clean ORM can auto-migrate with:

```clean
User.migrate()
```

---

## 📦 Relationships

### Foreign Key (One-to-Many)

```clean
data Post
    string title
    string content
    User author   // Automatically creates foreign key

    functions:
        string summary()
            return title + " by " + author.name
```

### Access Related Data

```clean
Post post = Post.get(id == 1)
User author = post.author

list<Post> postsByAlice = Post.where(author.name == "Alice")
```

---

## 🧪 Example

```clean
start()
    User.createTable()
    Post.createTable()

    User.insert:
        name = "Alice"
        age = 30
        email = "alice@example.com"

    User.insert:
        name = "Bob"
        age = 17
        email = "bob@example.com"

    list<User> adults = User.where(age >= 18)

    iterate person in adults
        println person.toString()

    Post.insert:
        title = "Welcome to Clean"
        content = "This is a sample post"
        author = User.get(name == "Alice")
```


Use Sql query and map the result to the data type.

Use db.queryAs(Type, sql, params)
This tells Clean to:
Execute raw SQL
Parse the result into instances of the specified data type

data User
	string name
	integer age
	string email

list<User> users = db.queryAs(User,
	"SELECT name, age, email FROM users WHERE age > ?",
	[18]
)

User u = db.queryOneAs(User,
	"SELECT * FROM users WHERE email = ?",
	["alice@example.com"]
)

| Function                           | Returns          | Description                     |
| ---------------------------------- | ---------------- | ------------------------------- |
| `db.query(sql, params)`            | `list<map>`      | Raw SQL → list of dictionaries  |
| `db.queryOne(sql, params)`         | `map` or `null`  | Raw SQL → one row as map        |
| `db.queryAs(Type, sql, params)`    | `list<Type>`     | Raw SQL → list of typed objects |
| `db.queryOneAs(Type, sql, params)` | `Type` or `null` | Raw SQL → one typed object      |
