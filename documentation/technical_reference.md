# Pitruck Language Specification & Technical Reference

## Executive Summary & Introduction
**Pitruck** is a lightweight, dynamically typed, interpreted programming language engineered in Rust. It combines imperative and functional programming paradigms with Python-like syntax, dynamic dictionary/list collections, prototype-like object instantiation via classes, and a first-class HTTP web application server framework.

### Design Philosophy
- **Simplicity and Expressiveness**: Clean, minimal syntax inspired by Python, JavaScript, and Rust.
- **Self-Contained Web Stack**: Built-in HTTP client and web server engine (`--serve`) capable of serving single-file APIs or full directory-based file systems without external dependencies.
- **Fast Symbol Resolution**: Uses custom Fowler-Noll-Vo (FNV-1a) 64-bit hashing (`hash_name`) for string identifier lookup across variable scopes and AST nodes.

### Intended Use Cases
- Microservices, lightweight web servers, and server-side rendered HTML applications.
- Embedded scripting and rapid automation tasks.
- Educational programming language exploration.

---

## Getting Started

### CLI Invocation & Commands
The Pitruck executable provides a REPL, script runner, library manager, and HTTP server framework.

```bash
# Interactive REPL
pitruck

# Execute a Pitruck script
pitruck script.pr

# Execute with detailed lexer, parser, and interpreter performance timings
pitruck script.pr --speed

# Start an HTTP server on a single handler script
pitruck --serve server.pr --port 8000

# Start an HTTP server with directory-based routing
pitruck --serve ./routes_dir/ --port 8080 --debug

# Managing libraries
pitruck lib install https://example.com/math_extra.pr
pitruck lib install ./local_lib.pr
pitruck lib list
pitruck lib delete math_extra
```

### Hello World Example
```pitruck
# hello.pr
print "Hello, World from Pitruck!"
```

---

## Lexical Structure

### Character Set
Pitruck source files are parsed as UTF-8 character sequences (`Vec<char>`).

### Comments
Single-line comments begin with `#` or `//` and extend to the end of the line.

```pitruck
# This is a single-line comment
// This is also a single-line comment
```

### Identifiers
Identifiers start with an ASCII letter (`a-z`, `A-Z`) or an underscore `_`, followed by alphanumeric characters or underscores.

### Whitespace Rules
Whitespace (spaces, tabs `\t`, carriage returns `\r`, and newlines `\n`) separates tokens but is otherwise insignificant. Pitruck uses explicit block markers (`{` and `}`) rather than whitespace indentation.

### Escape Sequences
Within double-quoted string literals (`"..."`), the lexer recognizes:
- `\n` - Newline (0x0A)
- `\t` - Horizontal Tab (0x09)
- `\r` - Carriage Return (0x0D)
- `\e` - Escape character (0x1B)
- `\"` - Double quote
- `\\` - Backslash
- `\xHH` - 2-digit Hexadecimal byte sequence

---

## Primitive & Complex Types

| Type Name | Internal Representation | Description | Truthiness |
| :--- | :--- | :--- | :--- |
| **Number** | `f64` | 64-bit floating-point number. | Truthy unless `0.0` or `NaN` (any non-null/false value is truthy). |
| **String** | `String` | UTF-8 character string. | Always truthy. |
| **Bool** | `bool` | `true` or `false`. | `true` is truthy, `false` is falsy. |
| **Null** | Unit (`Null`) | Represents empty or non-existent value. | Falsy. |
| **List** | `Rc<RefCell<Vec<Value>>>` | Mutable dynamically-sized array. | Always truthy. |
| **Dict** | `Rc<RefCell<AHashMap<String, Value>>>` | Mutable key-value hash table (Keys must be strings). | Always truthy. |
| **Function** | `Value::Function` | User-defined or closure function. | Always truthy. |
| **Class** | `Value::Class` | Class blueprint containing methods. | Always truthy. |
| **Instance** | `Value::Instance` | Class object instance holding fields and methods. | Always truthy. |

---

## Variables & Scope

### Variable Declaration & Mutability
Variables are declared using the `var` keyword and are mutable by default.
```pitruck
var x = 10
var name = "Pitruck"
x = x + 5
```

### Scope & Lifetime
- **Lexical Scoping**: Pitruck uses stack-based lexical scoping managed by a scope index stack (`scope_tops`).
- **Redeclaration Check**: Redeclaring a variable within the same scope using `var` raises a runtime error: `'<name>' is already declared in this scope`.
- **Global / Captured Scope**: Functions and closures maintain references to captured variable environments.

---

## Operators & Precedence

| Precedence (High to Low) | Operator | Description | Associativity |
| :--- | :--- | :--- | :--- |
| 1 (Postfix) | `()`, `[]`, `.` | Function call, Indexing, Property access | Left-to-Right |
| 2 (Unary) | `-`, `not` | Unary negation, Logical NOT | Right-to-Left |
| 3 (Multiplicative) | `*`, `/`, `%` | Multiplication, Division, Modulo | Left-to-Right |
| 4 (Additive) | `+`, `-` | Addition / Concatenation, Subtraction | Left-to-Right |
| 5 (Relational) | `<`, `>`, `<=`, `>=` | Comparison operators | Left-to-Right |
| 6 (Equality) | `==`, `!=` | Value equality and inequality | Left-to-Right |
| 7 (Logical AND) | `and` | Short-circuiting logical AND | Left-to-Right |
| 8 (Logical OR) | `or` | Short-circuiting logical OR | Left-to-Right |
| 9 (Assignment) | `=`, `+=`, `-=`, `*=`, `/=` | Assignment & Compound assignment | Right-to-Left |

---

## Control Flow

### Conditional (`if`, `elif`, `else`)
```pitruck
var score = 85

if score >= 90 {
    print "Grade: A"
} elif score >= 80 {
    print "Grade: B"
} else {
    print "Grade: C"
}
```

### Match Statement (`match`)
Matches an expression against literal values or a fallback pattern (`_`).
```pitruck
var status = 404

match status {
    200 => { print "OK" }
    404 => { print "Not Found" }
    500 => { print "Internal Server Error" }
    _   => { print "Unknown Status" }
}
```

### Loops
- **While Loop**: Iterates while condition is truthy.
```pitruck
var i = 0
while i < 5 {
    print i
    i += 1
}
```

- **For-In Loop**: Iterates over elements in a List or characters in a String.
```pitruck
for item in ["apple", "banana", "cherry"] {
    print item
}
```

---

## Functions & Closures

### Function Declaration
Functions are defined using the `func` keyword.
```pitruck
func add(a, b) {
    return a + b
}
```

### Anonymous Functions & Lambdas
Lambdas use fat-arrow syntax `(params) => { body }` or `(params) => expression`. Lambdas capture their lexical environment at creation time.
```pitruck
var double_fn = (x) => { return x * 2 }
var square_fn = (x) => x * x
```

---

## Object-Oriented Programming (Classes)

Pitruck supports class definitions with dynamic field assignment and method binding.

```pitruck
class BankAccount {
    func init(owner, balance) {
        self.owner = owner
        self.balance = balance
    }

    func deposit(amount) {
        self.balance += amount
        return self.balance
    }

    func get_summary() {
        return self.owner + ": $" + to_string(self.balance)
    }
}

var acc = BankAccount("Alice", 100)
acc.deposit(50)
print acc.get_summary() # "Alice: $150"
```

---

## Modules & Imports (`bring`)

Modules are loaded using the `bring` statement. Pitruck searches for `<module>.pr` or `<module>` across multiple candidate locations:
1. Absolute path (if provided).
2. Directory containing the executing script.
3. `./lib/` relative to the executing script.
4. Working directory `./`.
5. Binary executable directory (`<exe_dir>/lib/`).

```pitruck
bring math
bring trucky
```

---

## HTTP Web Server Framework (`--serve`)

Pitruck provides an integrated multi-threaded web server framework when executed with `pitruck --serve <file_or_dir>`.

### Request Context Injection
Before executing a script for an incoming HTTP request, Pitruck injects two global instances: `request` and `response`.

#### `request` Instance Attributes
- `request.method`: String (`"GET"`, `"POST"`, `"PUT"`, `"DELETE"`, etc.)
- `request.path`: Route string (e.g., `"/api/users"`)
- `request.query_str`: Raw query string (e.g., `"id=10&tab=info"`)
- `request.query`: Dict containing key-value string pairs from query parameters.
- `request.form`: Dict containing key-value string pairs from `application/x-www-form-urlencoded` bodies.
- `request.body`: Raw request payload string.
- `request.headers`: Dict containing incoming HTTP headers.

#### `response` Instance Attributes
- `response.status`: Number representing HTTP status code (default: `200`).
- `response.body`: String body returned to the client.
- `response.headers`: Dict of headers sent to the client.

### Web Server Example
```pitruck
# server.pr
if request.path == "/api/status" {
    response.status = 200
    response.headers["Content-Type"] = "application/json"
    response.body = json_encode({"status": "online", "uptime": timestamp()})
} else {
    response.status = 404
    response.body = "<h1>404 Page Not Found</h1>"
}
```

---

## Built-In Functions & Constants Reference

### Constants
- `PI`: `3.141592653589793`
- `E`: `2.718281828459045`

### Built-In Standard Library
- **`rand(min, max)`**: Returns pseudo-random integer between `min` and `max`.
- **`range(start, stop, [step])`**: Generates a List of numbers.
- **`input([prompt])`**: Reads a line from standard input.
- **`to_number(val)`**: Converts String/Bool to Number.
- **`to_string(val)`**: Converts value to String.
- **`is_number(val)`**: Checks if value is or can be parsed as Number.
- **`html_escape(val)`**: Escapes HTML special characters (`&`, `<`, `>`, `"`, `'`).
- **`clear()`**: Clears terminal screen ANSI sequence.
- **`len(val)`**: Returns length of String, List, or Dict.
- **`push(list, item)`**: Appends item to List.
- **`pop(list)`**: Pops and returns last item from List.
- **`contains(container, item)`**: Checks membership in String, List, or Dict.
- **`keys(dict)`**: Returns List of keys.
- **`values(dict)`**: Returns List of values.
- **`remove(container, key_or_index)`**: Removes element from Dict or List.
- **`split(str, sep)`**: Splits String into List of Strings.
- **`join(list, sep)`**: Joins List of values with separator.
- **`trim(str)`**, **`upper(str)`**, **`lower(str)`**: String manipulation.
- **`replace(str, from, to)`**: Replaces substrings.
- **`starts_with(str, prefix)`**, **`ends_with(str, suffix)`**: Prefix/Suffix checks.
- **`substr(str, start, [len])`**: Extracts substring.
- **`char_at(str, index)`**: Gets character at index.
- **`pad_left(str, width, fill)`**, **`pad_right(str, width, fill)`**: String padding.
- **`repeat_str(str, count)`**: Repeats string.
- **`index_of(str|list, target)`**: Finds index of substring or element (-1 if missing).
- **`list_slice(list|str, start, end)`**: Extracts slice.
- **`list_reverse(list|str)`**: Reverses list or string.
- **`list_sort(list)`**: Sorts list in-place.
- **`list_sort_by(list, comparator)`**: Sorts using custom comparator.
- **`list_map(list, func)`**: Maps function over list.
- **`list_filter(list, func)`**: Filters list with predicate function.
- **`list_reduce(list, func, initial)`**: Reduces list.
- **`json_encode(val)`**: Serializes value to JSON string.
- **`json_decode(str)`**: Parses JSON string to Pitruck value.
- **`url_encode(str)`**, **`url_decode(str)`**: URL percent encoding/decoding.
- **`http_request(method, url, body, headers)`**: Outbound HTTP GET/POST client.
- **`time()`**: Returns `"HH:MM:SS"`.
- **`timestamp()`**: Returns current Unix epoch seconds.
- **`sys_os()`**: Returns underlying OS string.
- **`sys_exit(code)`**: Exits interpreter process.
- **`sys_sleep(ms)`**: Sleeps current thread.
- **`sys_env(key)`**: Retrieves environment variable.
- **`sys_writefile(path, content)`**, **`sys_readfile(path)`**, **`sys_fileexists(path)`**: File system I/O operations (restricted in sandboxed server mode).
- **`math_abs(n)`**, **`math_sqrt(n)`**, **`math_pow(b, e)`**, **`floor(n)`**, **`ceil(n)`**, **`round(n)`**: Math utilities.

---

## EBNF Grammar Specification

```ebnf
Program         ::= Statement* EOF ;

Statement       ::= VarDecl
                  | BringStmt
                  | FuncDef
                  | ClassDef
                  | MatchStmt
                  | ReturnStmt
                  | PrintStmt
                  | IfStmt
                  | WhileStmt
                  | ForStmt
                  | AssignOrExprStmt ;

VarDecl         ::= "var" IDENT "=" Expression ;
BringStmt       ::= "bring" IDENT ;
FuncDef         ::= "func" IDENT "(" ParameterList? ")" Block ;
ClassDef        ::= "class" IDENT "{" Statement* "}" ;
MatchStmt       ::= "match" Expression "{" MatchArm* "}" ;
MatchArm        ::= (Expression | "_") "=>" (Block | Statement) ;
ReturnStmt      ::= "return" Expression? ;
PrintStmt       ::= "print" Expression ;
IfStmt          ::= "if" Expression Block ("elif" Expression Block)* ("else" Block)? ;
WhileStmt       ::= "while" Expression Block ;
ForStmt         ::= "for" IDENT "in" Expression Block ;
AssignOrExprStmt::= Expression (("=" | "+=" | "-=" | "*=" | "/=") Expression)? ;

Block           ::= "{" Statement* "}" | Statement ;
ParameterList   ::= IDENT ("," IDENT)* ;

Expression      ::= LambdaExpr | LogicalOr ;
LambdaExpr      ::= "(" ParameterList? ")" "=>" (Block | Expression) ;
LogicalOr       ::= LogicalAnd ("or" LogicalAnd)* ;
LogicalAnd      ::= LogicalNot ("and" LogicalNot)* ;
LogicalNot      ::= "not"* Comparison ;
Comparison      ::= Additive (("==" | "!=" | "<" | ">" | "<=" | ">=") Additive)* ;
Additive        ::= Multiplicative (("+" | "-") Multiplicative)* ;
Multiplicative  ::= Unary (("*" | "/" | "%") Unary)* ;
Unary           ::= ("-")? Postfix ;
Postfix         ::= Primary ( "(" ArgList? ")" | "[" Expression "]" | "." IDENT )* ;

Primary         ::= NUMBER | STRING | "true" | "false" | "null" | "self"
                  | IDENT | "(" Expression ")" | ListLiteral | DictLiteral ;

ListLiteral     ::= "[" (Expression ("," Expression)*)? "]" ;
DictLiteral     ::= "{" (Expression ":" Expression ("," Expression)*)? "}" ;
ArgList         ::= Expression ("," Expression)* ;
```