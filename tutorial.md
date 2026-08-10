# The Que Language Tutorial

Welcome to Que — a modern scripting language designed for build automation and
DevOps, combining C-style block syntax with functional programming and
first-class support for paths, commands, durations, and more.

This tutorial walks you through the language from first principles up to a
complete, realistic deployment script. Every example is valid Que code you can
save to a `.que` file and run with:

```sh
que hello.que
```

Or experiment interactively in the REPL:

```sh
que
```

The tutorial is organized in five parts. Part I covers the core language —
everything you'd expect from a modern scripting language. Part II covers the
types that make Que specifically good at DevOps and build automation: paths,
commands, durations, secrets. Part III tours the standard library. Part IV
covers the task and build system. Part V ties it together with a realistic
capstone and reference material.

---

## Table of Contents

**Part I — Language Fundamentals**

1. [Hello, Que!](#1-hello-que)
2. [Variables, Types, and Operators](#2-variables-types-and-operators)
3. [Control Flow](#3-control-flow)
4. [Strings](#4-strings)
5. [Collections](#5-collections)
6. [Functions](#6-functions)
7. [Closures and the Pipe Operator](#7-closures-and-the-pipe-operator)
8. [Destructuring and Pattern Matching](#8-destructuring-and-pattern-matching)
9. [Structs, Impl Blocks, and Traits](#9-structs-impl-blocks-and-traits)
10. [Enums](#10-enums)
11. [Error Handling](#11-error-handling)
12. [Modules and Imports](#12-modules-and-imports)

**Part II — DevOps-Native Types**

13. [Paths](#13-paths)
14. [Globs](#14-globs)
15. [Durations and Semantic Versioning](#15-durations-and-semantic-versioning)
16. [Regex and Secrets](#16-regex-and-secrets)
17. [Running Commands](#17-running-commands)
18. [Background Processes and Parallel Execution](#18-background-processes-and-parallel-execution)
19. [Resilience — Retry and Timeout](#19-resilience--retry-and-timeout)
20. [File I/O — Handles and Streams](#20-file-io--handles-and-streams)
21. [Environment and Platform](#21-environment-and-platform)
22. [Building Command-Line Apps](#22-building-command-line-apps)

**Part III — Standard Library**

23. [Config Formats — JSON, YAML, TOML](#23-config-formats--json-yaml-toml)
24. [HTTP Client](#24-http-client)
25. [Filesystem, Archives, Hashing, and CSV](#25-filesystem-archives-hashing-and-csv)
26. [Logging, Git, Dotenv, Templates, and Networking](#26-logging-git-dotenv-templates-and-networking)
27. [Date and Time](#27-date-and-time)
28. [Containers and Remote Execution](#28-containers-and-remote-execution)
29. [Watching Files](#29-watching-files)

**Part IV — The Build System**

30. [Tasks and the Build Graph](#30-tasks-and-the-build-graph)
31. [Testing, Formatting, and Linting](#31-testing-formatting-and-linting)
32. [Strict Mode, Dry Runs, and Sandboxing](#32-strict-mode-dry-runs-and-sandboxing)

**Part V — Wrapping Up**

33. [Inspection, Reflection, and the REPL](#33-inspection-reflection-and-the-repl)
34. [A Real-World Deployment Pipeline](#34-a-real-world-deployment-pipeline)
35. [Language Grammar Summary](#35-language-grammar-summary)
36. [Quick Reference](#36-quick-reference)

---

# Part I — Language Fundamentals

## 1. Hello, Que!

```que
println("Hello, Que!")
```

`println` prints a value followed by a newline. `print` does the same without
the newline. Both accept any number of arguments separated by spaces.

```que
let name = "World"
println("Hello,", name)
```

Comments begin with `//` and extend to the end of the line:

```que
// This is a comment
println("hi")   // inline comment
```

### Running Que

| Command | Description |
|---------|-------------|
| `que` | Start the interactive REPL |
| `que script.que` | Run a script file |
| `que run <task>` | Run a task from a Quefile |
| `que run <task> --help` | Show task usage and argument table |
| `que run <task> -- arg1 arg2` | Run task with positional arguments |
| `que run <task> -- key=val` | Run task with named arguments |
| `que run -g <task>` | Run a task from the global Quefile |
| `que tasks` | List available tasks |
| `que fmt` | Format source files |
| `que lint` | Lint source files |
| `que test` | Run the test suite |
| `que install` | Fetch dependencies declared in `que.toml` |
| `que install -g` | Fetch the global Quefile's dependencies |

---

## 2. Variables, Types, and Operators

### Variables and Mutability

Que has **immutable-by-default** variables. Use `let` for values that never
change and `mut` for values that do.

```que
let greeting = "Hello"      // immutable — cannot be reassigned
mut counter = 0             // mutable — can be reassigned

counter += 1
counter += 1
println(counter)            // 2
```

Trying to reassign an immutable variable is an error:

```que
let x = 1
x = 2   // runtime error: cannot assign to immutable variable
```

### Basic Types

| Type | Example | Description |
|------|---------|-------------|
| `Int` | `42`, `0xFF`, `0o755` | Integer (hex, octal supported) |
| `Float` | `3.14` | Floating-point number |
| `Bool` | `true`, `false` | Boolean |
| `String` | `"hello"` | Text (see [Section 4](#4-strings)) |
| `Null` | `null` | Absence of a value |

```que
let x = 42          // Int
let pi = 3.14       // Float
let hex = 0xFF      // 255
let octal = 0o755   // 493 (file permissions!)
let yes = true      // Bool
let nothing = null  // Null
```

### Operators

**Arithmetic**: `+`, `-`, `*`, `/`, `%` (modulo), `**` (power)

```que
println(2 ** 10)       // 1024
println(17 % 5)        // 2
println((2 + 3) * 4)   // 20
```

**Comparison**: `==`, `!=`, `<`, `>`, `<=`, `>=`

**Logical**: `&&` (and), `||` (or), `!` (not)

**Bitwise**: `&`, `|`, `^`, `~`, `<<`, `>>`

```que
println(!false && true)   // true
println(0xFF & 0x0F)      // 15
```

### Type Conversions

Use the built-in conversion functions:

```que
let n = int("42")      // 42
let f = float(42)      // 42.0
let s = str(3.14)      // "3.14"
let b = bool(1)        // true
```

Numeric and boolean types also have conversion methods:

```que
// Int methods
let x = 42
println(x.to_float())     // 42.0
println(x.to_string())    // "42"
println((-5).abs())       // 5

// Float methods
let f = 3.7
println(f.to_int())       // 3  (truncates)
println(f.to_string())    // "3.7"
println((-2.5).abs())     // 2.5
println(f.floor())        // 3.0
println(f.ceil())         // 4.0
println(f.round())        // 4.0

// Bool methods
println(true.to_int())    // 1
println(false.to_int())   // 0
println(true.to_string()) // "true"
```

---

## 3. Control Flow

### If / Else

`if` is an **expression** — it returns a value:

```que
fn classify(n) {
    if n > 100 { "large" }
    else if n > 10 { "medium" }
    else { "small" }
}

println(classify(5))     // small
println(classify(50))    // medium
println(classify(500))   // large
```

### For Loops

Iterate over lists, maps, or ranges:

```que
for word in ["hello", "beautiful", "world"] {
    println(word.to_upper())
}
```

### Ranges

Use `..` for exclusive and `..=` for inclusive ranges:

```que
mut sum = 0
for i in 1..=10 {
    sum += i
}
println(sum)   // 55
```

### While Loops

```que
mut n = 5
while n > 0 {
    println(n)
    n -= 1
}
```

### Loop with Break

`loop` creates an infinite loop. Use `break` to exit. `break` with a value
makes the loop an expression:

```que
mut i = 0
let result = loop {
    i += 1
    if i * i > 50 {
        break i
    }
}
println(result)   // 8
```

### Blocks Are Expressions

Any `{ ... }` block evaluates to its last expression:

```que
let x = {
    let a = 10
    let b = 20
    a + b
}
println(x)   // 30
```

### Scope Isolation

Blocks create new scopes. Inner variables don't leak out:

```que
let x = 1
{
    let x = 2
    println(x)   // 2
}
println(x)       // 1
```

---

## 4. Strings

### Interpolation

Use `${}` inside double-quoted strings to embed any expression:

```que
let name = "Que"
let version = 1
println("Welcome to ${name} v${version}!")
// Welcome to Que v1!
```

### Escape Sequences

Standard escape sequences work in double-quoted strings: `\n`, `\t`, `\\`, `\"`.

### Raw Strings

Raw strings disable all escape processing and interpolation. Prefix with `r`:

```que
let path = r"C:\Users\alice\Documents"   // backslashes are literal
let regex_src = r"\d{3}-\d{4}"           // no need to double-escape
let tmpl = r"${not_interpolated}"        // $ has no special meaning
```

When a raw string contains a double quote, use `#` delimiters (like Rust):

```que
let html = r#"<div class="main">hello</div>"#
let nested = r##"contains "# inside"##
```

### Triple-Quoted Strings

Triple-quoted strings preserve multiline text and strip leading indentation:

```que
let sql = """
    SELECT *
    FROM users
    WHERE active = true
"""
```

### String Methods

Strings have a rich set of methods you can chain:

```que
"  Hello, World!  "
    .trim()
    .to_lower()
    .replace("world", "que")
// "hello, que!"
```

```que
let csv = "alice,bob,charlie"
let names = csv.split(",")
let upper_names = names.map(|n| n.to_upper())
println(upper_names.join(" | "))
// ALICE | BOB | CHARLIE
```

| Method | Description |
|--------|-------------|
| `.len()` | Number of characters |
| `.trim()` | Remove leading/trailing whitespace |
| `.trim_start()` | Remove leading whitespace |
| `.trim_end()` | Remove trailing whitespace |
| `.to_upper()` | Convert to uppercase |
| `.to_lower()` | Convert to lowercase |
| `.starts_with(s)` | Check prefix |
| `.ends_with(s)` | Check suffix |
| `.contains(s)` | Check substring |
| `.contains_all(needles)` | Check every needle is a substring (`true` if empty) |
| `.contains_any(needles)` | Check at least one needle is a substring (`false` if empty) |
| `.replace(from, to)` | Replace occurrences |
| `.split(sep)` | Split into a list of strings |
| `.chars()` | Split into individual characters |
| `.lines()` | Split by newlines |
| `.repeat(n)` | Repeat the string n times |
| `.is_empty()` | Check if length is zero |
| `.parse_int()` | Parse as integer (returns `Result`) |
| `.parse_float()` | Parse as float (returns `Result`) |
| `.reverse()` | Reverse the string |
| `.index_of(s)` | Index of substring or `-1` |
| `.substring(start, end)` | Extract substring by index |
| `.pad_start(len, char)` | Pad from the left |
| `.pad_end(len, char)` | Pad from the right |
| `.matches(regex)` | Check if string matches a regex |
| `.bytes()` | List of byte values |
| `.to_path()` | Convert to a `Path` value |

---

## 5. Collections

### Lists

Lists are ordered, heterogeneous collections created with `[]`:

```que
let numbers = [1, 2, 3, 4, 5]
let mixed = [1, "two", true, null]
```

**Indexing:**

```que
let items = [10, 20, 30]
println(items[0])     // 10
println(items[2])     // 30
```

**Spread operator** — combine lists with `...`:

```que
let a = [1, 2, 3]
let b = [4, 5, 6]
let combined = [...a, 0, ...b]
println(combined)   // [1, 2, 3, 0, 4, 5, 6]
```

**Basic methods:**

```que
let nums = [3, 1, 4, 1, 5, 9, 2, 6]
println(nums.len())             // 8
println(nums.first())           // 3
println(nums.last())            // 6
println(nums.sort())            // [1, 1, 2, 3, 4, 5, 6, 9]
println(nums.reverse())         // [6, 2, 9, 5, 1, 4, 1, 3]
println(nums.contains(5))       // true
println(nums.is_empty())        // false
```

**Functional methods** — this is where Que really shines:

```que
let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

// Keep only even numbers, square them, sum the result
let result = numbers
    .filter(|x| x % 2 == 0)
    .map(|x| x * x)
    .fold(0, |acc, x| acc + x)

println(result)   // 220
```

```que
let items = [10, 20, 30, 40]

// find returns the first match, or null
println(items.find(|x| x > 25))     // 30

// any / all return booleans
println(items.any(|x| x > 100))     // false
println(items.all(|x| x > 5))       // true
```

| Method | Description |
|--------|-------------|
| `.len()` | Number of elements |
| `.is_empty()` | Check if empty |
| `.first()` | First element or `null` |
| `.last()` | Last element or `null` |
| `.get(i, default?)` | Element at `i`, or `default` / `null` if out of bounds |
| `.push(val)` | Return new list with value appended |
| `.pop()` | Return `(shortened_list, popped_value)` |
| `.reverse()` | Return reversed list |
| `.sort()` | Return sorted list |
| `.sort_by(fn)` | Sort by comparator function |
| `.contains(val)` | Check membership |
| `.index_of(val)` | Index of value or `-1` |
| `.join(sep)` | Join into string with separator |
| `.map(fn)` | Transform each element |
| `.filter(fn)` | Keep elements matching predicate |
| `.fold(init, fn)` | Reduce to single value |
| `.find(fn)` | First element matching predicate, or `null` |
| `.any(fn)` | True if any element matches |
| `.all(fn)` | True if all elements match |
| `.flat_map(fn)` | Map then flatten one level |
| `.flatten()` | Flatten nested lists one level |
| `.enumerate()` | Pairs of `(index, value)` |
| `.take(n)` | First `n` elements |
| `.skip(n)` | All elements after the first `n` |
| `.slice(start, end)` | Sub-list by index range |
| `.chunk(n)` | Split into chunks of size `n` |
| `.window(n)` | Sliding windows of size `n` |
| `.zip(other)` | Pair elements with another list |
| `.unique()` | Remove duplicates |
| `.partition(fn)` | Split into `(matches, non_matches)` |
| `.group_by(fn)` | Group elements by key function (returns map) |
| `.each(fn)` | Execute function for each element |
| `.to_tuple()` | Convert to tuple |
| `.to_set()` | Convert to set (deduplicates) |
| `.to_map()` | Convert list of `[key, value]` pairs to map |

### Maps

Maps are key-value collections using `{}` with `"key": value` syntax:

```que
let config = {
    "host": "localhost",
    "port": 8080,
    "debug": true,
}
```

**Field access** — dot notation or index notation:

```que
println(config.host)       // localhost
println(config["debug"])   // true
```

**Missing keys raise.** Indexing is the strict form for both maps and lists,
so a typo fails at the point of the typo instead of quietly producing `null`:

```que
println(config["hsot"])    // error: key 'hsot' not found in map
println([1, 2, 3][5])      // error: index 5 out of bounds (len 3)
```

Use `.get()` when absence is expected — it is the lenient form on both types:

```que
println(config.get("hsot") ?? "localhost")   // localhost
println(config.get("hsot", "localhost"))     // localhost
println([1, 2, 3].get(5, 0))                 // 0
```

**Spread** — merge maps inline:

```que
let defaults = { "timeout": 30, "retries": 3 }
let config = { ...defaults, "timeout": 60 }
println(config.timeout)    // 60
println(config.retries)    // 3
```

**Nested field assignment** — mutate deeply nested maps and lists in place:

```que
mut config = { server: { host: "localhost", port: 8080 } }
config.server.port = 9090
println(config.server.port)    // 9090

// Works with mixed field/index chains too
mut data = { items: [{ name: "a" }, { name: "b" }] }
data.items[1].name = "updated"
println(data.items[1].name)    // updated

// Compound assignment works through nested paths
mut counters = { stats: { hits: 10 } }
counters.stats.hits += 5
println(counters.stats.hits)   // 15
```

**Iteration:**

```que
let scores = { "alice": 95, "bob": 87, "charlie": 92 }

mut total = 0
for (name, score) in scores {
    total += score
}
println(total)   // 274
```

| Method | Description |
|--------|-------------|
| `.len()` | Number of entries |
| `.is_empty()` | Check if empty |
| `.keys()` | List of keys |
| `.values()` | List of values |
| `.entries()` | List of `(key, value)` tuples |
| `.contains(key)` | Check if key exists |
| `.get(key, default?)` | Get value, or `default` / `null` if absent |
| `.merge(other)` | Return merged map |
| `.deep_merge(other)` | Recursively merge nested maps |
| `.remove(key)` | Return map without key |
| `.map_values(fn)` | Transform all values, return a new Map |
| `.filter_values(fn)` | Keep entries where value matches, return a new Map |
| `.to_list()` | Convert to list of `[key, value]` pairs |

> **Tip:** `map_values` and `filter_values` preserve the map structure.
> Using `m.values().filter(...)` gives you a flat list — keys are lost.
> Use the methods when you need the result to remain a map:
> ```
> let m = {"a": 1, "b": 2, "c": 3}
> m.filter_values(|v| v > 1)          // {"b": 2, "c": 3}
> m.values().filter(|v| v > 1)        // [2, 3] — keys lost
> ```

### Tuples

Tuples are fixed-size, ordered collections created with `(a, b, c)`:

```que
let point = (10, 20)
println(point[0])    // 10
println(point[1])    // 20
```

| Method | Description |
|--------|-------------|
| `.len()` | Number of elements |
| `.to_list()` | Convert to list |
| `.to_set()` | Convert to set (deduplicates) |
| `.contains(val)` | Check membership |
| `.first()` | First element |
| `.last()` | Last element |

### Sets

A **Set** is an unordered collection of unique values, written `#{ ... }`.
Duplicate elements are silently discarded:

```que
let numbers = #{1, 2, 3}
let deduped = #{1, 2, 2, 3, 3, 3}
println(deduped)              // #{1, 2, 3}

let empty = #{}               // the empty set
let single = #{42}            // one element — no trailing comma needed
```

Sets have their own opener so `{ ... }` is never ambiguous: `{}` is the empty
map, `{k: v}` a map, and `{ expr }` a block.

**Operations:**

```que
let a = #{1, 2, 3}
let b = #{3, 4, 5}

a.union(b)                   // #{1, 2, 3, 4, 5}
a.intersection(b)            // #{3}
a.difference(b)              // #{1, 2}
a.symmetric_difference(b)    // #{1, 2, 4, 5}
```

**Operators** provide a concise alternative:

```que
a + b    // union:                #{1, 2, 3, 4, 5}
a - b    // difference:           #{1, 2}
a & b    // intersection:         #{3}
a ^ b    // symmetric difference: #{1, 2, 4, 5}
```

| Method | Description |
|--------|-------------|
| `.len()` | Number of elements |
| `.is_empty()` | Check if empty |
| `.contains(val)` | Membership test |
| `.add(val)` | New set with element added |
| `.remove(val)` | New set with element removed |
| `.union(other)` | All elements from both |
| `.intersection(other)` | Elements in both |
| `.difference(other)` | Elements in self but not other |
| `.symmetric_difference(other)` | Elements in exactly one |
| `.is_subset(other)` | True if all elements in other |
| `.is_superset(other)` | True if other is subset |
| `.is_disjoint(other)` | True if no common elements |
| `.to_list()` | Convert to list |
| `.to_tuple()` | Convert to tuple |
| `.map(fn)` | Transform elements (deduplicates) |
| `.filter(fn)` | Keep matching elements |
| `.each(fn)` | Iterate with side effects |

### Converting Between Collection Types

```que
let list = [1, 2, 3]
println(list.to_tuple())  // (1, 2, 3)
println(list.to_set())    // #{1, 2, 3}

let set = #{1, 2, 3}
println(set.to_list())    // [1, 2, 3]

let tuple = (1, 2, 3)
println(tuple.to_list())  // [1, 2, 3]

// List of pairs to Map
let pairs = [["a", 1], ["b", 2]]
println(pairs.to_map())   // {"a": 1, "b": 2}

// Map to list of [key, value] pairs
let map = {"x": 10, "y": 20}
println(map.to_list())    // [["x", 10], ["y", 20]]
```

---

## 6. Functions

Define functions with `fn`. The last expression is the implicit return value:

```que
fn greet(name) {
    "Hello, " + name + "!"
}

println(greet("Que"))   // Hello, Que!
```

### Explicit Return

Use `return` for early exits:

```que
fn binary_search(arr, target) {
    mut low = 0
    mut high = arr.len() - 1
    while low <= high {
        let mid = (low + high) / 2
        if arr[mid] == target {
            return Ok(mid)
        } else if arr[mid] < target {
            low = mid + 1
        } else {
            high = mid - 1
        }
    }
    Err("not found")
}
```

### Default Parameters

```que
fn http_get(url, timeout = 30, retries = 3) {
    println("${url} timeout=${timeout} retries=${retries}")
}

http_get("https://example.com")           // timeout=30, retries=3
http_get("https://example.com", 10)       // timeout=10, retries=3
http_get("https://example.com", 10, 5)    // timeout=10, retries=5
```

### Named Arguments

Call functions with named arguments for clarity. Named arguments can appear in
any order and mix with positional arguments:

```que
fn deploy(target, dryRun = false) {
    if dryRun { "dry: " + target }
    else { "real: " + target }
}

println(deploy("prod", dryRun: true))   // dry: prod
```

```que
fn add(a, b, c) {
    a + b + c
}

// Positional first, then named in any order
println(add(1, c: 30, b: 20))   // 51
```

### Recursion

```que
fn factorial(n) {
    if n <= 1 { 1 }
    else { n * factorial(n - 1) }
}

println(factorial(10))   // 3628800
```

### Higher-Order Functions

Functions are first-class values. Pass them as arguments and return them:

```que
fn compose(f, g) {
    |x| f(g(x))
}

let double = |x| x * 2
let inc = |x| x + 1
let double_then_inc = compose(inc, double)
println(double_then_inc(5))   // 11
```

### Partial Application

Use `_` as a placeholder to create a new function with some arguments filled in:

```que
fn add(a, b) { a + b }

let add5 = add(5, _)
println(add5(3))    // 8

fn greet(greeting, name) { greeting + " " + name }
let hello = greet("Hello", _)
println(hello("World"))   // Hello World
```

---

## 7. Closures and the Pipe Operator

### Lambda Syntax

Lambdas (anonymous functions) use the `|params| body` syntax — this is the
*only* closure syntax. `fn` always declares a named function.

```que
let double = |x| x * 2
let add = |a, b| a + b
```

For multi-line bodies, use a block:

```que
let process = |x| {
    let squared = x * x
    let doubled = squared * 2
    doubled
}
```

Zero-parameter lambdas use `||`:

```que
let get_value = || 42
println(get_value())   // 42
```

Lambda parameters accept the same type annotations and default values as named
function parameters:

```que
let scale = |x: Int, factor: Int = 2| x * factor
println(scale(5))      // 10
println(scale(5, 3))   // 15
```

A default is parsed below bitwise-or precedence, so a bare `|` ends the
parameter list. Parenthesise a default that needs `|`, `||`, `??` or `|>`:

```que
let mask = |x, m = (0xF0 | 0x0F)| x & m
```

### Mutable Capture

Closures capture variables **by reference**. Mutations to `mut` variables
inside a closure are visible to the outer scope:

```que
mut count = 0
let inc = || { count = count + 1 }
inc()
inc()
println(count)   // 2
```

### Factory Pattern

Closures returned from functions retain access to their creating scope.
Each call creates independent state:

```que
fn make_counter() {
    mut n = 0
    return || {
        n = n + 1
        return n
    }
}

let a = make_counter()
let b = make_counter()
println(a())   // 1
println(a())   // 2
println(b())   // 1  — independent state
```

### The Pipe Operator

The pipe operator `|>` passes the left-hand value as the first argument to the
right-hand function. This turns deeply nested calls into readable pipelines:

```que
fn double(x) { x * 2 }
fn square(x) { x * x }
fn negate(x) { -x }

let result = 3 |> double |> square |> negate
println(result)   // -36
```

When piping into a function call with arguments, the piped value becomes the
first argument:

```que
fn add(a, b) { a + b }
fn mul(a, b) { a * b }

let result = 5 |> add(3) |> mul(2)
println(result)   // 16  —  (5+3)*2
```

### Multiline Pipes with Lambdas

For complex transformations, use lambdas in the pipe:

```que
let result = [1, 2, 3, 4, 5]
    |> |list| list.filter(|x| x % 2 == 1)
    |> |list| list.map(|x| x * x)
    |> |list| list.fold(0, |a, b| a + b)

println(result)   // 35  —  1 + 9 + 25
```

### Breaking a Long Expression Across Lines

A statement ends at a newline, but a line that *starts* with a binary operator
continues the one above it, because it could not be a statement on its own:

```que
let pattern = base
    / "profiles"
    / "*.toml"

let total = subtotal
    + shipping
    + tax

let ok = user.is_active()
    && user.has_licence()
    && !user.is_banned()
```

The same holds for `.`, `?.` and `|>`, and blank lines between the parts are
fine. Two operators are excluded, because they can also *begin* an expression:
`-` is unary negation and `|` opens a lambda. For those, put the operator at
the end of the line instead:

```que
let diff = subtotal -
    discount
```

Newlines inside `(`, `[`, `{` and `#{` never end a statement, so arguments,
collection literals and match arms can be spread out freely. The same goes for
a closure's parameter list and for a `${...}` interpolation:

```que
let describe = |
    name: Str,
    version: Str = "dev",
| "${name}@${version}"

println("report for ${
    user.name()
        .to_upper()
}")
```

`que fmt` collapses all of these back onto one line, so use them where the
source reads better, not where the formatter will preserve them.

---

## 8. Destructuring and Pattern Matching

### Let Destructuring

Unpack collections directly in `let` bindings:

```que
// List destructuring
let [first, second, ...rest] = [1, 2, 3, 4, 5]
println(first)    // 1
println(second)   // 2
println(rest)     // [3, 4, 5]

// Map destructuring
let { name, ...rest } = { name: "que", version: "1.0", license: "MIT" }
println(name)                // que
println(rest.keys().len())   // 2

// Tuple destructuring
let (x, y) = (10, 20)
println(x + y)   // 30
```

### Match Expressions

`match` is a powerful expression for conditional logic based on the **shape**
of data.

**Literal patterns:**

```que
fn describe(x) {
    match x {
        0 => "zero",
        1 => "one",
        _ => "other",
    }
}
```

**Guard clauses** — add conditions with `if`:

```que
fn classify_temp(celsius) {
    match celsius {
        c if c < 0 => "freezing",
        c if c < 20 => "cold",
        c if c < 30 => "warm",
        c => "hot (" + str(c) + ")",
    }
}
```

**Or patterns** — match multiple alternatives with `|`:

```que
fn is_weekend(day) {
    match day {
        "Saturday" | "Sunday" => true,
        _ => false,
    }
}
```

### List Patterns

```que
fn describe_list(list) {
    match list {
        [] => "empty",
        [x] => "single: " + str(x),
        [x, ...rest] => "head: " + str(x) + ", rest len: " + str(rest.len()),
    }
}

println(describe_list([]))           // empty
println(describe_list([42]))         // single: 42
println(describe_list([1, 2, 3]))    // head: 1, rest len: 2
```

### Map Patterns

```que
let point = { "x": 3, "y": 4 }
let sum = match point {
    { x, y } => x + y,
}
println(sum)   // 7
```

Capture remaining fields with `...rest`:

```que
let config = { name: "que", version: "1.0", author: "test", license: "MIT" }
match config {
    { name, ...rest } => println("${name}: ${rest.keys().len()} more fields"),
}
```

### Matching Result Types

```que
fn safe_divide(a, b) {
    if b == 0 { Err("division by zero") }
    else { Ok(a / b) }
}

match safe_divide(10, 2) {
    Ok(val) => println("Result: " + str(val)),
    Err(msg) => println("Error: " + msg),
}
```

### Glob Patterns

Match strings against glob patterns:

```que
let file = "main.rs"
let lang = match file {
    glob("*.rs") => "Rust",
    glob("*.py") => "Python",
    glob("*.js") => "JavaScript",
    _ => "Unknown",
}
println(lang)   // Rust
```

### Binding with `@`

Bind a name to the whole matched value while also matching a sub-pattern:

```que
match [1, 2, 3] {
    list @ [first, ...rest] => {
        println(first)         // 1
        println(rest)          // [2, 3]
        println(list.len())    // 3
    },
    _ => println("no match"),
}
```

### If-Let

A concise way to match and bind in one step:

```que
let result = Ok(42)
if let Ok(val) = result {
    println("got: " + str(val))   // got: 42
}
```

### Struct Patterns

Struct instances (see [Section 9](#9-structs-impl-blocks-and-traits)) can be
matched by type name and field values:

```que
struct Circle { radius: Float }
struct Rect { width: Float, height: Float }

fn area(shape) {
    match shape {
        Circle { radius } => 3.14159 * radius * radius,
        Rect { width, height } => width * height,
        _ => 0.0,
    }
}

println(area(Circle { radius: 5.0 }))             // 78.53975
println(area(Rect { width: 4.0, height: 3.0 }))   // 12.0
```

---

## 9. Structs, Impl Blocks, and Traits

### Structs

Define a named data type with `struct`:

```que
struct Point {
    x: Float,
    y: Float,
}

let p = Point { x: 3.0, y: 4.0 }
println(p.x)          // 3.0
println(typeof(p))    // Point
println(p)            // Point { x: 3.0, y: 4.0 }
```

**Default field values** — fields with a default are optional at construction:

```que
struct Config {
    host: String = "localhost",
    port: Int = 8080,
    debug: Bool = false,
}

let c = Config { port: 3000 }
println(c.host)    // localhost
println(c.port)    // 3000
```

**Field shorthand** — when the variable name matches:

```que
let x = 10.0
let y = 20.0
let p = Point { x, y }    // same as Point { x: x, y: y }
```

**Mutable field assignment:**

```que
mut p = Point { x: 1.0, y: 2.0 }
p.x = 99.0
println(p.x)    // 99.0
```

### Impl Blocks

Add methods to a struct with `impl`:

```que
impl Point {
    // Instance method — first parameter is `self`
    fn length(self) {
        (self.x * self.x + self.y * self.y) ** 0.5
    }

    fn translate(self, dx, dy) {
        Point { x: self.x + dx, y: self.y + dy }
    }

    fn scale(self, factor) {
        Point { x: self.x * factor, y: self.y * factor }
    }

    // Static method — no `self` parameter
    fn origin() {
        Point { x: 0.0, y: 0.0 }
    }
}

let p = Point { x: 3.0, y: 4.0 }
println(p.length())        // 5.0

// Method chaining
let p2 = Point.origin().translate(1.0, 1.0).scale(3.0)
println(p2.x)              // 3.0
```

**Constructor shorthand** — if you define a static method named `new`, you can
call the type name directly as a function instead of `Type.new(...)`:

```que
impl Point {
    fn new(x, y) -> Point { Point { x, y } }
    fn length(self) { (self.x * self.x + self.y * self.y) ** 0.5 }
}

let p = Point(3.0, 4.0)    // same as Point.new(3.0, 4.0)
println(p.length())         // 5.0

// Chains naturally
println(Point(1.0, 0.0).length())    // 1.0
```

Struct instances are **values**, not references. Methods that "modify" return
a new instance:

```que
struct Counter { count: Int = 0 }

impl Counter {
    fn increment(self) { Counter { count: self.count + 1 } }
    fn value(self) { self.count }
}

mut c = Counter {}
c = c.increment()
c = c.increment()
println(c.value())    // 2
```

**`mut self`** — a method that declares its receiver `mut` may assign to its
own fields, and what it leaves in `self` is written back over the value it was
called on:

```que
impl Counter {
    fn bump(mut self) { self.count = self.count + 1 }
}

mut c = Counter {}
c.bump()
c.bump()
println(c.value())    // 2
```

This is a rebinding, not a reference: `c.bump()` means `c = <c after bump>`.
Two consequences follow from that, and both are errors rather than surprises:

- The receiver must be declared with `mut`, because the call assigns to it.
  `let c = Counter {}` then `c.bump()` is rejected.
- The receiver must be something that can be assigned to. `Counter().bump()`
  is rejected, because the mutation would land in a value that is discarded on
  the same line.

A plain `self` stays immutable, so a method cannot change its receiver by
accident — assigning to `self.field` without `mut self` is an error. And `mut`
is only allowed on the receiver: an ordinary parameter is a copy the caller
never sees again, so marking it mutable would promise something Que does not do.

Two names never refer to one instance. If you need shared mutable state, reach
for a value that is genuinely one resource — a `Stream`, a file handle, a
process handle — rather than a struct.

### Traits

A **trait** defines a set of methods that a type must implement:

```que
trait Describable {
    fn describe(self) -> String
}

struct Dog { name: String, breed: String }
struct Cat { name: String, indoor: Bool }

impl Describable for Dog {
    fn describe(self) {
        "${self.name} is a ${self.breed}"
    }
}

impl Describable for Cat {
    fn describe(self) {
        let place = if self.indoor { "indoor" } else { "outdoor" }
        "${self.name} is an ${place} cat"
    }
}

let d = Dog { name: "Rex", breed: "Labrador" }
let c = Cat { name: "Whiskers", indoor: true }
println(d.describe())    // Rex is a Labrador
println(c.describe())    // Whiskers is an indoor cat
```

**Default method implementations:**

```que
trait Greetable {
    fn greet(self) -> String {
        "Hello, I am " + self.name
    }
}

struct Robot { name: String }
impl Greetable for Robot {}    // uses the default

struct Person { name: String, formal: Bool = false }
impl Greetable for Person {
    fn greet(self) {
        if self.formal { "Good day. My name is " + self.name }
        else { "Hey, I'm " + self.name }
    }
}
```

### Built-in Protocol Traits

Que ships with four built-in traits that wire your types into core language
operators. You don't declare them — just implement them.

---

#### `Display` — string coercion

Implement `to_string(self) -> String` to control how your type prints in
`println()`, string interpolation `"${val}"`, and the `str()` builtin.

```que
struct Color { r: Int, g: Int, b: Int }

impl Display for Color {
    fn to_string(self) -> String {
        "rgb(${self.r}, ${self.g}, ${self.b})"
    }
}

let c = Color { r: 255, g: 128, b: 0 }
println(c)              // rgb(255, 128, 0)
println("Fill: ${c}")   // Fill: rgb(255, 128, 0)
```

---

#### `Eq` — equality operators

Implement `equals(self, other) -> Bool` to make `==` and `!=` dispatch to your
logic instead of comparing fields structurally.

```que
struct Version { major: Int, minor: Int, patch: Int }

impl Eq for Version {
    fn equals(self, other) -> Bool {
        self.major == other.major &&
        self.minor == other.minor &&
        self.patch == other.patch
    }
}

let v1 = Version { major: 1, minor: 2, patch: 0 }
let v2 = Version { major: 1, minor: 2, patch: 0 }
println(v1 == v2)    // true
println(v1 != v2)    // false
```

---

#### `Ord` — comparison operators

Implement `compare(self, other) -> Int` (returning `-1`, `0`, or `1`) to make
`<`, `>`, `<=`, and `>=` work on your type, and to enable `sort_by` on lists of
your instances.

```que
struct Version { major: Int, minor: Int, patch: Int }

impl Ord for Version {
    fn compare(self, other) -> Int {
        if self.major != other.major { return self.major - other.major }
        if self.minor != other.minor { return self.minor - other.minor }
        self.patch - other.patch
    }
}

let v1 = Version { major: 1, minor: 0, patch: 0 }
let v2 = Version { major: 2, minor: 0, patch: 0 }
println(v1 < v2)    // true

let versions = [v2, v1]
let sorted = versions.sort_by(|a, b| a.compare(b))
println(sorted[0].major)    // 1
```

---

#### `Hash` — use as a Set element or Map key

Implement `hash(self) -> Int` to allow your type to be used as a Set element or
Map key. You should also implement `Eq` alongside `Hash` so that membership
checks use your equality logic.

```que
struct Point { x: Int, y: Int }

impl Hash for Point {
    fn hash(self) -> Int { self.x * 31 + self.y }
}

impl Eq for Point {
    fn equals(self, other) -> Bool {
        self.x == other.x && self.y == other.y
    }
}

let origin = Point { x: 0, y: 0 }
let unit   = Point { x: 1, y: 0 }
let also_origin = Point { x: 0, y: 0 }

mut visited = #{origin, unit}
visited = visited.add(also_origin)   // deduplicated via equals()
println(visited.len())                // 2
println(visited.contains(also_origin))  // true
```

> **Rule:** A struct without a `Hash` implementation cannot be added to a Set
> or used as a Map key — Que will raise a runtime error.

---

### The `Contextual` Trait — Custom Context Managers

Any struct implementing `Contextual` can be used in `with ... as` blocks:

```que
trait Contextual {
    fn enter(self)             // returns the resource bound by `as`
    fn exit(self, resource)    // cleans up; called even on error
}
```

**Writing your own context manager:**

```que
import std.time

struct WorkDir { base: Path }

impl Contextual for WorkDir {
    fn enter(self) {
        let dir = self.base.join("workspace_" + str(time.timestamp()))
        dir.mkdir()
        dir
    }

    fn exit(self, dir) {
        dir.delete()
    }
}

with WorkDir { base: path("/tmp") } as dir {
    dir.join("output.txt").write_text("results").unwrap()
}
// workspace dir removed automatically
```

You'll meet the two built-in `Contextual` types — `TempDir` and `TempFile` —
in [Section 13](#13-paths), and `env.scope` in [Section 21](#21-environment-and-platform).

---

## 10. Enums

Enums define a type with a fixed set of named **variants**. Each variant can
carry **associated data** (named fields), just like Rust enums.

### Declaring an Enum

```que
enum Direction { North, South, East, West }

enum Shape {
    Circle { radius: Float }
    Rect   { width: Float, height: Float }
    Point
}

enum Msg {
    Quit
    Move  { x: Int, y: Int }
    Write { text: String }
}
```

A data variant may be declared with either delimiter — `{ ... }` or `( ... )`.
The two are identical:

```que
enum State {
    Running { pid: Int }      // brace form
    Failed(code: Int, msg: String)   // paren form — same thing
}
```

### Constructing Enum Values

**Unit variants** (no fields) are accessible as `EnumName.Variant` and are also
bound directly in scope by their bare name after the declaration:

```que
enum Direction { North, South, East, West }

let d = Direction.North   // qualified
let e = North             // bare name — same value
println(d)                // Direction.North
```

**Data variants** are constructed with either delimiter, matching the
declaration. Named and positional arguments both work:

```que
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float }, Point }

let c = Shape.Circle { radius: 5.0 }       // brace form, named fields
let c2 = Shape.Circle(radius: 5.0)         // paren form — identical value
let r = Shape.Rect(3.0, 4.0)               // positional — maps to width, height
let p = Shape.Point                        // unit

println(c == c2)   // true
println(c)         // Shape.Circle {radius: 5}
println(p)         // Shape.Point
```

The brace form requires explicit `field: value` pairs. This keeps
`if s == State.Idle { ... }` parsing as a unit variant followed by a block —
only `Variant {` immediately followed by `name:` is read as a constructor.
Use the paren form when you need positional arguments.

### Pattern Matching

Match enum values with named-field destructuring patterns (`Variant { field }`).
Unit variants use an empty brace pattern:

```que
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float }, Point }

let s = Shape.Circle(radius: 2.5)

match s {
    Circle { radius } => println("circle r=${radius}")
    Rect   { width, height } => println("rect ${width}x${height}")
    Point  {} => println("point")
}
// circle r=2.5
```

Bare variant patterns like `Circle { ... }`, `Point {}`, and `Ok(v)` continue to
work for the common uppercase style.

When a variant name would otherwise be ambiguous with a binding, use the
qualified form `EnumName.Variant` in the pattern. This is especially useful for
lowercase enum variants:

```que
enum Status { ok, err { code: Int } }

let status = Status.err(code: 404)

match status {
    Status.ok => println("ok")
    Status.err { code } => println("err ${code}")
}
```

Qualified patterns also work with positional variants:

```que
enum Response { success(body: String), failure(code: Int) }

match Response.failure(500) {
    Response.success(body) => println(body)
    Response.failure(code) => println(code)
}
```

Note that a data variant's fields always need names in the declaration, even
when you plan to construct and match it positionally — `success(String)`
without a field name is a parse error; `success(body: String)` is what makes
`Response.success("ok")` and the positional pattern above both work.

Guards and wildcards work as usual:

```que
match s {
    Circle { radius } if radius > 10.0 => println("big circle")
    Circle { radius } => println("small circle r=${radius}")
    _ => println("other shape")
}
```

### Enum Methods with `impl`

Add instance methods to an enum with `impl`. Inside the method, use `match self`
to branch on the variant:

```que
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float } }

impl Shape {
    fn area(self) -> Float {
        match self {
            Circle { radius } => 3.14159 * radius * radius
            Rect   { width, height } => width * height
        }
    }

    fn describe(self) -> String {
        match self {
            Circle { radius } => "circle with radius ${radius}"
            Rect   { width, height } => "${width}×${height} rectangle"
        }
    }
}

let c = Shape.Circle(radius: 4.0)
println(c.area())       // 50.26544
println(c.describe())   // circle with radius 4
```

### Built-in Enum Methods

Every enum value has these built-in methods:

| Method | Returns | Description |
|--------|---------|-------------|
| `.variant()` | `String` | The variant name (`"Circle"`, `"North"`, …) |
| `.enum_name()` | `String` | The enum type name (`"Shape"`, `"Direction"`, …) |
| `.is_variant(name)` | `Bool` | Check which variant this is |
| `.fields()` | `Map` | All associated data fields as a map |
| `.inspect()` | `Map` | Full introspection map |

```que
let s = Shape.Circle(radius: 3.0)
println(s.variant())           // Circle
println(s.enum_name())         // Shape
println(s.is_variant("Rect"))  // false
println(s.fields())            // {radius: 3.0}
```

### Enums in Modules

Mark an enum `pub` to export it. The type name (as a `TypeRef`) and all unit
variant bindings are exported automatically:

```que
// shapes.que
pub enum Color { Red, Green, Blue }
pub enum Shape { Circle { radius: Float }, Point }
```

```que
// main.que
import .shapes { Color, Shape, Red, Green }

let c = Shape.Circle(radius: 1.0)
let color = Red
```

---

## 11. Error Handling

Que uses `Result` (`Ok` / `Err`) for operations that can fail, and plain `null`
for "there is no value here".

### Creating Results

```que
let good = Ok(42)
let bad = Err("something went wrong")
```

### The `?` Operator

The try operator `?` unwraps an `Ok` or propagates an `Err` up to the caller:

```que
fn parse_number(s) {
    if s == "42" { Ok(42) }
    else { Err("not a number") }
}

fn process() {
    let n = parse_number("42")?   // unwraps to 42
    n * 2
}

println(process())   // 84
```

### The `??` Null Coalescing Operator

Provide fallback values for `null`:

```que
let a = null
let b = null
let c = 42
let result = a ?? b ?? c
println(result)   // 42
```

### Result Methods

```que
let val = Ok(5)
let doubled = val.map(|x| x * 2)
println(doubled.unwrap())   // 10
```

| Method | Description |
|--------|-------------|
| `.unwrap()` | Extract value (errors on `Err`) |
| `.unwrap_or(default)` | Extract value or return default |
| `.unwrap_err()` | Extract the error value from `Err` |
| `.is_ok()` | `true` if `Ok` |
| `.is_err()` | `true` if `Err` |
| `.map(fn)` | Transform the inner value |
| `.and_then(fn)` | Chain with a function returning `Result` |
| `.map_err(fn)` | Transform the error value |
| `.or_else(fn)` | Recover from error with a function |

### Missing Values

There is no `Option` type. Anything that may not produce a value returns `null`:

```que
let found = [1, 2, 3].find(|x| x > 2)    // 3
let missing = [1, 2, 3].find(|x| x > 10) // null

println(found ?? 0)     // 3
println(missing ?? 0)   // 0
```

### Optional Chaining with `?.`

`?.` reads a field or calls a method only when the receiver is not `null`;
otherwise the whole chain short-circuits to `null`:

```que
let user = load_user()
println(user?.address?.city)          // null if either link is missing
println(user?.address?.city ?? "n/a") // combine with ?? for a fallback

let name = null
println(name?.to_upper())             // null — no error
```

This replaces `Option`, `Some`, `None`, `.is_some()` and `.is_none()`, which
were all removed.

`?.` also carries the meaning of `?` on a `Result`, because the two characters
are read as one operator: `res?.field` takes the field of the value, not of the
`Ok` around it, and an `Err` propagates as `?` would.

```que
let n = glob("src/**/*.txt").copy_to(p"/tmp/out")?.len()
let text = path("notes.md").read()?.trim()
```

### Error Context with `.context()`

Add descriptive context when propagating errors. Instead of seeing "file not
found" deep in a call stack, you see the full chain of what was being attempted:

```que
fn load_config(p) {
    p.read().context("reading service config")?
}

fn deploy(service) {
    let cfg = load_config(path("services/${service}/config.yaml"))
        .context("loading ${service} configuration")?
    // ... deploy using cfg
}
```

When the file is missing, the error message shows the full breadcrumb trail:
```
Error: loading api configuration: reading service config: No such file or directory
```

How `.context()` works:
- `Ok(v).context(msg)` — pass-through, returns `Ok(v)` unchanged
- `Err(e).context(msg)` — wraps the error: `Err("${msg}: ${e}")`

### One Error Channel

`Err` is a value you can inspect, but it is not a way to fail quietly. An `Err`
that reaches **statement position** — nothing binds it, matches it or unwraps
it — is raised as an error:

```que
fn might_fail() -> Result<String> { Err("boom") }

might_fail()             // raises: boom
println("not reached")
```

Handling it in any of the usual ways keeps it a value:

```que
let r = might_fail()          // bound — no raise
if r.is_err() { ... }

match might_fail() {          // matched — no raise
    Ok(v)  => println(v),
    Err(e) => println("failed: ${e}"),
}

let v = might_fail() ?? "default"
```

This means `try`/`catch` catches `Err` values too — the raising channel
(`fail()`, failing commands) and the value channel (`Err`) are the
same channel:

```que
try {
    might_fail()
} catch e {
    println("caught: ${e}")   // caught: boom
}
```

And it means a script that ends on an unhandled `Err` exits non-zero instead of
reporting success.

### Exit Codes

Every way a Que script can stop maps to a defined process exit code, so CI
steps can gate on the result:

| Cause | Exit code |
|-------|-----------|
| Ran to completion | `0` |
| `fail(msg)`, failed assertion, unhandled `Err`, uncaught runtime error | `1` |
| Bad invocation, unreadable file, lex or parse error | `2` |
| `os.exit(n)` | `n` |
| `fail(msg, n)` | `n` |
| A command that exits non-zero | the command's own exit code |
| Interrupted by SIGINT / SIGTERM | `130` / `143` |

The last two rows are what make a script usable as a CI step:

```que
`pytest tests/`              // if pytest exits 5, the script exits 5

if !health_ok {
    fail("health check failed", 75)   // script exits 75
}
```

The split between `1` and `2` separates "your build broke" from "you invoked me
wrong" — a parse error in a Quefile should not look like a failing test.

### Try / Catch / Finally

For imperative error handling:

```que
mut result = ""
try {
    fail("something went wrong")
} catch e {
    result = "caught: " + e
}
println(result)   // caught: something went wrong
```

The `finally` block always runs, even if an error was caught:

```que
mut cleanup = false
try {
    fail("oops")
} catch e {
    println("caught")
} finally {
    cleanup = true
}
println(cleanup)   // true
```

### Defer

`defer` schedules an expression to run when the **current block exits** —
whether normally, via `return`, due to an error, via `os.exit()`, or because
the script was interrupted with Ctrl-C. Multiple deferred expressions run in
reverse order (last registered, first to run).

```que
fn deploy(env) {
    println("starting deploy to ${env}")
    defer println("deploy to ${env} finished")

    // ... deployment steps ...
}
```

Unlike `finally`, `defer` doesn't require a `try` block — it works in any
scope, including the top level of a script:

```que
fn with_temp(work) {
    let tmp = path("/tmp/workdir")
    tmp.mkdir()
    defer tmp.delete()   // cleaned up when the function returns

    work(tmp)            // defer runs after this, even on error
}
```

### Interrupts

SIGINT (Ctrl-C) and SIGTERM do not kill the script outright. They unwind it,
so every `defer` on the stack runs first:

```que
let db = spawn(`docker run --rm postgres`)
defer db.kill()

`./run-integration-tests.sh`   // Ctrl-C here still stops the container
```

The script then exits with the shell convention of 128 + the signal number:
**130** for SIGINT, **143** for SIGTERM. An interrupt is deliberately not
catchable with `try`/`catch` — a script cannot refuse Ctrl-C.

Because the check happens between statements, a script blocked in a long-running
child process reacts when that process returns. In a terminal the signal reaches
the child too, so this is usually immediate.

---

## 12. Modules and Imports

As your Que projects grow beyond a single file, the module system lets you
split code into reusable pieces. The filesystem **is** the module tree — no
`mod` declarations needed.

### Your First Module

Create a file called `utils.que`:

```que
// utils.que
pub fn greet(name) {
    "Hello, " + name + "!"
}

pub fn slug(s) {
    s.to_lower().replace(" ", "-")
}

// No `pub` — private to this file
fn internal_helper() {
    "you can't see me"
}
```

Then import from your main script:

```que
// main.que
import .utils

println(utils.greet("Que"))       // Hello, Que!
println(utils.slug("Hello World")) // hello-world
```

The leading `.` means "local module." The module is bound to the last path
segment (`utils`) as a namespace.

### Import Forms

```que
// Namespace import — module bound to `math`
import .lib.math
println(math.add(2, 3))

// Aliased — choose your own name
import .lib.strings as str
println(str.capitalize("que"))

// Selective — bring specific names into scope directly
import .lib.math { add, mul }
println(add(10, 20))

// Multi-module shorthand
import .{utils, config}
```

### Visibility with `pub`

Only declarations marked `pub` are visible to importers:

```que
// config.que
pub fn app_name() { "My App" }
pub fn max_retries() { 3 }
fn secret_key() { "don't export me" }  // private
```

### Directory Modules with `mod.que`

When a module grows large, create a `mod.que` entry point:

```
project/
  main.que
  lib/
    mod.que          <-- import .lib loads this
    math.que         <-- import .lib.math
    strings.que      <-- import .lib.strings
```

Re-export children with `pub import`. Inside `lib/mod.que` the dot is
relative to `lib/`, so `.math` resolves to `lib/math.que`:

```que
// lib/mod.que
pub import .math
pub import .strings

pub fn version() { v"1.0.0" }
```

Consumers access everything through the `lib` namespace:

```que
import .lib
println(lib.version())
println(lib.math.add(2, 3))
```

A re-exported `struct` or `enum` keeps its `impl` blocks, however many
modules it travels through, so a facade module can pass a type on without
its methods going missing.

### Standard Library Imports

Domain-specific functions are available via `std.*` module imports. Core
utilities like `println`, `len`, `typeof`, `path`, `env` remain globals.

```que
import std.fs { read, write, exists }
import std.json { parse, stringify }

let data = read(path("./config.json"))?
let config = parse(data)?
```

Or use namespace imports:

```que
import std.fs
import std.json

let data = fs.read(path("./config.json"))?
let config = json.parse(data)?
```

### Modules Are Maps

At runtime, a module is just a `Map` of its exported names:

```que
import .lib.math

println(typeof(math))   // Map
println(math.keys())     // ["add", "factorial", "mul", "sub", ...]

fn compute(m, a, b) {
    m.add(a, m.mul(a, b))
}
println(compute(math, 3, 4))   // 15
```

### Module Caching

Each module is loaded once. Subsequent imports return the cached result:

```que
// lib/math.que
println("Loading math module...")   // prints once, no matter how many importers

pub fn add(a, b) { a + b }
```

### Module Resolution Summary

The **package root** is the directory containing your project's `que.toml`,
or — for a loose script run with `que script.que` — the script's own
directory. Bare (no-dot) imports are resolved from the package root.
Local imports (with a leading `.`) are resolved **relative to the directory
of the file containing the import statement**.

| Import | In file | Resolves to |
|--------|---------|-------------|
| `import .utils` | `main.que` | `<root>/utils.que` |
| `import .lib.math` | `main.que` | `<root>/lib/math.que` |
| `import .lib` | `main.que` | `<root>/lib/mod.que` |
| `import .math` | `lib/mod.que` | `<root>/lib/math.que` |
| `import .cursor` | `que_packages/escapes/mod.que` | `que_packages/escapes/cursor.que` |
| `import std.fs` | any | Built-in |
| `import colors` | any | `<root>/que_packages/colors/mod.que` |
| `import colors.shades` | any | `<root>/que_packages/colors/shades.que` |

> **Common gotcha:** `import .colors` is **local** and resolves relative to the
> importing file, not from `que_packages/`. To import a package, drop the
> leading dot — and remember the package must be a **directory** containing
> a `mod.que`, not a loose `que_packages/colors.que` file.

### External Packages with `que_packages/`

Bare (no-dot) imports name an external package. Each package lives in its
own directory under `que_packages/` and must provide a `mod.que` as its
entry point:

```
project/
  que.toml
  main.que
  que_packages/
    colors/
      mod.que          <-- import colors
      shades.que       <-- import colors.shades
    deploy_tools/
      mod.que          <-- import deploy_tools
      k8s.que          <-- import deploy_tools.k8s
```

```que
// main.que
import colors { Color }              // selective from mod.que
import colors.shades as palette      // sub-module, aliased
import deploy_tools                  // whole package as namespace

let c = Color.Red
palette.lighten(c)
deploy_tools.deploy("staging")
```

Hyphens in package names are normalized to underscores on disk:
`import my-tool` looks for `que_packages/my_tool/`.

If you have a single-file utility you want to reuse without packaging it,
put it at the package root (e.g. `<root>/colors.que`) and use a local
import: `import .colors { Color }`.

### Declaring Dependencies (`que.toml` and `que install`)

`que_packages/` is where packages live; `que.toml` is what says which ones
belong there and at what version.

```toml
# que.toml
[package]
name = "deploy-scripts"
version = "0.1.0"

[dependencies]
colors       = { git = "https://github.com/acme/que-colors", tag = "v1.4.0" }
deploy-tools = { git = "git@github.com:acme/deploy-tools", rev = "9f2c1ab" }
nightly      = { git = "https://github.com/acme/nightly", branch = "main" }
shared       = { path = "../shared" }
helpers      = "https://github.com/acme/helpers#v2"   # shorthand: url#ref
que-std      = { git = "https://github.com/jasal82/que-lang", tag = "1.1.0", subdir = "stdlib" }
```

```sh
que install              # fetch dependencies, write que.lock
que install --locked     # fail instead of resolving anything not already pinned
```

`que install` fetches each dependency into `que_packages/<name>/` (hyphens
become underscores, matching what `import` looks for) and writes `que.lock`:

```toml
[[package]]
name = "colors"
source = "https://github.com/acme/que-colors"
requirement = "v1.4.0"
revision = "3f1d0b8a…"
```

**Commit `que.lock`.** It records the exact commit each dependency resolved
to, and it is the only thing that makes a fresh checkout get the same code
you tested. A tag can be moved; a commit id cannot. Once a dependency is
locked, `que install` reuses the pin instead of resolving again — changing
the `git` URL or the `rev`/`tag`/`branch` in `que.toml` is what asks for a
new resolution.

Use `--locked` in CI. It refuses to resolve anything the lockfile does not
already pin, so a build cannot quietly pick up code nobody reviewed.

A `path` dependency is symlinked rather than copied, so edits in the other
directory take effect immediately. That is the point of one, and a copy would
silently go stale.

Fetching shells out to `git`, so ssh keys, credential helpers, proxies and
whatever else your organisation uses already work.

Add `que_packages/` to `.gitignore`. It is a cache that `que install`
rebuilds from `que.lock`; committing it means reviewing other people's
diffs forever.

### Packages That Are Not at a Repository Root (`subdir`)

A repository does not have to *be* a package. `subdir` names the directory
inside it that is one, which is how a single repository ships several
packages, or ships one next to unrelated code:

```toml
[dependencies]
que-std = { git = "https://github.com/jasal82/que-lang", tag = "1.1.0", subdir = "stdlib" }
```

```que
import que_std.colors { colored, Color }
import que_std.select { select }
```

The repository is cloned once into `que_packages/.sources/<name>/`, and
`que_packages/<name>/` points at the sub-directory inside it — so the import
side sees a normal package and never the code around it. `.sources` starts
with a dot, which no import path can spell, so nothing there is importable by
accident.

`subdir` must stay inside the checkout: an absolute path or one containing
`..` is rejected rather than allowed to reach outside `que_packages/`. It
applies to `git` dependencies only — a `path` dependency can simply point at
the sub-directory itself.

### The `que-std` Package

Most of the standard library is built into the interpreter and needs no
installing (`import std.fs`). The parts written in Que rather than Rust ship
as the `que-std` package in the [`stdlib/`](stdlib) directory of the Que
repository, installed with the `subdir` dependency above:

| Module | What it does |
| --- | --- |
| `que_std.colors` | ANSI colors and text styles, honouring `NO_COLOR` and non-ANSI terminals |
| `que_std.select` | An interactive single-choice menu, with a plain numbered fallback when there is no terminal |

The package's `mod.que` re-exports both, so `import que_std` and
`import que_std.colors` are equally good ways in.

To use it from your global Quefile, install it beside that file rather than
inside a project — see [Dependencies for the global
Quefile](#dependencies-for-the-global-quefile).

---

# Part II — DevOps-Native Types

Everything so far would be at home in any modern scripting language. What
makes Que specifically good for build automation and DevOps starts here:
typed paths that know the difference between a file and a glob pattern,
durations you can do arithmetic on, commands that fail loudly by default, and
secrets the runtime actively hides from you.

## 13. Paths

### Creating Paths

There are two ways to create a `Path` value:

**Path literals** — the preferred form for static paths. The prefix `p` signals
the lexer to produce a `Path` directly:

```que
let bin   = p"/usr/local/bin"    // absolute
let cfg   = p"./config"          // relative — current directory
let logs  = p"../logs"           // relative — parent directory
let empty = p""                  // empty path (identity for /)
```

Path literals support `${...}` interpolation. Interpolated values may be `Path`
or `String` — they are joined as path segments, not concatenated as raw strings:

```que
let appname = "myapp"
let version = "1.4.2"

let install = p"/opt/${appname}"              // → p"/opt/myapp"
let lib     = p"/opt/${appname}/lib/${version}"  // → p"/opt/myapp/lib/1.4.2"
```

The lexer normalises consecutive slashes: `p"/a//b"` → `p"/a/b"`.

A leading `~` is expanded to the home directory:

```que
let key = p"~/.ssh/id_ed25519"                 // → /home/you/.ssh/id_ed25519
```

**`path()` builtin** — converts a runtime string to a `Path`. Use it when the
text arrives from a config file, an argument or an environment variable; use
`p"..."` for a path you can write down. Both expand `~` the same way:

```que
let config  = path(env.get("CONFIG") ?? "~/.config/que")
let ssh_key = path.home() / ".ssh" / "id_ed25519"
```

**`script_dir()`** — the directory the running script lives in, regardless of
where it was invoked from. Relative paths are resolved against the *current*
directory, which is wherever the user happened to be, so anything a script
ships alongside itself should be reached through `script_dir()`:

```que
let template = script_dir() / "templates" / "nginx.conf"
let root     = script_dir() / ".."
```

Without it, `./templates/nginx.conf` finds the file when you run
`que deploy.que` from the project root and fails when you run
`que scripts/deploy.que` from anywhere else. `quefile_dir()` is the same
function under the name that reads better in a Quefile.

### Composing Paths with `/`

The `/` operator is the primary way to build longer paths. It is left-associative
and always produces a new `Path`:

```que
let root = p"/opt/myapp"
let bin  = root / "bin" / "myapp"      // → p"/opt/myapp/bin/myapp"
let cfg  = root / p"etc/app.toml"      // → p"/opt/myapp/etc/app.toml"
```

**Rules for `/`:**

| Left | Right | Result | Notes |
|------|-------|--------|-------|
| `Path` | `Path` | `Path` | Absolute right side replaces the left |
| `Path` | `String` | `Path` | String is *always* a relative segment |
| `Path` | `Glob` | **error** | Use `glob(path) / pattern` instead |

```que
// Absolute Path on the right wins
p"/a/b" / p"/etc"   // → p"/etc"

// String is never absolute — its leading slash is stripped
p"/a/b" / "/etc"    // → p"/a/b/etc"
```

### Decomposing Paths

```que
let p = p"/opt/myapp/config.toml"

p.name()            // "config.toml"  — last segment
p.stem()            // "config"       — name without extension
p.extension()       // "toml"         — extension without dot
p.ext_dot()         // ".toml"        — extension with dot
p.parent()          // p"/opt/myapp"  — parent directory
p.root()            // p"/"           — root (p"" if relative)
p.is_absolute()     // true
p.is_relative()     // false
p.components()      // ["/", "opt", "myapp", "config.toml"]
p.depth()           // 4
```

Edge cases: `p"/".parent()` → `p"/"`, `p"".parent()` → `p""`.

### Transforming Paths (pure, no I/O)

```que
let p = p"/opt/myapp/config.toml"

p.with_name("out.log")       // p"/opt/myapp/out.log"
p.with_stem("settings")      // p"/opt/myapp/settings.toml"
p.with_ext(".bak")           // p"/opt/myapp/config.bak"
p.with_ext("bak")            // same — leading dot is optional
p.normalize()                // resolves . and .., collapses double slashes
p.relative_to(p"/opt/myapp") // p"config.toml"
p.resolve_or(p"/etc/defaults") // absolute form of p, or the fallback if p is empty
```

Converting a path back to a plain string:

```que
let p = p"/usr/local/bin"
p.to_string()            // "/usr/local/bin"  — explicit
"binary is at: ${p}"     // automatic in string interpolation
```

### Checking Path Properties (I/O)

```que
let p = p"/opt/myapp/config.toml"

p.exists()      // bool — file or directory exists
p.is_file()     // bool — exists and is a regular file
p.is_dir()      // bool — exists and is a directory
p.is_link()     // bool — exists and is a symbolic link
p.size()        // Int  — file size in bytes
p.modified()    // Int  — last modification time (ms since epoch)
```

### File Operations on Paths

```que
// Read and write
let content = p"data.txt".read()?
p"output.txt".write_text("hello")?
p"log.txt".append_text("new line\n")?

// Create directories and manage files
p"./build".mkdir()?
p"./config.json".copy_to(p"./backup/config.json")?
p"./build/output".move_to(p"/mnt/deploy/output")?
p"./old".delete()?

// Idempotent variants, for setup steps that may run twice
p"./build".mkdir({ clean: true })?      // empty afterwards, existed or not
p"./old".delete({ missing_ok: true })?  // an absent path is not an error

// List directory contents — paths are directly iterable
for f in p"./src" {
    println(f.name())
}

// Or use .ls() explicitly
let entries = p"./src".ls()
println(entries.len())

// .ls() takes an optional glob pattern to filter entries by file name
let configs = p"./etc".ls("*.toml")
```

### Recursive Tree Walking

```que
let src = p"./src"

// All descendants (files and directories)
let everything = src.walk()

// Only files, recursively
let all_files = src.files()

// Only directories, recursively
let all_dirs = src.dirs()

// Find all .rs files using a pipeline
let rust_files = src.files().filter(|f| f.extension() == "rs")
for f in rust_files {
    println(f)
}
```

### Temp Files and Directories

Use `TempDir` and `TempFile` — built-in structs implementing the `Contextual`
trait (see [Section 9](#9-structs-impl-blocks-and-traits)) — for temporary
resources that are automatically cleaned up:

```que
// Create a temp directory — deleted when block exits
with TempDir {} as work {
    work.join("data.txt").write_text("hello").unwrap()
    println(work.join("data.txt").read().unwrap())   // hello
}
// work directory no longer exists

// Create a temp file — deleted when block exits
with TempFile { suffix: ".json" } as scratch {
    scratch.write_text("{\"ok\": true}").unwrap()
    println(scratch.read().unwrap())
}
```

Both accept a `prefix`, and `dir` to choose the directory they are created in:

```que
with TempDir { prefix: "build_", dir: p"./.cache" } as work { ... }
with TempFile { suffix: ".part", dir: p"/mnt/data" } as scratch { ... }
```

`dir` is worth reaching for more often than it looks. The system temp
directory is frequently a tmpfs too small for a build artefact, is often on a
different filesystem than where the result is going — which turns the atomic
rename a temp file exists for into a plain copy — and may be swept while a long
job is still running.

`fs.temp_dir()` and `fs.temp_file()` take the same option as a trailing map,
for temporaries you want to clean up yourself:

```que
import std.fs
let stage = fs.temp_dir("stage_", { dir: p"./.cache" }).unwrap()
let part  = fs.temp_file("part_", ".bin", { dir: p"./.cache" }).unwrap()
```

Names are unpredictable rather than sequential, and creation fails rather than
reuses an existing entry — the system temp directory is world-writable, and a
name someone else can guess is a name they can create first, as a symlink
pointing somewhere you did not intend to write.

The `with ... as` block is an expression — it returns the value of its body:

```que
let data = with TempDir {} as tmp {
    tmp.join("work.txt").write_text("processed").unwrap()
    tmp.join("work.txt").read().unwrap()
}
println(data)   // processed (tmp is already cleaned up)
```

### Path Method Reference

| Method | Returns | Description |
|--------|---------|-------------|
| `.name()` | `String` | File name with extension |
| `.stem()` | `String` | File name without extension |
| `.extension()` | `String` | Extension without leading dot |
| `.ext_dot()` | `String` | Extension with leading dot (e.g. `".toml"`) |
| `.parent()` | `Path` | Parent directory; root and empty paths return themselves |
| `.root()` | `Path` | `p"/"` if absolute, `p""` if relative |
| `.is_absolute()` | `Bool` | True if path is absolute |
| `.is_relative()` | `Bool` | True if path is relative |
| `.is_link()` | `Bool` | True if path is a symbolic link |
| `.components()` | `List<String>` | All path segments including root |
| `.depth()` | `Int` | Number of path components |
| `.with_name(name)` | `Path` | Replace file name |
| `.with_stem(stem)` | `Path` | Replace stem, keep extension |
| `.with_ext(ext)` | `Path` | Replace extension (dot optional) |
| `.normalize()` | `Path` | Resolve `.` and `..` lexically |
| `.resolve()` | `Path` | Resolve to absolute path |
| `.resolve_or(fallback)` | `Path` | Resolve to absolute path, or `fallback` if this path is empty |
| `.relative_to(base)` | `Path` | Path relative to base |
| `.to_string()` | `String` | Convert to plain string |
| `.exists()` | `Bool` | Exists on disk |
| `.is_file()` | `Bool` | Exists and is a regular file |
| `.is_dir()` | `Bool` | Exists and is a directory |
| `.size()` | `Int` | File size in bytes |
| `.modified()` | `Int` | Last modification time (ms since epoch) |
| `.read()` | `Result<String>` | Read file contents |
| `.write_text(s)` | `Result` | Write string to file |
| `.append_text(s)` | `Result` | Append string to file |
| `.mkdir(opts?)` | `Result` | Create directory recursively. `{ clean: true }` removes an existing path first |
| `.delete(opts?)` | `Result` | Remove file, directory or symlink. `{ missing_ok: true }` treats an absent path as success |
| `.copy_to(dest)` | `Result` | Copy to destination |
| `.move_to(dest)` | `Result` | Move / rename |
| `.symlink(target)` | `Result` | Create *this* path as a symlink pointing at `target` (Unix) |
| `.ls(pattern?)` | `List<Path>` | List directory contents, optionally filtered by a glob pattern on the file name |
| `.walk()` | `List<Path>` | All descendants recursively |
| `.files()` | `List<Path>` | All files recursively |
| `.dirs()` | `List<Path>` | All directories recursively |
| `.glob(pattern)` | `List<Path>` | Expand glob relative to this directory |
| `.join(part)` | `Path` | Append a path segment |

---

## 14. Globs

`Glob` is a distinct type for filesystem patterns — it is not a `Path` and not a
`String`. The distinction matters: functions that expect a concrete `Path` reject
a `Glob` at runtime, preventing accidental expansion when you meant a specific file.

### Creating Globs

**Glob literals** — the preferred form for static patterns. The prefix `g`
produces a `Glob` value directly:

```que
let logs     = g"/tmp/*.log"                  // all .log files in /tmp
let all_logs = g"/var/log/**/*.log"           // recursive
let configs  = g"/etc/{dev,prod}/*.toml"      // alternation
let backups  = g"/backup/[0-9][0-9][0-9][0-9]" // character class
```

Glob literals support `${...}` interpolation (same as path literals). Bare
`{a,b}` is literal glob alternation syntax — not interpolation:

```que
let app = "myapp"
let pattern = g"/etc/${app}/*.conf"   // → g"/etc/myapp/*.conf"
```

**`glob()` builtin** — for runtime-constructed patterns:

```que
let g = glob("src/**/*.rs")      // from a string
let g = glob(p"./src") / "**" / "*.rs"   // from a Path
```

### Wildcard Syntax

| Pattern | Matches |
|---------|---------|
| `*` | Any characters within a single path segment (no `/` crossing) |
| `**` | Any characters across directory boundaries (recursive) |
| `?` | Any single character (no `/`) |
| `[abc]` | One of the listed characters |
| `[!abc]` | None of the listed characters |
| `[a-z]` | Character in range |
| `{a,b,c}` | Alternation — matches `a`, `b`, or `c` |

`**` must be a complete path segment: `foo/**` is valid; `a**b` is not.

### Composing Globs with `/`

Glob supports the same `/` operator as `Path`. The result is always a `Glob`:

```que
let base = g"/var/log"
let logs = base / "**" / "*.log"   // → g"/var/log/**/*.log"

// Convert a Path to a Glob first, then attach a pattern:
let dir  = p"/var/log/myapp"
let pattern = glob(dir) / "**" / "*.log"   // → g"/var/log/myapp/**/*.log"
```

`Path / Glob` is a type error — the direction must always go `Glob / …`:

```que
p"/var/log" / g"*.log"     // runtime error
glob(p"/var/log") / "*.log" // correct
```

| Left | Right | Result |
|------|-------|--------|
| `Glob` | `String` | `Glob` — string is always relative |
| `Glob` | `Path` | `Glob` — absolute Path replaces left |
| `Glob` | `Glob` | `Glob` — absolute Glob replaces left |
| `Path` | `Glob` | **error** |

### Expanding Globs (lazy)

A glob literal creates a `Glob` object — **no filesystem access at construction
time**. Expansion happens only when you iterate or call an expansion method:

```que
let g = g"/var/log/**/*.log"    // no I/O

// Iteration expands lazily
for file in g {                 // I/O here — file is a Path
    println(file)
}
```

**Quick checks without full expansion:**

```que
let pattern = g"/var/log/**/*.log"

pattern.any()       // Bool — at least one match?
pattern.count()     // Int  — total match count
pattern.first()     // Path — first match, or null
pattern.pattern()   // String — the raw pattern string
```

**Full expansion:**

```que
// Explicit expansion
let files = g"src/**/*.rs".expand()
println(files)   // [src/ast.rs, src/lexer.rs, ...]

// Globs are iterable — expand automatically in for loops
for file in g"src/*.rs" {
    println(file)
}

// Expand relative to a directory
let rust_files = p"./src".glob("*.rs")
```

Every one of these spellings — `.expand()`, a `for` loop, `.first()`,
`.count()`, `.any()`, `.copy_to()`, `Path.glob()` and a glob in `@inputs` —
expands the pattern the same way: `~` becomes the home directory and `{a,b}`
is expanded into its alternatives before matching.

```que
p"~/projects".glob("{web,api}/*.toml")   // same matches as
g"~/projects/{web,api}/*.toml".expand()  // this
```

**Combining with pipelines:**

```que
let large_files = g"build/**/*"
    .expand()
    .filter(|f| f.is_file() && f.size() > 1_000_000)
    .sort()

for f in large_files {
    println("${f}: ${f.size()} bytes")
}
```

**Testing a path against a pattern** (no filesystem access):

```que
let pattern = glob("src/**/*.rs")
println(pattern.test("src/main.rs"))         // true
println(pattern.test("src/nested/lib.rs"))   // true
println(pattern.test("README.md"))           // false
```

**Batch transfer:** `.copy_to(dest)` and `.move_to(dest)` expand the glob and
transfer every match into a destination directory in one call:

```que
g"build/**/*.log".copy_to(p"./archived-logs")
g"/tmp/uploads/*.tmp".move_to(p"./incoming")
```

### Glob Method Reference

| Method | Returns | Description |
|--------|---------|-------------|
| `.expand()` | `List<Path>` | All matching paths |
| `.first()` | `Path \| Null` | First match, or `null` |
| `.count()` | `Int` | Number of matches |
| `.any()` | `Bool` | True if at least one match |
| `.pattern()` | `String` | The raw glob pattern string |
| `.test(path)` | `Bool` | Check if a path matches (no filesystem access) |
| `.copy_to(dest)` | `Result` | Copy every matched file into directory `dest` |
| `.move_to(dest)` | `Result` | Move every matched file into directory `dest` |

---

## 15. Durations and Semantic Versioning

### Durations

Duration literals use units directly:

```que
let timeout = 30s
let retry_delay = 500ms

let total = timeout + retry_delay
println(total.to_seconds())   // 30.5
```

Supported units: `ms` (milliseconds), `s` (seconds), `m` (minutes), `h`
(hours), `d` (days).

Durations support arithmetic and comparisons:

```que
let a = 10s
let b = 3s
println((a - b).to_seconds())   // 7.0
println((a * 3).to_seconds())   // 30.0
println(a > b)                  // true
```

#### Deadline Calculations with `time.timestamp()`

`time.timestamp()` returns the current Unix timestamp in milliseconds. Adding or
subtracting a Duration enables natural deadline arithmetic:

```que
import std.time

let deadline = time.timestamp() + 24h
let one_week_ago = time.timestamp() - 7d

// Check whether a cached file is fresh
let file_age_ms = time.timestamp() - path("cache.json").modified()
if file_age_ms > (1h).to_millis() {
    println("cache is stale, refreshing...")
}
```

| Method | Description |
|--------|-------------|
| `.to_millis()` | Convert to milliseconds |
| `.to_seconds()` | Convert to seconds |
| `.to_minutes()` | Convert to minutes |
| `.to_hours()` | Convert to hours |

### Semantic Versioning

Semver literals are created with `v"..."`:

```que
let current = v"1.2.3"
let minimum = v"1.0.0"

println(current > minimum)    // true
println(current.major)        // 1
println(current.minor)        // 2
println(current.patch)        // 3
```

Parse semver strings at runtime:

```que
let result = semver_parse("2.0.0")
match result {
    Ok(ver) => println(ver.major),   // 2
    Err(e) => println("invalid: " + e),
}
```

| Field / Method | Description |
|----------------|-------------|
| `.major` | Major version number |
| `.minor` | Minor version number |
| `.patch` | Patch version number |
| `.prerelease` | Prerelease string (if any) |
| `.is_prerelease()` | Check if it has a prerelease tag |
| `.bump_major()` | Return version with major incremented |
| `.bump_minor()` | Return version with minor incremented |
| `.bump_patch()` | Return version with patch incremented |

**Constraint matching** — `.satisfied_by(version)` checks a version against a
range. The receiver is a `v"..."` literal holding the constraint expression
(npm/cargo-style range syntax, not a single version), and the argument is the
`Semver` to check against it:

```que
let constraint = v">=1.2.0, <2.0.0"
println(constraint.satisfied_by(v"1.3.0"))   // true
println(constraint.satisfied_by(v"2.0.0"))   // false
```

---

## 16. Regex and Secrets

### Regex

Create regex literals with `re"..."`:

```que
let pattern = re"^\d{3}-\d{4}$"
println(pattern.test("555-1234"))   // true
println(pattern.test("abc"))        // false
```

For a pattern that isn't known until runtime, `regex(str)` parses a string
into a `Regex`, returning a `Result` so an invalid pattern is a value, not a
crash:

```que
let pattern = regex(user_supplied_pattern)?
println(pattern.test("hello"))
```

| Method | Description |
|--------|-------------|
| `.test(str)` | Check if string matches |
| `.find(str)` | First match or `null` |
| `.find_all(str)` | List of all matches |
| `.captures(str)` | Capture groups as list |
| `.named_captures(str)` | Named capture groups as a `Map<String, String>` |
| `.replace(str, replacement)` | Replace first match |
| `.replace_all(str, replacement)` | Replace every match |
| `.split(str)` | Split string by pattern |

### Secrets

A `Secret` is a string the runtime refuses to show you.

```que
let token = secret("super-secret-token")
println(token)          // <redacted>
```

**Read secrets from where they live.** `env.secret()` and `fs.read_secret()`
exist because `env.get()` and `fs.read()` return a plain `String` that nothing
can track:

```que
let token = env.secret("API_TOKEN").unwrap()          // Ok(Secret) | Err(String)
let db = fs.read_secret(p"/run/secrets/db_password").unwrap()
```

`fs.read_secret` strips a trailing newline — it is an artefact of how the file
was written, not part of the token, and sending it in an `Authorization` header
is a long debugging session.

**Interpolate them into commands directly.** The process gets the real token;
every rendering meant for a human gets `<redacted>`:

```que
`curl -H "Authorization: Bearer ${token}" https://api.example.com`
```

```console
$ que deploy.que --dry-run
[dry-run] curl -H "Authorization: Bearer <redacted>" https://api.example.com
```

The same applies to `.arg(token)`, `.flag("--token", token)` and
`` `...`.to_string() ``.

**Where secrets are hidden:**

| Surface | Result |
| --- | --- |
| `println`, `print`, `log.*` | `<redacted>` |
| `--dry-run` command echo | `<redacted>` |
| A pipeline stage's failure message | `<redacted>` |
| `json.stringify`, `yaml.stringify`, `toml` writes | `"<redacted>"` |
| Anything printed that *contains* the plaintext | `<redacted>` |

That last row is the one that matters. Redacting the `Secret` type is not
enough, because `.expose()` produces an ordinary `String` that no type can
catch. Every secret the run has seen is also scrubbed out of printed text by
value:

```que
let token = secret("hunter2")
println("token is " + token.expose())   // token is <redacted>
println(`echo $CI_TOKEN`.out())         // <redacted>, if the values match
```

**Where they are not.** Values a script computes with are left alone —
scrubbing captured stdout would corrupt a script that legitimately reads a
token out of a command. `.expose()` returns the real string, and writing it to
a file is a thing you can do; it just has to be a thing you *chose* to do.

Secrets shorter than four characters are not registered with the scrubber:
they would turn output into confetti, and are not secrets worth protecting.

---

## 17. Running Commands

Que has first-class command execution using backtick literals.

### Running Commands

A backtick literal written as a **statement** runs immediately, streams its
output to the terminal, and **raises if the command fails**:

```que
`cargo build --release`     // runs; raises on non-zero exit
println("build succeeded")  // only reached if the build worked
```

This is the important default: a failing command stops the script instead of
being silently ignored.

Binding a command to a name keeps it lazy, so you can build it up before
running it:

```que
let cmd = `echo hello`
println(typeof(cmd))   // Cmd
```

### The Three Execution Forms

| Form | Output | On failure |
|------|--------|-----------|
| `` `cmd` `` or `` `cmd`.run() `` | streamed to the terminal | **raises** |
| `` `cmd`.out() `` | returned as a trimmed `String` | **raises** |
| `` `cmd`.try() `` | captured in a `ProcessResult` | returns normally |

```que
// Streamed, checked — the everyday case
`npm ci`

// Capture output you need
let branch = `git rev-parse --abbrev-ref HEAD`.out()

// Handle failure yourself
let probe = `which docker`.try()
if probe.exit_code != 0 {
    println("docker is not installed")
}
```

`.run_checked()` and `.capture()` were removed; `.run()` is now checked and
`.out()` replaces `.capture()`.

### ProcessResult

`.run()` and `.try()` return a `ProcessResult`:

| Field / Method | Description |
|----------------|-------------|
| `.stdout` | Standard output as string |
| `.stderr` | Standard error as string |
| `.exit_code` | Exit code as integer |
| `.success()` | `true` if exit code is 0 |
| `.ok()` | `Ok(stdout)` if success, `Err(stderr)` if not |
| `.lines()` | Stdout split into list of lines |
| `.trim()` | Stdout with leading/trailing whitespace removed |

### String Interpolation in Commands

Use `${}` for safe interpolation (values are shell-escaped):

```que
let tag = "myapp:latest"
println(`echo -n ${tag}`.out())   // myapp:latest
```

### Command Modifiers

Commands support a builder pattern. Each modifier returns a new `Cmd`:

```que
// Run in a specific directory
let cwd = `pwd`.dir(path("/tmp")).out()

// Set environment variables
let greeting = `echo -n $MY_VAR`
    .env("MY_VAR", "hello_que")
    .out()

// Pipe data into stdin
let echoed = `cat`.stdin("hello from stdin").out()

// Suppress stdout/stderr
`echo noisy`.silent()
```

| Modifier | Description |
|----------|-------------|
| `.dir(path)` | Set working directory |
| `.env(key, val)` | Set an environment variable |
| `.env_map(map)` | Set multiple env vars from a map |
| `.stdin(data)` | Pipe string data into stdin |
| `.silent()` | Suppress stdout and stderr |
| `.sudo(user?)` | Run with elevated privileges |
| `.timeout(duration)` | Set execution timeout |
| `.forward_stdout(stream)` | Forward stdout to a stream (stdout, file, …) |
| `.forward_stderr(stream)` | Forward stderr to a stream |
| `.arg(value)` | Append a single shell-escaped argument |
| `.flag(name, value?)` | Append a boolean flag, or `name value` if `value` is given |

`.arg()` and `.flag()` are useful for building a command up conditionally,
piece by piece, without hand-quoting each piece yourself:

```que
mut cmd = `kubectl apply`
if namespace != null { cmd = cmd.flag("--namespace", namespace) }
if dry_run() { cmd = cmd.flag("--dry-run", "client") }
cmd = cmd.flag("-f").arg(manifest_path)
cmd.run()
```

### Elevated Privileges

```que
`apt-get install -y nginx`.sudo().run()
`psql -c "select 1"`.sudo("postgres").out()
`make install`.sudo({ preserve_env: true, non_interactive: true }).run()
```

| Option | Default | Description |
|---|---|---|
| `user` | `root` | Run as this user (`sudo -u`) |
| `preserve_env` | `false` | Keep the current environment (`sudo -E`) |
| `non_interactive` | `false` | Fail instead of prompting for a password (`sudo -n`) |
| `binary` | `"sudo"` | Use `doas`, `run0`, or an absolute path instead |

`.sudo()` rewrites the command text rather than setting a hidden flag, so
`--dry-run`, `.to_string()` and every failure message show the elevation you
are actually getting.

**Que never handles your password.** `sudo` prompts on the terminal directly,
which is the only place a password should ever be typed. In CI, where nobody
is watching the prompt, pass `non_interactive: true` so the command fails
loudly instead of hanging until the job times out.

**Already root is a no-op.** If the process is running with uid 0, `.sudo()`
returns the command unchanged. This is what lets the same script work on a
laptop and inside a minimal container image that has no `sudo` binary
installed at all.

**Shell operators are refused.** Only the first word after `sudo` is
elevated, so this is a trap:

```que
`echo "127.0.0.1 db" > /etc/hosts`.sudo()   // error
```

The redirect would be performed by *your* shell, as *you*, and the permission
error would point at the wrong thing entirely. Que refuses it. Use a command
that does the whole job under elevation instead:

```que
`echo "127.0.0.1 db"` | `tee -a /etc/hosts`.sudo()
```

An operator inside quotes is fine — `` `grep "a|b" f`.sudo() `` is one
command and is accepted.

`env.is_root()` reports whether the process is already elevated, if you want
to branch on it yourself.

### Pipes and Redirection

A command literal is handed to `sh`, so ordinary shell plumbing works inside
the backticks — and `${}` is still escaped:

```que
let pattern = "connection refused"
let count = `grep -F ${pattern} app.log | wc -l`.out()

`./build.sh > build.log 2>&1`
`sort < names.txt`
```

To build a pipeline out of separate commands, use `|` between them:

```que
let logs = `cat app.log`
let errors = logs | `grep ERROR` | `wc -l`
println(errors.out())
```

Both forms run the same way. Reach for `|` when the stages are values you want
to keep, reuse, or assemble conditionally, and for shell text when you are just
writing a pipeline out.

Each stage keeps the modifiers written on it, exactly like separate processes
in a shell:

```que
let versions = `git log --oneline`.dir(repo) | `head -20`
```

**A pipeline fails if any stage fails,** not just the last one:

```que
// `sh` reports success here — `wc` succeeded. Que raises.
`cat missing-file.txt | wc -l`.out()      // shell text: shell rules apply
(`cat missing-file.txt` | `wc -l`).out()  // raises: exit code 1 from `cat`
```

That difference is the reason to prefer `|` for anything whose failure
matters. A pipeline whose first stage died but whose last one succeeded is the
classic shell footgun; `.try()` still gives you the `ProcessResult` if you want
to inspect it instead.

### Checking Tool Availability

`which(name)` searches the system `PATH` for an executable:

```que
let docker = which("docker")
if docker == null {
    fail("docker is required but not installed")
}
println("docker found at: ${docker}")
```

---

## 18. Background Processes and Parallel Execution

### Spawn

The `spawn` keyword launches a command in the background and immediately
returns a `ProcessHandle`:

```que
let server = spawn `npm run dev`

println("server started, pid: ${server.pid()}")

if server.is_alive() {
    println("server is up")
}

// Wait until it exits
let code = server.wait()

// Kill the process
server.kill()        // SIGTERM (graceful)
server.kill_force()  // SIGKILL (immediate)
```

| Method | Description |
|--------|-------------|
| `.pid()` | Process ID as `Int` |
| `.is_alive()` | `true` if still running |
| `.wait()` | Block until exit; returns exit code |
| `.kill()` | Send SIGTERM (graceful shutdown) |
| `.kill_force()` | Send SIGKILL (immediate kill) |

**Common pattern — start and cleanup:**

```que
let db = spawn `docker run --rm -p 5432:5432 postgres:15`
defer db.kill()

sleep(3s)

let test_result = `pytest integration/`.try()
if test_result.exit_code != 0 {
    fail("tests failed")
}
```

### Parallel Execution

The `parallel` keyword evaluates multiple expressions and returns their results
as a collection:

```que
// Unnamed branches: returns a Tuple
let base = 5
let (doubled, tripled, squared) = parallel {
    base * 2,
    base * 3,
    base ** 2
}

// Named branches: returns a Map
let results = parallel {
    coverage:  run_coverage_check(),
    lint:      run_linter(),
    typecheck: run_typechecker()
}

println("lint passed: ${results["lint"]}")
```

All branches must be either all named or all unnamed — mixing is not allowed.

**Branches run at the same time.** Each one gets its own thread, so three
one-minute jobs take one minute, not three:

```que
let results = parallel {
    unit:        `cargo test`.out(),
    integration: `cargo test --test integration`.out(),
    docs:        `cargo doc`.out(),
}
```

A few rules follow from that:

- **Branches see the surrounding variables** and can read them freely. Anything
  a branch declares itself stays inside that branch.
- **Output is replayed in source order.** A branch's `println` output is held
  until the whole block finishes, so the transcript reads top to bottom no
  matter which branch finished first. Command output still streams live unless
  you capture it.
- **The first failing branch in source order is the one that is raised**, even
  if a later branch failed first in wall-clock time. That keeps the error
  message the same on every run.
- **Avoid writing to the same shared variable from two branches.** Nothing will
  corrupt, but which write lands last is up to the scheduler. Have each branch
  return a value instead — that is what the tuple and map are for.

### Sleep

Use `sleep` with a duration to pause execution:

```que
sleep(2s)
sleep(500ms)
```

---

## 19. Resilience — Retry and Timeout

### Retry

Retry a closure up to `n` times. The closure receives the attempt number
(0-indexed). If it succeeds, the result is returned. If all attempts fail, the
last error is raised:

```que
let result = retry(3, |attempt| {
    if attempt < 2 {
        fail("not ready yet")
    }
    "success on attempt " + str(attempt)
})
println(result)   // success on attempt 2
```

### Timeout

Run a closure with a time limit:

```que
let result = timeout(5s, || {
    42
})
println(result)   // 42
```

### Practical Pattern: Resilient HTTP

```que
import std.http { get }

let resp = retry(3, || {
    let r = get("https://api.example.com/health").unwrap()
    if !r.ok { fail("unhealthy: " + str(r.status)) }
    r
})
```

---

## 20. File I/O — Handles and Streams

### File Handles

For simple cases, use path methods (`path.read()`, `path.write_text()`). When
you need incremental reading, line-by-line iteration, or paired
setup/teardown, use `open()` to get a **FileHandle**.

```que
let f = open("data.txt")          // read mode (default)
let f = open("out.txt", "w")      // write / truncate
let f = open("log.txt", "a")      // append / create
```

| Mode | Effect |
|------|--------|
| `"r"` | Open for reading (file must exist) |
| `"w"` | Open for writing, truncating to zero (creates if absent) |
| `"a"` | Open for appending (creates if absent) |

Always close with `defer`:

```que
let f = open("report.txt")
defer f.close()

let content = f.read()
println(content)
```

**Reading:**

```que
let f = open("data.txt")
defer f.close()

let all = f.read()          // Ok(String) | Err(String)
let line1 = f.read_line()   // String | Null (Null at EOF)
let lines = f.lines()       // List<String>
```

**Writing:**

```que
let f = open("output.txt", "w")
defer f.close()
f.write("first line\n")     // no trailing newline added
f.writeln("second line")    // adds \n automatically
f.flush()                   // force OS flush
```

| Method | Modes | Description |
|--------|-------|-------------|
| `f.read()` | `r` | Read entire file |
| `f.read_line()` | `r` | Read next line (`Null` = EOF) |
| `f.lines()` | `r` | All lines as list |
| `f.write(s)` | `w`, `a` | Write bytes |
| `f.writeln(s)` | `w`, `a` | Write bytes + newline |
| `f.flush()` | `w`, `a` | Flush write buffer |
| `f.close()` | any | Close handle (idempotent) |
| `f.seek(pos)` | any | Seek to byte position |
| `f.is_open()` | any | Whether still open |
| `f.path()` | any | The path opened |

### Streams

Que has a built-in **Stream** type for text processing pipelines. The
constructors live in `std.stream`:

```que
import std.stream
```

Streams are **lazy and line-streaming**:

- A stream has a *source* (buffer, file, or stdin), a queue of *transformations*,
  and an optional *sink* (stdout, stderr, file, or file handle).
- Chainable methods (`to_upper`, `grep`, `map`, …) only append a transformation —
  no I/O, no allocation of the full content.
- The pipeline runs only when you call a **terminal** method (`collect`, `write`,
  `lines`, `count_lines`, `parse_json`, …). At that point the source is read
  line-by-line through a `BufReader`, ops are applied per line, and output is
  emitted incrementally. Large files never need to fit in memory.
- `.head(n)` short-circuits as soon as it has its lines, so
  `stream.file(big_file).grep(...).head(3)` stops reading after the third match.

A small subset of ops require buffering the whole text and force materialization
at that step: `.trim()`, `.sort_lines()`, `.reverse_lines()`, `.tail(n)`,
`.unique_lines()` (effectively), `.join_lines()`, `.prepend()`, `.append()`, and
`.replace()` when the pattern contains a newline. Put these as late in the
pipeline as possible — anything before them still streams.

**Creating streams:**

```que
let s = stream.file(path("./data/input.txt"))  // lazy file source — not read yet
let s = stream.of("hello\nworld")             // from a string
let s = stream.of(["alice", "bob"])           // from a list (joined with newlines)
let s = stream.of(file_handle)                // from an open FileHandle (writable sink)

// Standard I/O streams
let out = stream.stdout()   // stream wired to process stdout
let err = stream.stderr()   // stream wired to process stderr
let inp = stream.stdin()    // stream that lazily reads stdin
```

**Method chaining:**

```que
let result = stream.of("  hello world  ")
    .trim()
    .to_upper()
    .collect()

println(result)   // HELLO WORLD
```

**Reading, transforming, and writing a file** — streamed line-by-line:

```que
stream.file(path("./input.txt"))
    .to_upper()
    .write_to(path("./output.txt"))
```

**Writing to standard output directly:**

```que
// Print the last 20 ERROR lines straight to stdout — file is read lazily,
// output is written line-by-line, no full file in memory.
stream.file(path("./log.txt"))
    .grep("ERROR")
    .tail(20)
    .write_to(stream.stdout())
```

**Forwarding command output to a stream:**

Use `.forward_stdout(stream)` and `.forward_stderr(stream)` on a `Cmd` to live-
forward child process output to any stream sink (stdout, stderr, or a file):

```que
// Mirror the child's stdout directly to our stdout
`cargo build --release`.forward_stdout(stream.stdout()).run()

// Capture stderr to a file while printing stdout normally
let build_log = open("build.log", "w")
defer build_log.close()

`make all`
    .forward_stdout(stream.stdout())
    .forward_stderr(stream.of(build_log))
    .run()
```

**Text transformations:**

| Method | Streaming? | Description |
|--------|------------|-------------|
| `.to_upper()` | yes | Convert to uppercase |
| `.to_lower()` | yes | Convert to lowercase |
| `.replace(from, to)` | yes¹ | Replace all occurrences |
| `.trim()` | buffers | Remove leading/trailing whitespace |
| `.prepend(text)` | buffers | Add text before content |
| `.append(text)` | buffers | Add text after content |

¹ Streams per-line unless `from` contains a newline, in which case it buffers.

**Line-oriented operations:**

| Method | Streaming? | Description |
|--------|------------|-------------|
| `.map(fn)` | yes | Transform each line |
| `.filter(fn)` | yes | Keep lines where `fn` returns true |
| `.grep(pattern)` | yes | Keep lines matching a regex or substring |
| `.head(n)` | yes (short-circuits) | First `n` lines |
| `.skip_empty()` | yes | Remove blank lines |
| `.enumerate_lines()` | yes | Prefix each line with `1\t`, `2\t`, ... |
| `.unique_lines()` | yes² | Remove duplicate lines |
| `.tail(n)` | buffers | Last `n` lines |
| `.sort_lines()` | buffers | Sort lines alphabetically |
| `.reverse_lines()` | buffers | Reverse line order |
| `.join_lines(sep)` | buffers | Join all lines with separator |

² Streaming, but memory grows with the number of distinct lines seen so far.

**Terminal methods** — consume the stream and return a value:

| Method | Returns | Description |
|--------|---------|-------------|
| `.collect()` | `String` | Get content as string |
| `.write_to(dest)` | `Ok(Stream)` | Write to a path, another stream, or stdout/stderr |
| `.append_to(dest)` | `Ok(Stream)` | Append to a path or stream |
| `.lines()` | `List<String>` | Split into list |
| `.split(sep)` | `List<String>` | Split content on an arbitrary separator |
| `.count_lines()` | `Int` | Number of lines |
| `.is_empty()` | `Bool` | Check if empty |
| `.contains(s)` | `Bool` | Check if contains substring |
| `.parse_json()` | `Result` | Materialize and parse as JSON |
| `.parse_yaml()` | `Result` | Materialize and parse as YAML |
| `.parse_toml()` | `Result` | Materialize and parse as TOML |

**Realistic example:**

```que
// Find errors in a log file
stream.file(path("/var/log/app.log"))
    .grep("ERROR")
    .tail(10)
    .write_to(path("./recent-errors.txt"))

// Filter and transform lines
let admins = stream.file(path("./users.csv"))
    .filter(|line| line.contains("admin"))
    .map(|line| line.split(",")[0])
    .to_upper()
    .collect()
```

---

## 21. Environment and Platform

### Reading Environment Variables

`env` is a **namespace object**, not a function. Every operation is a method on
it, so there is exactly one spelling for each:

```que
let home = env.get("HOME")            // string or null
let port = env.get("PORT", "8080")    // with a default
let present = env.has("CI")           // true / false
println(home)   // /home/user
```

### Typed Environment Access

```que
let debug = env.bool("DEBUG")       // true / false, or null if unset
let port = env.int("PORT")          // 8080, or null if unset
let tags = env.list("TAGS")         // ["a", "b"], or null  (splits on ",")
```

`env.list()` splits on `,` by default. Pass a second argument for a different
separator:

```que
let dirs = env.list("PATH", os.path_separator)
// ["/usr/local/bin", "/usr/bin", "/bin", ...]
```

### Setting Environment Variables

```que
env.set("CARGO_TERM_COLOR", "always")
env.set("NODE_ENV", "production")

println(env.get("NODE_ENV"))   // production

env.unset("NODE_ENV")
println(env.has("NODE_ENV"))   // false
```

`env.load_file(path)` parses a dotenv-style file (`KEY=value` per line,
`#`-comments and blank lines skipped) directly into the process environment —
a lighter-weight alternative to `std.dotenv` ([Section 26](#26-logging-git-dotenv-templates-and-networking))
when you don't need its quoting rules or the ability to parse without loading:

```que
env.load_file(".env")?
```

### Requiring Variables

Fail fast if required variables are missing:

```que
env.require(["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"])
// Error: required environment variables not set: AWS_REGION
```

### All Environment Variables

```que
let all = env.all()
println("PATH: ${all["PATH"]}")
```

### CI Detection

```que
if env.is_ci() {
    println("running in CI: ${env.ci_name()}")  // "github-actions", "jenkins", etc.
}

if env.is_interactive() {
    // show prompts, progress bars, etc.
}

let platform = env.platform()  // "linux", "macos", "windows"
```

### The `os` Object

Platform information through the built-in `os` struct:

```que
println(os.name)             // "linux", "macos", "windows"
println(os.family)           // "unix" or "windows"
println(os.arch)             // "x86_64", "aarch64", etc.
println(os.path_separator)   // ":" on Unix, ";" on Windows
println(os.dir_separator)    // "/" on Unix, "\" on Windows
```

Use `os.exit(code)` to stop execution immediately:

```que
if !checks_passed {
    println("checks failed")
    os.exit(1)
}
```

### Running on Windows

Que builds and runs on Windows, and the pieces that differ between platforms
are handled rather than assumed away:

| Behaviour | Unix | Windows |
| --- | --- | --- |
| Command literals | `sh -c` | `cmd /C` |
| Argument quoting | `'single'` | `"double"`, `%` doubled |
| `~` expansion, `path.home()` | `$HOME` | `$HOME`, then `USERPROFILE`, then `HOMEDRIVE`+`HOMEPATH` |
| `p"...".symlink()` | always | needs Developer Mode; the error says so |
| Local timezone | `/etc/timezone`, `/etc/localtime` | matched from the OS's UTC offset |

Set `QUE_SHELL` to override the shell. That is the flag to reach for under Git
Bash, MSYS or WSL, where a POSIX shell is present and is what your command
literals were written for:

```
set QUE_SHELL=C:\Program Files\Git\bin\bash.exe
```

A shell name ending in `cmd` or `cmd.exe` selects `/C` and cmd-style quoting;
anything else gets `-c` and POSIX quoting.

Two honest caveats. The contents of a backtick literal are still shell text,
so `` `rm -rf build` `` is not portable however carefully Que quotes it — use
`std.fs` for filesystem work you want to run everywhere, and keep commands for
tools that exist on both. And Ctrl-C unwinding, which runs your `defer` blocks
on the way out, is Unix-only; on Windows an interrupt terminates the process
directly.

Branch on `os.family` when a command genuinely differs:

```que
let open = if os.family == "windows" { "start" } else { "xdg-open" }
`${open} report.html`.run()
```

### Scoped Environment with `env.scope`

`env.scope(map)` returns a context manager. Use it with `with` to temporarily
set environment variables for a block — values are automatically restored when
the block exits:

```que
with env.scope({ "DATABASE_URL": "postgres://localhost/test" }) {
    let url = env.get("DATABASE_URL")
    println(url)   // postgres://localhost/test
}
// DATABASE_URL is restored to its previous value (or unset)
```

The `as NAME` part of `with` is optional — omit it when you do not need the
entered resource, as here.

| Method | Description |
|--------|-------------|
| `env.get(key, default?)` | Read as string, or `default` / `null` if unset |
| `env.secret(key)` | Read as `Ok(Secret)`, or `Err(String)` if unset |
| `env.has(key)` | `true` if the variable is set |
| `env.bool(key)` | Parse as `Bool`, or `null` if unset |
| `env.int(key)` | Parse as `Int`, or `null` if unset |
| `env.list(key, sep?)` | Split into a `List`, or `null` if unset |
| `env.set(key, value)` | Set a variable in the current process |
| `env.unset(key)` | Remove a variable from the current process |
| `env.load_file(path)` | Parse a dotenv-style file into the current process (returns `Result`) |
| `env.scope(map)` | Context manager for a scoped override |
| `env.require(keys)` | Fail if any variable is missing |
| `env.all()` | All variables as `Map<String, String>` |
| `env.is_ci()` | `true` if running in CI |
| `env.ci_name()` | CI provider name or `null` |
| `env.platform()` | OS name |
| `env.is_root()` | `true` if running with an effective uid of 0 |
| `env.is_interactive()` | `true` if stdout is a TTY |

---

## 22. Building Command-Line Apps

Que ships with a small set of primitives for turning scripts into proper CLI
applications: reading positional arguments, detecting whether output is a
terminal, and asking the user interactive questions.

Higher-level conveniences like colored output, argument parsers, progress bars,
and pagers are intentionally **not** built in — they can be implemented as pure
Que packages on top of these primitives.

### `args()` — Command-line arguments

The `args()` builtin returns the list of arguments passed after the script
name. An optional `--` separator can be used to make the boundary explicit:

```sh
que script.que one two three        # args() → ["one", "two", "three"]
que script.que -- --flag a b        # args() → ["--flag", "a", "b"]
```

```que
let a = args()
if a.len() < 1 {
    println("usage: greet.que <name>")
    os.exit(1)
}
println("hello,", a[0])
```

In the REPL, or when no arguments are given, `args()` returns an empty list.

### `std.tty` — Detect terminals and query size

```que
import std.tty

tty.is_stdin()    // true if stdin is a TTY (not piped)
tty.is_stdout()   // true if stdout is a TTY (not redirected)
tty.is_stderr()   // true if stderr is a TTY
tty.size()        // {"cols": Int, "rows": Int} or null
tty.supports_ansi()  // true if stdout interprets ANSI escape sequences
```

A typical use is to disable ANSI colors when output is being piped to a file:

```que
import std.tty

let use_color = tty.is_stdout()
fn red(s) { if use_color { "\e[31m" + s + "\e[0m" } else { s } }
println(red("error: file not found"))
```

`tty.size()` returns `null` when stdout isn't a terminal — useful for choosing
between a wide table and a compact list:

```que
import std.tty
let sz = tty.size()
let wide = sz != null && sz["cols"] >= 100
```

`tty.supports_ansi()` is the stronger check to make before *drawing* rather
than merely coloring — redrawing a menu in place needs the terminal to act on
cursor-movement and line-clearing escapes, not just to be a TTY. It ignores
`NO_COLOR`, which is a color policy and says nothing about cursor positioning,
and on Windows it switches the console into virtual-terminal mode, which a
legacy console stays out of until a program asks:

```que
import std.tty

if tty.supports_ansi() {
    println("working…")
    print("\e[1A\e[J")     // erase that line and redraw over it
    println("done")
} else {
    println("done")        // no escapes: append-only output
}
```

### `input()` and `confirm()` — quick one-liners

For the simplest case — read one line, or ask a yes/no question — two global
builtins skip the import entirely. Both write their prompt to **stderr**, so
piping a script's stdout to another program doesn't capture the prompt text:

```que
let name = input("name: ")            // reads a line from stdin
if confirm("proceed?") {              // "y" or "yes" (case-insensitive) → true
    println("continuing as ${name}")
}
```

Reach for `std.prompt` below when you need to hide input (passwords), read a
single keypress without Enter, or build a richer interaction like a menu.

### `std.prompt` — Input primitives

`std.prompt` only contains the two primitives that need OS-level terminal
support; everything else (line editors, confirm prompts, select menus,
password-with-confirmation, …) can be built on top in pure Que using ANSI
escape sequences via `print`.

#### `prompt.read_line(opts?)` — read one line

```que
import std.prompt

print("name: ")
let name = prompt.read_line()
```

Pass `{ "echo": false }` to hide characters as they're typed (for passwords):

```que
import std.prompt

print("password: ")
let pw = prompt.read_line({ "echo": false })
println()   // newline after hidden input
```

#### `prompt.read_key()` — read one key press in raw mode

Blocks until exactly one key is pressed (no Enter required) and returns its
name:

| Returned string | Key |
|-----------------|-----|
| `"a"`, `"1"`, `" "`, … | Printable character |
| `"up"`, `"down"`, `"left"`, `"right"` | Arrow keys |
| `"enter"`, `"escape"`, `"backspace"` | Editing keys |
| `"tab"`, `"backtab"`, `"delete"`, `"insert"` | Editing keys |
| `"home"`, `"end"`, `"pageup"`, `"pagedown"` | Navigation |
| `"ctrl+c"` | Ctrl-C |
| `"unknown"` | Anything else |

```que
import std.prompt

println("press any key…")
let k = prompt.read_key()
println("you pressed:", k)
```

#### Building a select menu on top of the primitives

Here's a complete arrow-key single-choice menu written in pure Que using only
`prompt.read_key` and ANSI escapes:

```que
import std.prompt

fn select(title, options) {
    mut cursor = 0
    let n = options.len()
    println(title)
    loop {
        // Render
        for i in range(0, n) {
            let marker = if i == cursor { "> " } else { "  " }
            println(marker + options[i])
        }
        // Read key
        let k = prompt.read_key()
        if k == "enter"   { return options[cursor] }
        if k == "ctrl+c"  { return null }
        if k == "up"      { cursor = (cursor - 1 + n) % n }
        if k == "down"    { cursor = (cursor + 1) % n }
        // Move cursor back up to overwrite the menu (ANSI: \e[<n>A = up n lines, \e[J = clear below).
        // Note: use `print`, not `println` — println adds a newline that would push the menu down each iteration.
        print("\e[" + str(n) + "A\e[J")
    }
}

let env = select("Deploy to:", ["dev", "staging", "prod"])
println("→", env)
```

The same approach — `read_key` in a loop + ANSI cursor escapes — gives you
multi-select (toggle on Space), yes/no confirms, fuzzy pickers, and so on, all
without adding code to the interpreter.

---

# Part III — Standard Library

The core language and the DevOps-native types cover what you can build with
values. The standard library is what talks to the outside world: config
files, HTTP, hashing, git, containers, and more. Everything here lives behind
an explicit `import std.*` — nothing in this part is a global.

## 23. Config Formats — JSON, YAML, TOML

Que has built-in support for reading, writing, and manipulating **JSON**,
**YAML**, and **TOML** configuration files.

### Parsing and Serialization

The parsing functions require an import from the corresponding `std.*` module:

```que
import std.json
import std.yaml
import std.toml

let data = json.parse("{\"name\": \"myapp\", \"port\": 8080}").unwrap()
println(data["name"])    // myapp

let data = yaml.parse("name: myapp\nport: 8080").unwrap()
let data = toml.parse("name = \"myapp\"\nport = 8080").unwrap()
```

Serialize back to strings:

```que
let data = {"name": "myapp", "port": 8080}

json.stringify(data)        // compact JSON
json.stringify(data, 2)     // pretty-printed with 2-space indent
yaml.stringify(data)        // YAML format
toml.stringify(data)        // TOML format
```

### Method Forms (No Import Needed)

Maps have built-in serialization methods that are always available:

```que
let data = {"name": "myapp", "port": 8080}
println(data.to_json(2))
println(data.to_yaml())
println(data.to_toml())
```

### Reading and Writing Config Files

`config.read` and `config.write` auto-detect the format from the file extension:

```que
import std.config

let settings = config.read(path("./config.json"))?
let more     = config.read(path("./settings.yaml"))?
let manifest = config.read(path("./Cargo.toml"))?

config.write(path("./output.json"), data)   // writes JSON
config.write(path("./output.yaml"), data)   // writes YAML
config.write(path("./output.toml"), data)   // writes TOML
```

### Path Syntax for Nested Access

Navigate deeply nested structures without chains of bracket accesses:

```que
import std.json { parse }

let config = parse("{\"database\": {\"host\": \"localhost\", \"port\": 5432}}").unwrap()

// Navigate with dot notation
println(config.get_path("database.host"))    // localhost
println(config.get_path("database.port"))    // 5432
```

Array indexing and wildcards:

```que
let data = parse("{\"servers\": [{\"host\": \"web1\"}, {\"host\": \"web2\"}]}").unwrap()
println(data.get_path("servers[0].host"))     // web1
println(data.get_path("servers[*].host"))     // ["web1", "web2"]
```

### Modifying Config

`set_path` returns a new map with the change applied:

```que
let updated = config
    .set_path("app.version", "2.0")
    .set_path("app.debug", false)
    .set_path("app.port", 9090)
```

Other path operations:

```que
config.has_path("database.host")    // true
config.delete_path("app.debug")     // returns new map without that key
config.paths()                      // list all leaf paths: ["app.name", "app.port", ...]
```

### In-Place File Editing

`json.edit`, `toml.edit`, and `yaml.edit` read a file, parse it, pass the
document to your callback, and atomically write the result back — all in one
call. The callback receives the parsed map and must return the modified map:

```que
import std.toml

// Bump the version in Cargo.toml
toml.edit("Cargo.toml", |doc| {
    doc.package.version = "2.0.0"
    doc.package.edition = "2024"
    doc
})
```

```que
import std.json

// Update a package.json dependency
json.edit("package.json", |doc| {
    doc.dependencies.react = "^19.0.0"
    doc
})
```

```que
import std.yaml

// Toggle a feature flag in a YAML config
yaml.edit("config.yaml", |doc| {
    doc.features.dark_mode = true
    doc
})
```

> **Tip:** The callback's parameter is mutable, so you can modify nested fields
> directly with `doc.a.b = val` — no need for `set_path`.

### Deep Merge

```que
let base = parse("{\"a\": 1, \"nested\": {\"x\": 10}}").unwrap()
let overlay = parse("{\"b\": 2, \"nested\": {\"y\": 20}}").unwrap()
let merged = base.deep_merge(overlay)
println(merged.get_path("nested.x"))     // 10
println(merged.get_path("nested.y"))     // 20
```

### Stream Integration

Streams can parse config content directly:

```que
import std.stream

let config = stream.file(path("./config.json")).parse_json()?
let settings = stream.file(path("./settings.yaml")).parse_yaml()?
let manifest = stream.file(path("./Cargo.toml")).parse_toml()?
```

### Config Functions Reference

| Function / Method | Description |
|-------------------|-------------|
| `json.parse(str)` | Parse JSON string (returns `Result`) |
| `yaml.parse(str)` | Parse YAML string (returns `Result`) |
| `toml.parse(str)` | Parse TOML string (returns `Result`) |
| `json.stringify(val, indent?)` | Serialize to JSON |
| `yaml.stringify(val)` | Serialize to YAML |
| `toml.stringify(val)` | Serialize to TOML |
| `json.edit(path, fn)` | Read-parse-modify-write JSON file atomically |
| `toml.edit(path, fn)` | Read-parse-modify-write TOML file atomically |
| `yaml.edit(path, fn)` | Read-parse-modify-write YAML file atomically |
| `config.read(path)` | Read and parse (auto-detect format) |
| `config.write(path, val)` | Serialize and write (auto-detect format) |
| `.get_path(path)` | Get nested value by path |
| `.set_path(path, val)` | Set nested value (returns new map) |
| `.delete_path(path)` | Remove at path (returns new map) |
| `.has_path(path)` | Check if path exists |
| `.paths()` | List all leaf paths |
| `.deep_merge(overlay)` | Deep-merge two maps |
| `.to_json(indent?)` | Map method: serialize to JSON |
| `.to_yaml()` | Map method: serialize to YAML |
| `.to_toml()` | Map method: serialize to TOML |

---

## 24. HTTP Client

Que has a built-in HTTP client for talking to REST APIs and downloading files.

```que
import std.http { get, post, download, url_encode, query_string }
```

Every HTTP function returns a `Result`: `Ok(response_map)` on success,
`Err(message)` on transport failure. HTTP error statuses (4xx, 5xx) are still
`Ok` — check `response.ok` or `response.status` to distinguish.

### GET Requests

```que
import std.http { get }

let resp = get("https://api.github.com/zen").unwrap()
println(resp.status)    // 200
println(resp.body)      // some GitHub zen wisdom
println(resp.ok)        // true
```

The response map contains:

| Key | Type | Description |
|-----|------|-------------|
| `status` | Int | HTTP status code |
| `headers` | Map | Response headers (lowercase keys) |
| `body` | String | Response body |
| `ok` | Bool | `true` when status is 2xx |

Two convenience methods save you from writing out `resp["body"]` /
`resp["ok"]` or a manual `json.parse`:

```que
let resp = get("https://api.github.com/repos/acme/ferry").unwrap()
if resp.ok() {
    let repo = resp.json().unwrap()   // parses .body as JSON
    println(repo["full_name"])
}
```

### POST, PUT, PATCH, DELETE

```que
import std.http { post }
import std.json { stringify }

let payload = stringify({"name": "Alice", "role": "admin"})
let resp = post("https://api.example.com/users", payload, {
    "Content-Type": "application/json",
}).unwrap()
println(resp.status)   // 201
```

### The General-Purpose `request`

```que
import std.http { request }

let resp = request({
    "method": "POST",
    "url": "https://api.example.com/webhook",
    "headers": { "Content-Type": "application/json" },
    "body": stringify({"event": "deploy"}),
    "timeout": 30,
}).unwrap()
```

### Downloading Files

```que
import std.http { download }

let result = download(
    "https://example.com/tool-linux-amd64",
    path("./bin/tool"),
).unwrap()
println(result.size)     // bytes written
println(result.path)     // ./bin/tool
```

The body is streamed to disk, so the size of an artefact you can download is
not bounded by memory — which matters, because release tarballs and container
layers are exactly what a build script downloads. Parent directories are
created as needed, and the bytes land in a sibling `.que-partial` file that is
renamed into place only once the transfer completes: an interrupted download
leaves no file that the next run could mistake for a finished one.

Uploads stream too — `upload(url, path)` sends the file without reading it into
memory first.

### URL Encoding and Query Strings

```que
import std.http { url_encode, url_decode, query_string, get }

let encoded = url_encode("hello world & more")
println(encoded)   // hello%20world%20%26%20more

let qs = query_string({"q": "que lang", "page": 1})
let url = "https://api.example.com/search?" + qs
let resp = get(url).unwrap()
```

### HTTP Functions Reference

| Function | Description |
|----------|-------------|
| `get(url, headers?)` | GET request |
| `post(url, body, headers?)` | POST request |
| `put(url, body, headers?)` | PUT request |
| `patch(url, body, headers?)` | PATCH request |
| `delete(url, headers?)` | DELETE request |
| `request(options)` | General-purpose request |
| `download(url, dest, headers?)` | Download to file |
| `upload(url, path, headers?)` | Stream a file as the request body |
| `url_encode(str)` | Percent-encode a string |
| `url_decode(str)` | Decode a percent-encoded string |
| `query_string(map)` | Build query string from map |

`.json()` and `.ok()` (shown above) are general `Map` methods — they read a
`body` / `ok` field off *any* map, which makes them handy wherever your own
code produces a map shaped the same way as an HTTP response.

---

## 25. Filesystem, Archives, Hashing, and CSV

### Expanded Filesystem (`std.fs`)

```que
import std.fs
```

Beyond the basic `read`/`write`/`exists`, `std.fs` provides:

```que
// Atomic write: temp+rename to prevent partial writes
fs.atomic_write(path("config.json"), config.to_json())?

// Recursive directory operations
fs.copy_dir(path("src/"), path("dist/"))?
fs.remove_dir(path("build/"))?

// Find files recursively
let configs = fs.find(path("."), "*.config.json")
for cfg in configs {
    println("found: ${cfg}")
}

// Read and write lines
let hosts = fs.read_lines(path("/etc/hosts")).unwrap()
let filtered = hosts.filter(|line| !line.starts_with("#"))
fs.write_lines(filtered, path("/tmp/active_hosts.txt"))?

// Temp files and directories (manual cleanup)
let tmp = fs.temp_dir("build_").unwrap()
defer fs.remove_dir(tmp)

// Transform a file in place (read → callback → atomic write back)
fs.transform("config.txt", |content| {
    content.replace("localhost", "prod-server.example.com")
})
```

| Function | Description |
|----------|-------------|
| `fs.read(path)` | Read file contents (returns `Result`) |
| `fs.read_secret(path)` | Read a mounted secret file as `Ok(Secret)`, trailing newline stripped |
| `fs.write(path, content)` | Write file (returns `Result`) |
| `fs.exists(path)` | Check if path exists |
| `fs.atomic_write(path, content)` | Write via temp+rename |
| `fs.temp_file(prefix?, suffix?, opts?)` | Create temp file (manual cleanup); `opts.dir` sets the base directory |
| `fs.temp_dir(prefix?, opts?)` | Create temp directory (manual cleanup); `opts.dir` sets the base directory |
| `fs.copy_dir(src, dest)` | Recursively copy directory |
| `fs.remove_dir(path)` | Recursively remove directory |
| `fs.find(dir, pattern?)` | Walk directory, filter by glob |
| `fs.read_lines(path)` | Read file as `List<String>` |
| `fs.write_lines(lines, path)` | Write list as newline-separated file |
| `fs.transform(path, fn)` | Read file, pass to callback, write result back atomically |

### Archive Operations (`std.archive`)

```que
import std.archive
```

All path arguments accept `Path` values (via the `p"..."` literal or `path()`) or plain strings.

#### Creating archives

```que
// Bare paths — each file/directory is archived under its own name.
// A directory is archived as  dirname/contents.
archive.tar_gz(p"out.tar.gz", [p"README.md", p"LICENSE", p"dist/"])

// { src, dest } maps — control exactly where each entry lands.
// For a file,  dest  is the exact archive path.
// For a directory,  dest  is the prefix for the directory's *contents*.
archive.zip(p"release.zip", [
    { src: p"target/release/myapp", dest: "bin/myapp" },
    { src: p"config/default.toml",  dest: "etc/myapp.toml" },
    p"README.md",
])

// Archive a directory's contents at the archive root (no wrapper directory).
// { dest: "." } means "contents go to the top level".
archive.tar_gz(p"site.tar.gz", [{ src: p"build/", dest: "." }])
// build/index.html  ->  index.html   (not  build/index.html)

// Archive a directory's contents under a named prefix.
archive.tar_gz(p"myapp-1.0.tar.gz", [{ src: p"dist/", dest: "myapp-1.0" }])
// dist/bin/myapp  ->  myapp-1.0/bin/myapp

// Optional top-level prefix applied to every entry (convenience shorthand
// equivalent to wrapping all entries in a dest prefix).
archive.tar_gz(p"myapp-v1.0.tar.gz", [
    p"README.md",
    p"LICENSE",
    { src: p"dist/", dest: "." },
], "myapp-v1.0")
// README.md  ->  myapp-v1.0/README.md
// dist/bin   ->  myapp-v1.0/bin

// Set Unix permission bits on a specific entry
archive.tar_gz(p"tools.tar.gz", [
    { src: p"target/release/tool", dest: "bin/tool", mode: 0o755 },
])
```

#### What is preserved

| | tar.gz / tar | zip |
| --- | --- | --- |
| Permission bits | source file's, or `mode:` when given | source file's, or `mode:` when given |
| Modification time | source file's | source file's |
| Empty directories | yes | yes |
| Symlinks and special files | skipped | skipped |

Both formats behave the same way on purpose. A build script that switches from
`tar_gz` to `zip` should not discover afterwards that its release binary came
out without the executable bit, or that every file is stamped with the moment
the archive was built — which defeats every downstream mtime comparison, from
`make` to `rsync`.

#### Extracting and listing

```que
// Extract (destination directory is created if it does not exist)
archive.extract(p"release.tar.gz", p"./extracted/")

// List contents
let entries = archive.list(p"release.tar.gz")
```

Supports `.tar.gz`, `.tgz`, `.tar`, and `.zip` for both `extract` and `list`.

### Checksums and Hashing (`std.hash`)

```que
import std.hash
```

```que
// Hash a string or file
let digest = hash.sha256("hello world")
let file_hash = hash.sha256(path("release.tar.gz"))
let sha512 = hash.sha512("important data")
let md5 = hash.md5(path("legacy.bin"))

// Verify integrity
let ok = hash.verify(path("release.tar.gz"),
    "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e...")
if !ok { fail("checksum mismatch") }

// Generate and verify checksum files
let artifacts = glob("dist/*.tar.gz").expand()
hash.write_checksums(artifacts, path("dist/SHA256SUMS"))
hash.verify_checksums(path("dist/SHA256SUMS"))
```

### CSV Parsing (`std.csv`)

```que
import std.csv
```

```que
// Parse a CSV file (first row = headers)
let rows = csv.parse(path("data.csv"))
for row in rows {
    println("name=${row["name"]}, age=${row["age"]}")
}

// Parse from a string
let rows = csv.parse_str("name,age\nAlice,30\nBob,25")
println(rows[0]["name"])   // Alice

// TSV (tab-separated)
let rows = csv.parse_str(tsv_text, "\t")

// Write CSV
let data = [
    { "name": "Alice", "score": "100" },
    { "name": "Bob",   "score": "85"  }
]
csv.write(data, path("results.csv"))

// Serialize to string
let text = csv.to_string(data)
```

---

## 26. Logging, Git, Dotenv, Templates, and Networking

### Structured Logging (`std.log`)

```que
import std.log
```

Four severity levels, each with an optional fields map:

```que
log.debug("checking connection", { host: "prod-1", port: 5432 })
log.info("deployment started", { version: "1.2.3", env: "production" })
log.warn("retry attempt", { attempt: 3, max: 5 })
log.error("healthcheck failed", { service: "api", status: 503 })
```

Output:
```
[INFO] 1708371234 | deployment started | version=1.2.3 env=production
```

**Level filtering:**

```que
log.set_level("warn")       // only warn and error emitted
```

**JSON output:**

```que
log.set_format("json")
log.info("deploy", { env: "prod" })
// {"level":"INFO","timestamp":1708371234,"message":"deploy","env":"prod"}
```

One line per record, always valid JSON: messages and field values are escaped,
so a newline or a quote in a message cannot break the record your ingester is
parsing. `Int`, `Float`, `Bool` and `null` fields keep their JSON type so a log
query can compare them numerically; anything else is stringified.

**Sinks** — direct log output to files or additional consoles:

```que
log.add_file_sink("/var/log/app.log")
log.add_file_sink("/var/log/errors.json", { format: "json", level: "error" })
log.add_console_sink({ format: "json", level: "debug" })

// Reset to default console-only output
log.remove_sinks()
```

**Logger instances with persistent context:**

```que
let logger = log.new({ service: "api", version: "1.2" })
logger.info("starting")
// [INFO] 1708371234 | starting | service=api version=1.2

let db_log = logger.child({ component: "db" })
db_log.warn("slow query", { ms: 1200 })
```

### Git Integration (`std.git`)

```que
import std.git
```

Read repository metadata (uses built-in libgit2 — no external `git` CLI needed):

```que
let branch = git.branch()           // "main"
let sha    = git.commit()           // full 40-char SHA
let short  = git.short_commit()     // 7-char abbreviation
let tag    = git.tag()              // "v1.2.3" or null
let dirty  = git.is_dirty()         // Bool
let clean  = git.is_clean()         // Bool
let tags   = git.tags()             // List<String>
let origin = git.remote_url()       // "https://github.com/org/repo"
```

All functions accept an optional repository path (default: `"."`).

**Cloning.** `git.clone` returns the destination as a `Path`:

```que
let src = git.clone("https://github.com/org/repo.git")?          // ./repo
let vendored = git.clone(url, p"./vendor/repo")?
git.clone(url, dest, { branch: "main", depth: 1, recursive: true })?
```

Options are `branch`, `depth`, `recursive` and `quiet`. Progress goes to
stderr unless `quiet: true`.

This is the one function in the module that runs the `git` binary rather
than libgit2. Cloning is the only operation here that talks to a remote, and
remotes live behind credential helpers, SSH agents, proxies and per-host
`~/.gitconfig` settings that libgit2 does not read — so it defers to the
program that already handles them.

**Common CI/CD pattern:**

```que
let version = git.tag() ?? "0.0.0-dev+${git.short_commit()}"

if git.is_dirty() {
    fail("cannot release — uncommitted changes detected")
}
```

### Environment Files (`std.dotenv`)

```que
import std.dotenv
```

```que
// Load .env into current process environment
dotenv.load(path(".env"))

// Load and overwrite existing vars
dotenv.load_overwrite(path(".env.local"))

// Parse without loading into environment
let vars = dotenv.parse(path(".env"))
println(vars["PORT"])

// Write a .env file from a map
dotenv.write({
    "PORT": "8080",
    "DATABASE_URL": "postgres://localhost/mydb",
}, path(".env"))
```

### Template Engine (`std.template`)

```que
import std.template
```

**Basic substitution** with `{{ key }}`:

```que
let result = template.render("Hello, {{ name }}!", { "name": "World" })
// "Hello, World!"
```

**Dotted paths:**

```que
let ctx = { "server": { "host": "localhost", "port": 8080 } }
template.render("bind {{ server.host }}:{{ server.port }}", ctx)
// "bind localhost:8080"
```

**Conditionals:**

```que
let tmpl = "mode: {{# if production }}PROD{{# else }}DEV{{/ if }}"
template.render(tmpl, { "production": true })   // "mode: PROD"
```

**Loops:**

```que
let tmpl = "{{# for svc in services }}- {{ svc.name }}: {{ svc.port }}\n{{/ for }}"
let ctx = {
    "services": [
        { "name": "api", "port": 8080 },
        { "name": "web", "port": 3000 }
    ]
}
template.render(tmpl, ctx)
```

**Rendering from files:**

```que
let config = template.render_file(path("templates/nginx.conf.tmpl"), {
    "server_name": "example.com",
    "upstream_port": 8080
})
```

### Network Utilities (`std.net`)

```que
import std.net
```

```que
// Check host reachability
net.ping("example.com")
net.ping("10.0.0.1", 2s)      // with timeout

// Check if a port is open
net.port_open("localhost", 5432)

// DNS resolution
let ips = net.resolve("example.com")

// Wait for port (essential after spawn)
let server = spawn `npm run dev`
defer server.kill()
net.wait_for_port("localhost", 3000, 60s, 1s)

// Wait for HTTP endpoint
net.wait_for_url("http://localhost:8080/health", 60s, 2s)
```

---

## 27. Date and Time

```que
import std.time
```

Create DateTime values from components, timestamps, or parsed strings:

```que
let now   = time.now()                      // local timezone (from $TZ, default UTC)
let dt    = time.of(2024, 6, 15, 14, 30, 0) // June 15 2024, 2:30pm UTC
let east  = time.of(2024, 6, 15, 10, 0, 0, "America/New_York")
let ts    = time.from_timestamp(time.timestamp())  // from a millisecond count
let parsed = time.parse("2024-03-15 14:30:00", "%Y-%m-%d %H:%M:%S")
```

`time.now()` and `time.timestamp()` answer different questions. Reach for
`time.now()` when you want a date to format or compare by field, and for
`time.timestamp()` when you want a plain number to subtract from another
number:

```que
let started = time.timestamp()
build()
println("took ${time.timestamp() - started}ms")
```

Extract components:

```que
println(dt.year())       // 2024
println(dt.month())      // 6
println(dt.day())        // 15
println(dt.hour())       // 14
println(dt.minute())     // 30
println(dt.weekday())    // "Sat"
println(dt.day_of_year()) // 167
```

Format and convert:

```que
println(dt.format("%Y/%m/%d %H:%M"))  // "2024/06/15 14:30"
println(dt.to_iso())                    // RFC 3339 string
println(dt.timestamp())                 // Unix ms
```

Timezone conversion:

```que
let nyc = dt.in_tz("America/New_York")
println(nyc.timezone())   // "America/New_York"
println(nyc.hour())       // 10 (UTC-4 in summer)
let back = nyc.utc()
println(back.timezone())  // "UTC"
```

Arithmetic (returns a new DateTime):

```que
let tomorrow  = dt.add_days(1)
let yesterday = dt.add_days(-1)
let later     = dt.add_hours(3)
let soon      = dt.add_minutes(15)
```

Comparison uses `<`, `>`, `<=`, `>=` by timestamp:

```que
let a = time.of(2024, 1, 1)
let b = time.of(2024, 6, 15)
println(a < b)   // true
```

String interpolation prints ISO 8601:

```que
println("deployed at ${dt}")  // "deployed at 2024-06-15T14:30:00+00:00"
```

**Constructor functions:**

| Function | Description |
|---|---|
| `time.now()` | Current time in local timezone (from `$TZ`, default UTC) |
| `time.timestamp()` | Current Unix timestamp in milliseconds, as a plain `Int` |
| `time.of(y, m, d, h?, min?, sec?, tz?)` | Construct from components (defaults: `0` for time, `"UTC"` for tz) |
| `time.parse(str, fmt, tz?)` | Parse a string with a `strftime` format |
| `time.from_timestamp(ms, tz?)` | Construct from Unix milliseconds |
| `time.timezone()` | Detected system IANA timezone name (e.g. `"Europe/Berlin"`) |

**DateTime methods:**

| Method | Returns | Description |
|---|---|---|
| `.year()` `.month()` `.day()` | `Int` | Date components |
| `.hour()` `.minute()` `.second()` | `Int` | Time components |
| `.weekday()` | `String` | `"Mon"` .. `"Sun"` |
| `.day_of_year()` | `Int` | Ordinal day (1–366) |
| `.format(fmt)` | `String` | Format with `strftime` pattern |
| `.to_iso()` | `String` | RFC 3339 / ISO 8601 |
| `.timestamp()` | `Int` | Unix milliseconds |
| `.in_tz(tz)` | `DateTime` | Convert to another timezone |
| `.utc()` | `DateTime` | Convert to UTC |
| `.timezone()` | `String` | Current timezone name |
| `.add_days(n)` `.add_hours(n)` | `DateTime` | Arithmetic (negative values subtract) |
| `.add_minutes(n)` `.add_seconds(n)` | `DateTime` | Arithmetic (negative values subtract) |

---

## 28. Containers and Remote Execution

### Containers (`std.container`)

```que
import std.container
```

A full integration-test fixture:

```que
import std.container

let pw = env.secret("TEST_DB_PASSWORD")?

container.run({
    image: "postgres:16",
    name: "test-db",
    ports: { "5432": 5432 },
    env: { "POSTGRES_PASSWORD": pw, "POSTGRES_DB": "testdb" },
})?
defer container.remove("test-db")

container.wait_healthy("test-db", 30s)?
container.exec("test-db", "psql -U postgres -c 'select 1'")?
```

| Function | Description |
|---|---|
| `container.engine()` | `"docker"`, `"podman"`, `"nerdctl"` or `null` |
| `container.build(opts)` | Build an image; returns the tag |
| `container.run(opts)` | Start a container; returns its id |
| `container.exec(name, command, opts?)` | Run something inside it |
| `container.stop(name, timeout?)` | Stop it |
| `container.remove(name)` | Force-remove it |
| `container.logs(name, tail?)` | Read its logs |
| `container.exists(name)` | Does it exist |
| `container.is_running(name)` | Is it running |
| `container.wait_healthy(name, timeout?)` | Block until healthy |
| `container.pull(image)` / `container.push(image)` | Registry transfer |
| `container.login(registry, user, password)` | Registry auth |

**`build` options:** `tag` (required), `context` (default `"."`), `file`,
`build_args`, `platform`, `cache`, `pull`, `targets` (list, repeated
`--target`, for multi-stage builds), `args`.

**`run` options:** `image` (required), `name`, `detach` (default `true`),
`remove` (default `true`), `ports`, `volumes`, `env`, `workdir`, `user`,
`network`, `tty` (attach the terminal; incompatible with `detach: true`),
`command`, `args`.

**`exec` options:** an optional trailing `opts` map accepts `tty` the same
way, for an interactive shell inside a running container.

**It is `container`, not `docker`.** Podman is a drop-in for everything here,
and naming the module `docker` would force every podman user to write the
wrong word. `$QUE_CONTAINER_ENGINE` overrides detection.

**It drives the CLI, not the daemon socket.** Credential helpers,
`DOCKER_HOST`, contexts, rootless configuration, buildx builders and registry
auth all live in the CLI. A direct socket client reimplements them and works
in fewer places.

**`--rm` is on by default.** A script that leaves stopped containers behind
fills a CI runner's disk over a few hundred builds, and nothing in the script
points at the cause. Pass `remove: false` when you want to inspect the corpse.

**A `Secret` in `env` never reaches the command line.** Only the variable
*name* is passed to the engine; the value is set in the engine process's own
environment. This matters because `ps` is readable by every user on the host,
so `-e POSTGRES_PASSWORD=hunter2` publishes the password to all of them.
`container.login` uses `--password-stdin` for the same reason.

**`wait_healthy` handles images without a `HEALTHCHECK`.** Those report no
status at all, and a naive poll blocks until the timeout on a container that
started perfectly. Que falls back to "is it running", which is the strongest
claim available. A container reporting `unhealthy` fails immediately, because
the engine has already exhausted its own configured retries by then.

**`exec` distinguishes a string from a list.** A `String` or backtick command
is shell syntax, so it runs through `sh -c` *inside* the container. A `List`
is an argv and is passed straight through, with no shell involved:

```que
container.exec("db", "pg_dump mydb | gzip > /tmp/dump.gz")   // shell
container.exec("db", ["pg_dump", "mydb"])                    // argv
```

Compose is not wrapped; it needs no wrapping:

```que
`docker compose -f ${compose_file} up -d`.run()
defer `docker compose -f ${compose_file} down -v`.run()
```

### Remote Execution (`std.ssh`)

```que
import std.ssh
```

**`ssh.cmd(host, command, opts?)`** returns an ordinary `Cmd`. That is the
whole design: a remote command is a command, so every modifier you already
know works on it, and `--dry-run` shows it like any other.

```que
import std.ssh

ssh.cmd("web-1", `systemctl restart nginx`, { user: "deploy" })
    .timeout(30s)
    .run()

let kernel = ssh.out("web-1", "uname -r")?
```

| Function | Description |
|---|---|
| `ssh.cmd(host, command, opts?)` | Build a remote command as a `Cmd` |
| `ssh.run(host, command, opts?)` | Run it, return `Ok(ProcessResult)` |
| `ssh.out(host, command, opts?)` | Run it, return `Ok(trimmed stdout)` or `Err` |
| `ssh.check(host, opts?)` | `true` if the host is reachable and authenticates |
| `ssh.upload(local, host, remote, opts?)` | `scp` a path up |
| `ssh.download(host, remote, local, opts?)` | `scp` a path down |

**Options** (shared by all of them):

| Option | Default | Description |
|---|---|---|
| `user` | none | Prepended as `user@host`, unless the host already has one |
| `port` | ssh default | Connect port |
| `key` | agent/config | Identity file; also pins `IdentitiesOnly=yes` |
| `jump` | none | Bastion host (`-J` / `ProxyJump`) |
| `connect_timeout` | `10` | Seconds before giving up on the connection |
| `interactive` | `false` | Allow password prompts (otherwise `BatchMode=yes`) |
| `forward_agent` | `false` | Forward the ssh agent (`-A`) |
| `strict_host_key_checking` | `true` | Verify the host key |
| `known_hosts` | ssh default | Alternative `UserKnownHostsFile` |
| `options` | `[]` | Raw `-o Key=Value` strings for anything not listed |

**Why it drives the `ssh` binary.** Que does not link an SSH library. Your
system client already knows about ssh-agent, `~/.ssh/config` and its `Match`
blocks, `ProxyJump`, hardware keys, GSSAPI, and whatever `Include` file your
security team ships. A linked library starts from zero on every one of those,
and the first host someone cannot reach is the last time they use the
feature. This is the same reasoning behind `std.git`.

**Batch mode is the default.** Without it, an unreachable host makes a CI job
hang on a password prompt nobody will ever answer until the job times out.
Pass `interactive: true` when a human is present.

**An explicit `key` also sets `IdentitiesOnly=yes`.** Otherwise ssh offers
every identity the agent holds *before* the one you asked for, and a host
configured with `MaxAuthTries=3` disconnects before reaching it — producing a
"permission denied" for a key that is perfectly valid.

**Turning off host key verification also detaches `known_hosts`:**

```que
ssh.cmd("ephemeral-ci-box", "id", { strict_host_key_checking: false })
```

sets `UserKnownHostsFile=/dev/null` as well. Writing an unverified key into
your real `known_hosts` would mean every *later* connection silently trusts
it. Only use this for hosts that are genuinely disposable.

The remote command is single-quoted, so the remote shell sees exactly what you
wrote and your local shell does not get a second chance to interpret it:

```que
ssh.cmd("web-1", "echo $HOSTNAME > /tmp/who")   // $HOSTNAME expands remotely
```

---

## 29. Watching Files

```que
import std.watch
```

The dev loop — edit a file, rebuild, repeat — is the one thing a build tool
does that a plain script cannot. `std.watch` provides it.

**`watch.run(paths, callback, opts?)`** watches and rebuilds forever:

```que
import std.watch

watch.run(p"src", |changes| {
    println("changed: ${changes.len()} file(s)")
    `cargo build`
}, { initial: true, debounce: 300ms })
```

`initial: true` runs the callback once immediately, so the first build does
not wait for you to type something. Ctrl-C unwinds the loop normally, which
means any `defer` you set up — killing a `spawn`ed dev server, removing a
temp directory — still runs.

**`watch.wait(paths, opts?)`** is the one-shot primitive, for when you want
to drive the loop yourself:

```que
let changes = watch.wait([p"src", p"Cargo.toml"], { timeout: 30s })?
for c in changes {
    println("${c["kind"]}: ${c["path"]}")
}
```

Each change is a map with `path` (a `Path`) and `kind` (`"created"`,
`"modified"` or `"deleted"`). Directories are watched recursively.

**Options:**

| Option | Default | Description |
|---|---|---|
| `interval` | `250ms` | How often to poll |
| `debounce` | `200ms` | Wait until the tree stops changing before reporting |
| `timeout` | none | Give up and return `Err` (or stop the loop) after this long |
| `ignore` | see below | Glob patterns to exclude |
| `initial` | `false` | (`run` only) Fire the callback once before waiting |
| `times` | none | (`run` only) Stop after this many callback runs |

`ignore` defaults to `.git`, `target` and `node_modules`. That default is not
a convenience — without it, a callback that runs `cargo build` writes into
`target/`, which wakes the watcher, which runs `cargo build` again, forever.
If you set `ignore` yourself, make sure your own build output is in it.

**Why polling.** `std.watch` compares directory snapshots rather than using
inotify, FSEvents or `ReadDirectoryChangesW`. Native watch APIs are faster and
cheaper at rest, but they do not work on several filesystems people actually
run builds on: bind-mounted Docker volumes, NFS and SMB shares, and `/mnt/c`
under WSL all drop events silently or emit none at all. A watcher that misses
changes fails in the worst possible way — the build quietly stops rebuilding
and nobody sees an error. Polling has no blind spot, and a quarter-second of
latency is invisible next to the time it takes to notice your editor saved.

Snapshots compare both mtime *and* size, because many filesystems store mtime
at one-second granularity and a rebuild that rewrites a file within the same
second would otherwise go unnoticed.

**`watch.snapshot(paths, opts?)`** returns how many files are being tracked.
Use it to fail fast instead of watching an empty or misspelled directory:

```que
if watch.snapshot(p"src") == 0 {
    fail("nothing to watch under src/")
}
```

A glob is rejected as a watch root on purpose: a glob is expanded once, so a
file created afterwards would never match. Watch the containing directory and
filter inside the callback instead.

Under `--dry-run`, `watch.run` announces itself and returns immediately rather
than blocking forever on a change that is never coming.

---

# Part IV — The Build System

Everything up to here works in a standalone script. This part covers what
happens when you organize work into a **Quefile**: tasks with dependencies,
freshness tracking, tests, formatting, linting, and the safety nets
(`--dry-run`, `--strict`, `--allow`/`--deny`) that make a task runnable
without reading it first.

## 30. Tasks and the Build Graph

Tasks are Que's **first-class build automation primitive**. They model units
of work with declared dependencies, input/output tracking, automatic skip
detection, and a built-in execution graph.

### Declaring and Running Tasks

```que
@description("Compile the project")
task build {
    println("compiling...")
    println("done!")
}

build()
// [RUN]  build
// compiling...
// done!
// [DONE] build
```

Tasks emit color-coded status lines (`[RUN]`, `[DONE]`, `[FAIL]`, `[SKIP]`).

### Task Metadata

```que
@description("Compile source files")
@inputs(["src/**/*.que", "que.toml"])
@outputs(["./build/app"])
@env([CC, CFLAGS])
task compile {
    println("compiling from sources...")
}
```

- **`description`** — human-readable summary
- **`inputs`** — file paths or glob patterns the task reads
- **`outputs`** — file paths the task produces
- **`env`** — environment variable names the task depends on
- **`deps`** — tasks that must run first (see below)
- **`aliases`** — additional names the task can be run under

```que
@description("Run the test suite")
@aliases([t, tests])
task test {
    `cargo test`
}
```

```sh
que run t          # same task as `que run test`
```

If you don't need metadata, just write the body directly (no `run` block):

```que
task clean {
    println("cleaning build artifacts")
}
```

### Dependencies

Declare dependencies with the `@deps` attribute. Que resolves the dependency
graph and runs prerequisites first:

```que
task compile { println("compiling") }
@deps([compile])
task link { println("linking") }
@deps([link])
task package { println("packaging") }

package()
// [RUN]  compile  -> [DONE]
// [RUN]  link     -> [DONE]
// [RUN]  package  -> [DONE]
```

Every piece of task metadata is written the same way — as an attribute above
the declaration, never inside the braces:

```que
@description("Bundle the release artifact")
@deps([link])
@inputs(["./build/app"])
@outputs(["./dist/app.tar.gz"])
task package {
    println("packaging")
}
```

Diamond dependencies are deduplicated — if multiple tasks depend on the same
prerequisite, it runs exactly once.

`@deps` takes task *names*, bare or quoted — `@deps([link])` and
`@deps(["link"])` mean the same thing, as with `@aliases` and `@env`. The name
is looked up when the task runs, so anything that is not a name is refused
where it is written:

```que
@deps([make_dep("link")])   // parse error: expected dependency name
```

### Parameterized Tasks

```que
@description("Build for a specific target")
task build(target, mode = "debug") {
    println("building ${target} in ${mode} mode")
}

build("linux-amd64")                    // positional — debug mode
build("linux-arm64", "release")         // positional — release mode
build(target: "darwin-arm64")           // named — uses default mode
build(target: "win-amd64", mode: "release")  // all named
```

From the CLI, pass arguments after `--`:

```sh
que run build -- linux-amd64          # positional
que run build -- target=darwin-arm64  # named
que run build -- target=win mode=release
```

Use `--help` to see the full argument table for a task:

```sh
que run build --help
# Usage: build [target] [mode]
#
# Build for a specific target
#
# Arguments:
#   target  (required)
#   mode    default: "debug"
```

An argument no parameter can claim is an error, not something quietly dropped:

```sh
que run build -- linux-amd64 release oops
# task 'build' takes 2 positional arguments, but 3 were given
```

#### Rest Parameters

When the number of arguments is the caller's to decide — a list of files a
shell glob expanded to, say — declare the last parameter with `...`. It
collects every remaining positional argument into a list:

```que
task lint(...files) {
    if files.len() == 0 {
        println("nothing to lint")
        return
    }
    for f in files {
        println("linting ${f}")
    }
}
```

```sh
que run lint -- src/a.que src/b.que src/c.que
que run lint                            # files == []
```

A rest parameter is never required: with nothing left over it is an empty
list, so it takes no default. It must come last, and it is a task spelling
only — a `fn` has a caller that knows its own argument count, so it does not
need one.

### Incremental Builds (Freshness)

When a task declares both `inputs` and `outputs`, Que decides whether it still
needs to run:

```que
@inputs(["src/**/*.rs"])
@outputs(["./target/release/app"])
task compile {
    println("compiling (this is expensive)...")
}

compile()  // First run:  [RUN] -> [DONE]
compile()  // Second run: [SKIP] (outputs are fresh)
```

The check asks three questions, cheapest first:

1. **Do all outputs exist**, and do the arguments and any `env:` variables
   match the last successful run? If not, the task runs.
2. **Is every input older than every output?** If so nothing changed and no
   file is read.
3. **Otherwise, do the inputs still hash to what the last run recorded?**

The third step is what makes freshness survive a change of machine. A
timestamp records when a file was written, not what is in it — and a fresh
`git clone`, a restored CI cache, a container layer or a plain `touch` all
rewrite timestamps without changing a byte. With timestamps alone, every one
of those turns into a full rebuild.

The record lives in `.que/task-cache.json` next to your script. Add `.que/` to
`.gitignore`; deleting it is safe, and costs at most one rebuild. Nothing is
written for scripts run from the REPL or embedded, since there is no project
directory to anchor a cache to.

#### Running anyway

`--force` (or `-B`) runs the task whether or not it looks up to date:

```
que run --force compile
```

Dependencies are forced too, so `--force` on a task at the top of a chain
rebuilds the chain. The run is still recorded, so the next plain `que run`
skips again.

Reach for it rather than trying to provoke a rerun by hand — the two obvious
tricks do not work, and for the same reason each exists. Deleting
`.que/task-cache.json` does nothing if every input is already older than every
output, because question 2 answers before the cache is consulted. And `touch`
does nothing either, because question 3 hashes the contents and finds them
unchanged — ignoring a timestamp bump with no edit behind it is the whole point
of that step. Deleting an output does force a run, since question 1 fails, but
`--force` says what you mean without removing anything.

### Task Status and Introspection

```que
@description("Deploy to production")
task deploy {
    42
}

println(deploy.name)          // "deploy"
println(deploy.description)   // "Deploy to production"
println(deploy.status)        // "pending"
deploy()
println(deploy.status)        // "succeeded"
```

Status values: `"pending"`, `"succeeded"`, `"failed"`, `"skipped"`.

| Method | Description |
|--------|-------------|
| `.run(args...)` | Execute the task (with dependency resolution) |
| `.deps()` | List of dependency names |
| `.inputs()` | Evaluated list of input paths |
| `.outputs()` | Evaluated list of output paths |
| `.status()` | Current execution status |
| `.result()` | The value the task's body evaluated to |
| `.description()` | Description or `null` |

### Passing Data Between Tasks

A dependency often produces something the task that needs it has to know about:
a temporary directory, a version string, a list of files. The value a task's
body evaluates to is kept, and `.result()` hands it over:

```que
import std.fs

task get_profiles {
    let dir = fs.temp_dir("profiles_").unwrap()
    `curl -sL $PROFILE_URL | tar -xz -C ${dir}`
    dir                              // the task's result
}

@deps([get_profiles])
task install {
    let dir = get_profiles.result()  // /tmp/profiles_xxxx
    `install --prefix ${dir}`
}
```

`run_task("get_profiles")` returns the same value, and does not run the task
again if it already succeeded in this run — so it works whether or not the task
is also named in `@deps`.

Two edges are worth knowing. A task that was **skipped** because its declared
outputs were up to date never ran its body, so its result is `null`; the files
it declared as `@outputs` are what it produced in that case. And asking for the
result of a task that has not run at all is an error rather than a silent
`null`, since that is almost always a missing `@deps`.

### The `tasks()` and `run_task()` Builtins

```que
// List all registered tasks
let all = tasks()      // Map of task_name -> task_value
for name in all.keys() {
    println(name)
}

// Execute a task dynamically by name
run_task("build")
```

### Command-Line Task Runner

Create a **Quefile** (or `Quefile.que`) in your project root:

```que
// Quefile
import std.fs { read, write, exists }

@description("Remove build artifacts")
task clean {
    if exists(path("./build")) {
        path("./build").delete()
    }
}

@description("Compile the project")
@deps([clean])
task compile {
    println("compiling...")
}

@description("Run test suite")
@deps([compile])
task test {
    println("testing...")
}
```

Then use the CLI:

```sh
que tasks                              # list all tasks
que run deploy                         # run a task (with dependencies)
que run -f build/pipeline.que deploy   # use a specific Quefile
que run -g backup                      # run a task from the global Quefile
que run build --help                   # show task usage and argument table
que run build -- linux-amd64           # positional arguments
que run build -- target=linux mode=release  # named arguments
```

`que run` accepts exactly one task name. Dependencies are resolved and run
automatically before the named task.

#### Where the Quefile comes from

Without `-f`, Que looks for `Quefile`, `Quefile.que`, or `quefile.que` in the
current directory and then in each parent, so `que run test` works from
anywhere inside a project. Walking up to find the Quefile does not move you:
the task still runs in the directory you invoked `que` from, so a relative path
means what it would have meant in your shell.

A task the project Quefile does not define is looked up in your **global
Quefile**, the first of these that exists:

```
$QUE_HOME/Quefile
$XDG_CONFIG_HOME/que/Quefile
~/.config/que/Quefile
~/.que/Quefile
```

Global tasks are the ones you want everywhere — `backup`, `sync-dotfiles`,
`scaffold` — so they run in the directory you invoked `que` from, not in your
home directory. A project task shadows a global task of the same name; `-g`
ignores the project Quefile and goes straight to the global one. `que tasks`
lists both, marking shadowed global names.

#### Dependencies for the global Quefile

A Quefile is a script like any other, so its **own directory** is its package
root: a global Quefile imports against the `que_packages/` beside it, not
against whatever project you are standing in. Put a `que.toml` next to it and
install with `-g`, from anywhere:

```
~/.que/
  Quefile
  que.toml
  que.lock
  que_packages/
    que_std/
```

```toml
# ~/.que/que.toml
[dependencies]
que-std = { git = "https://github.com/jasal82/que-lang", tag = "1.1.0", subdir = "stdlib" }
```

```sh
que install -g          # installs into the global Quefile's directory
```

```que
// ~/.que/Quefile
import que_std.colors { colored }
import que_std.select { select }

task deploy {
    let target = select("Deploy to:", ["dev", "staging", "prod"])
    println(colored("deploying to ${target}").green())
}
```

This holds however the global task was reached — `que run -g deploy`, or
`que run deploy` falling through from a project that does not define it. The
task still runs in the directory you invoked `que` from; only import
resolution follows the file.

A project never resolves imports against the global directory. That would
make a checkout depend on what the machine happens to have installed, which
is the thing `que.toml` and `que.lock` exist to prevent.

#### Rooting paths at the project or at the caller

Since every relative path resolves against the caller's directory, a Quefile
usually wants both roots. A `fmt` task that formats what you are standing in
wants the caller's directory; a `build` task wants the project's `build/`
wherever you ran it from. Name them both at the top of the Quefile and neither
is in doubt:

```que
import std.fs { write }

let project = quefile_dir()        // where the Quefile lives
let here    = path(".").resolve()  // where `que` was invoked

@inputs([project / "src/*.que"])
@outputs([project / "build/app"])
task build {
    (project / "build").mkdir()
    println("building into " + str(project / "build"))
}

@outputs([here / "report.txt"])
task report {
    write(here / "report.txt", "done")
}
```

Binding the root once matters more than it looks: `@inputs` and `@outputs` are
evaluated outside the task body — before it runs, to decide whether it can be
skipped, and after, to record what it produced. A shared binding is what keeps
the attributes and the body pointing at the same files.

Glob patterns are expanded in `@inputs` however you write them — a plain
string, a `g"..."` literal, or a pattern joined onto a root with `/`:

```que
@inputs([project / "src/*.que"])              // expands
@inputs(["${quefile_dir()}/src/*.que"])       // expands
@inputs([g"${quefile_dir()}/src/*.que"])      // expands
```

In `@outputs` a pattern is an error. Those files do not exist yet when the
freshness check runs, so a pattern could never match one — the task would look
permanently unfinished and rerun every time. Name the directory, or a stamp
file the task writes last:

```que
@outputs([project / "build/*.o"])       // error: not a concrete path
@outputs([project / "build"])           // the directory
@outputs([project / "build/.stamp"])    // a file the task writes at the end
```

If nearly every task in a Quefile is project-rooted, `cd` at the top of the
file moves the process once, before any task runs. It returns the directory it
left, so you keep the caller's directory too:

```que
let here = cd(quefile_dir())   // now: cwd is the project, `here` is the caller
```

That is what `cd` is for: a move meant to outlive the block it appears in.
Inside a task body, where the move is meant to end, use `with dir(...)`, which
restores the old directory on the way out — including when the block fails:

```que
task build {
    with dir(quefile_dir() / "build") {
        `make`.run()
    }
    // back where the task started, whether or not make succeeded
}
```

A bare `cd(...)` whose return value is discarded is reported by `que lint` as
`unscoped-cd`, since nothing is left to move back with.

Neither form is safe inside `parallel`. The working directory belongs to the
process, not to a task or a block, so a `with dir(...)` open in one branch
moves every other branch for as long as it is open — scoping bounds the change
in time, not across threads. Point a single command somewhere else with
`` `cmd`.dir(p) ``, which never moves the process at all.

The same applies to plain scripts, where `script_dir()` is the name for the
directory the running `.que` file lives in.

---

## 31. Testing, Formatting, and Linting

### Testing (`que test`)

A test is a top-level function whose name starts with `test_` and that takes no
arguments. There is no attribute to remember and no framework to import:

```que
// math_test.que
import .math { add }

fn test_adds_positive_numbers() {
    assert(add(2, 3) == 5)
}

fn test_adds_negatives() {
    assert(add(-2, -3) == -5)
}

fn round_trip(x) {       // no prefix: a helper, not a test
    add(x, 0)
}
```

```sh
que test                       # every test file under the current directory
que test tests/                # a directory
que test math_test.que         # one file, whatever it is named
que test --filter adds_neg     # only tests whose name contains this
```

```
math_test.que
  ok   test_adds_positive_numbers
  FAIL test_adds_negatives
       math_test.que:8:5: assertion failed: add(-2, -3) == -5  (5 == -5)

1 of 2 failed
```

**Discovery.** A file is a test file if it is named `*_test.que` or
`test_*.que`, or if it lives under a directory called `tests`. A file you name
on the command line is always run, whatever it is called. `target`,
`node_modules` and dot-directories are skipped.

**What counts as a failure.** A test fails if it raises — which `assert`,
`fail()`, `?` and a failing command all do — or if it returns
`Err(...)`. Anything else passes.

**Output.** A test's `println` output is captured and shown only when that test
fails, where it is the context that explains the failure. A passing run stays
quiet.

**Scope.** Top-level code in a test file runs once, before any test, and what
it defines is shared by all of them. Each test then runs in its own scope, so a
`let` in one test is not visible in the next.

**Exit code.** `0` when every test passed, `1` when any failed, `2` when a file
could not be read or a `--filter` matched nothing. A run that verifies nothing
is an error, not a pass.

### Code Formatting (`que fmt`)

```sh
que fmt                    # format all .que files recursively
que fmt script.que        # format a specific file
que fmt --check            # check without writing (exit 1 if unformatted)
que fmt --diff             # show diff without writing
```

The formatter enforces:
- 4-space indentation
- Spaces around operators
- One blank line between top-level declarations
- Newlines as statement terminators
- Trailing commas in multi-line collections
- Opening brace on same line

### Linting (`que lint`)

```sh
que lint                   # lint all .que files
que lint script.que       # lint a specific file
```

| Rule | Description |
|------|-------------|
| `undefined-name` | An identifier that is never bound — usually a typo |
| `arity` | A call with too few or too many arguments |
| `unreachable-code` | Code after `return`, `break`, `continue`, or `fail()` |
| `unused-result` | Command or HTTP response result discarded without checking |
| `empty-block` | Function with empty body |
| `secret-interpolation` | Potential secret interpolated into a display string. Not reported inside a `` ` ` `` command, where handing the token to the process is the point |

The first two are the reason to run `que lint` in CI before anything deploys.
They are found by walking the source with a scope stack, so they are reported
for every line of the script — including branches that a given run never
reaches:

```que
fn deploy(env, version) {
    println("deploying ${version} to ${env}")
}

let version = "1.4.0"
if env.get("CI") != null {
    deploy("prod")        // arity: 'deploy' takes 2 argument(s), but 1 were given
} else {
    deploy("staging", verison)   // undefined-name: 'verison' (did you mean 'version'?)
}
```

Both rules only report what is certain from the syntax alone. A name is
undefined only if it is not a local, a parameter, a top-level declaration, an
import, a task, an enum variant, or a builtin — and the check switches off
entirely inside a file that uses a wildcard import (`import std.json { * }`),
since the imported names are not knowable statically. Field and method names
are not checked, because they depend on values.

The same diagnostics appear live in the editor through the language server.

---

## 32. Strict Mode, Dry Runs, and Sandboxing

### Strict Mode (Type Enforcement)

Que supports optional type annotations. By default they are documentation-only.
Strict mode enforces them at runtime. It is a property of the file, declared in
the prologue:

```que
#!strict

fn add(a: Int, b: Int) -> Int {
    a + b
}

add(1, 2)       // OK
// add("x", 2)  // ERROR: parameter 'a' expected Int, got String
```

The pragma goes above everything except a `#!/usr/bin/env que` line, which may
precede it. Return types are enforced too:

```que
#!strict

fn get_name() -> String {
    "Alice"           // OK
}

fn bad() -> Int {
    "not a number"    // ERROR: return type expected Int, got String
}
```

| Annotation | Matches |
|------------|---------|
| `Int` | Integer values |
| `Float` | Floating-point values |
| `Bool` | Booleans |
| `String` | Strings |
| `Path` | Path values |
| `Duration` | Duration values |
| `List` | Lists |
| `Map` | Maps |
| `Set` | Sets |
| `Tuple` | Tuples |
| `Function` | Functions and builtins |
| `Any` | Any type (always passes) |

Parameters without annotations are not checked, even in strict mode.

A single run can be checked without editing the file:

```sh
que --strict deploy.que
```

```que
let is_strict = strict()  // query current state
```

### Dry Runs (`--dry-run`)

```sh
que deploy.que --dry-run
que run release --dry-run
```

In a dry run, operations that change the world outside the process are printed
instead of performed:

```
[dry-run] kubectl apply -f deploy.yaml
[dry-run] write /etc/app.conf (214 bytes)
[dry-run] http.post https://hooks.example.com/notify
```

Suppressed: running a command (including `spawn` and `|` pipelines), the
filesystem operations that write, create, copy, move or remove, and the HTTP
verbs that change something on the other end (`post`, `put`, `patch`,
`delete`, `upload`, `download`). Each returns a plausible success value, so
the script keeps going.

`open(path, "w")` and `open(path, "a")` are suppressed *at the open*, not at
the first `write` — opening for writing is already destructive, since `"w"`
truncates. What you get back is a real handle that reports the right path and
mode and accepts writes, announcing each one and discarding it:

```
[dry-run] open ./build/manifest.json for writing (truncate)
[dry-run] write ./build/manifest.json (214 bytes)
```

Refusing to open would push the script down its error path, and a dry run that
stops at the first write shows nothing about what the rest of it would do.

Not suppressed: reads. A script that cannot read cannot reach the decisions
the dry run exists to show. `open(path)` with no mode reads for real.

A dry run is a preview, not a simulation, and it cannot be perfect. A script
that branches on the output of a command it did not run will take a different
path than the real thing. Where that matters, ask:

```que
if dry_run() {
    println("would restart ${service}")
} else {
    restart(service)
}
```

`dry_run()` is also the only way to guard an effect Que does not know about —
anything you reach through a library or a side effect of your own code.

Task freshness is not recorded during a dry run: the outputs were never
produced, so recording them would make the next real run skip the work.

### Sandboxing (`--allow` / `--deny`)

A dry run answers "what *would* this do?". A sandbox answers "what is this
*permitted* to do?" — which is the question you want when you run a task you
did not write.

Que has five capabilities:

| Capability | Covers | Subject a grant is scoped to |
| --- | --- | --- |
| `read` | `std.fs` reads, `Path` read methods, `json.read`, `watch`, `open(p)`, `glob`, `stream.file(p)`, `config.read` | a path prefix |
| `write` | `std.fs` writes, `Path` write methods, `archive`, downloads, `open(p, "w")`, `stream.write_to`, `config.write`, `TempDir`/`TempFile` | a path prefix |
| `exec` | every backtick command, pipeline, `spawn`, `std.git`, `std.container` | the rendered command text |
| `net` | `std.http`, `std.net`, `std.ssh` | a host name |
| `env` | `env.get`, `env.set`, `env.all`, `with env.scope` | a variable name |

The capability a builtin needs sometimes depends on its arguments, and the
sandbox reads them rather than guessing:

```bash
que t.que --allow read=.        # open("log.txt")        ok
                               # open("log.txt", "w")   denied -- that is a write
                               # stream.of("a\nb")      ok -- in memory, reads nothing
                               # stream.file(p"log.txt") ok -- a file read
```

`with env.scope({...})` is checked one variable at a time, so
`--allow env=CI` admits a scope that sets `CI` and refuses one that also sets
`AWS_SECRET_ACCESS_KEY` — and the denial names the variable without printing
its value.

Nothing is restricted until you ask for it:

```bash
que build.que                       # unrestricted, exactly as before
```

`--allow` switches the script into deny-by-default and grants one capability:

```bash
que build.que --allow read --allow exec
```

A grant can be scoped to a comma-separated list:

```bash
que build.que \
    --allow read=src,Cargo.toml \
    --allow write=target \
    --allow net=crates.io,static.crates.io \
    --allow exec
```

Path scopes are prefixes, resolved against the working directory and cleaned
lexically, so `..` cannot be used to climb out:

```bash
que t.que --allow read=src        # src/main.que  ok
                                  # src/../etc    denied
```

The path is cleaned *without* touching the filesystem, which is what lets a
`write` grant cover a file that does not exist yet.

Host scopes match the host exactly or as a suffix beginning with a dot, so
`--allow net=example.com` covers `api.example.com` but not `evil-example.com`.

`--deny` is the inverse, for when the script is broadly trusted and one thing
is not:

```bash
que vendored.que --deny net       # everything except the network
```

A denial names the flag that would have permitted it:

```
runtime error: permission denied: exec 'curl https://example.com' —
  grant it with --allow exec=curl https://example.com (or --allow exec for all)
```

Two deliberate choices are worth knowing about.

**Enforcement lives at dispatch, not at the call sites.** Que has four ways to
reach the outside world — std-module functions, global builtins, `Path`
methods, and subprocesses — and each is checked at its single dispatch point.
Sprinkling checks across a hundred filesystem calls would guarantee that the
hundred-and-first is missed, and a sandbox with a hole in it is worse than no
sandbox because it invites trust it has not earned.

**An unclassified function is denied.** Every std function and every global
builtin is classified in a table, a test asserts the tables are complete, and
an unclassified name is refused. If someone adds a builtin and forgets to
classify it, the first person to run it under a policy gets a clear error
rather than an unnoticed hole.

The sandbox constrains *Que's* effects. It is not a kernel-level jail: it
cannot stop a subprocess you granted `exec` from doing whatever it likes. Use
it to bound honest mistakes and to make a script's reach explicit — for
hostile code, use a container or a real sandbox as well.

---

# Part V — Wrapping Up

## 33. Inspection, Reflection, and the REPL

### `dbg` — Quick-and-Dirty Debugger

`dbg()` prints a debug summary and **returns the value unchanged**, so you can
drop it anywhere — even inside a pipeline:

```que
let result = [1, 2, 3]
    .map(|x| x * 2)
    |> dbg                // prints: [dbg] List = List(len=3)
    .filter(|x| x > 2)
    |> dbg                // prints: [dbg] List = List(len=2)

println(result)           // [4, 6]
```

### `inspect` — Detailed Introspection

`.inspect()` returns a map describing a value in detail:

```que
let info = [1, 2, 3].inspect()
println(info["type"])          // List
println(info["length"])        // 3
println(info["homogeneous"])   // true
println(info["element_type"])  // Int
```

Available as a method on every type.

### Type Queries

The questions a *value* can answer about itself are methods. The ones about
the program around it live in `std.reflect`:

```que
import std.reflect

typeof(42)                 // "Int"
typeof([1, 2])             // "List"

let info = reflect.type_info(|x| x + 1)
println(info.is_callable)  // true
println(info.type_name)    // "Function"

42.is_type("Int")          // true
```

### Method Reflection

```que
"hello".methods()              // ["len", "to_upper", "to_lower", ...]

reflect.has_method([1, 2], "map")      // true
reflect.has_method([1, 2], "to_upper") // false
```

### Structural Reflection

`reflect.fields()` returns keys, indices, or parameter names:

```que
reflect.fields({"a": 1, "b": 2})       // ["a", "b"]
reflect.fields((10, 20, 30))           // [0, 1, 2]

fn greet(name, greeting) { greeting + ", " + name }
reflect.fields(greet)                  // ["name", "greeting"]
```

### Environment Reflection

```que
let x = 42
let name = "que"
reflect.vars()                    // {"name": "que", "x": 42}

mut counter = 0
let info = reflect.var_info("counter")
println(info.name)       // "counter"
println(info.type_name)  // "Int"
println(info.mutable)    // true

reflect.scope_depth()    // 0  (global)
```

### Universal Methods

Four methods are available on **every** Que value:

| Method | Description |
|--------|-------------|
| `.inspect()` | Detailed introspection map |
| `.methods()` | List of available method names |
| `.type_name()` | Type as a string |
| `.is_type(name)` | Type-check predicate |

### `help` — Interactive Documentation

`help()` is the entry point for discovering what's available — especially
useful in the REPL where you may not remember an exact builtin or method name.

```que
help()                    // overview: builtin categories, std modules,
                          //   local packages, REPL meta-commands
help("println")           // signature and docs for the `println` builtin
help("map")               // documentation for the `.map()` method
help("if")                // documentation for a language keyword
help("fs")                // list the functions exported by std module `fs`

help(42)                  // type + available methods, grouped
help([1, 2, 3])           // same, for a List

help(stream.file("data.txt"))  // describes a stream value
help(json)                // lists exports of an imported module

// User-defined types — pass the type name or any instance.
struct Point { x = 0, y = 0 }
impl Point {
    fn distance() { (self.x * self.x + self.y * self.y).abs() }
}
help(Point)               // shows struct fields + impl methods
help(Point{x: 3, y: 4})   // same, plus current field values
```

`help` always prints to stdout and returns `null`, so it never clutters the
REPL with a `=> ...` echo.

### Listing Available Modules

`reflect.modules()` returns a Map of every bundled standard-library module to
the list of bare function names it exports:

```que
import std.reflect

reflect.modules()
// {"fs": ["read", "write", "exists", ...],
//  "http": ["get", "post", ...],
//  "json": ["parse", "stringify", "edit"], ... }

reflect.modules()["fs"]     // ["read", "write", "exists", ...]
reflect.modules().keys()    // ["archive", "config", "csv", "dotenv", "fs", ...]
```

For a formatted view with import hints, use `help("<module>")` (or `?<module>`
in the REPL):

```text
que> ?fs
Standard library module `fs`

Import with:
  import fs
  import fs { read }

Functions (12):
  fs.read, fs.write, fs.exists, fs.atomic_write, fs.temp_file, fs.temp_dir
  fs.copy_dir, fs.remove_dir, fs.find, fs.read_lines, fs.write_lines
  fs.transform
```

`help()` (no arguments) also lists any user packages found in
`<package-root>/que_packages/`, so a fresh REPL gives you an immediate
overview of what's importable in the current project.

### REPL Meta-Commands

In an interactive `que` session, any line starting with `:` or `?` is a
shortcut that's rewritten before being evaluated as Que code:

| Shortcut | Equivalent | Purpose |
|----------|------------|---------|
| `?` / `:h` / `:help` | `help()` | Show the overview |
| `?expr` / `:h expr` | `help(expr)` | Inspect a value or a name |
| `:t expr` | `typeof(expr)` | Print the type name |
| `:m expr` | `expr.methods()` | List available methods |
| `:i expr` | `expr.inspect()` | Detailed introspection map |
| `:v` / `:vars` | `reflect.vars()` | List user-defined variables |
| `:load file.que` | *(reads + evaluates the file)* | Bring code into the session |
| `:r` / `:reset` | *(fresh interpreter)* | Clear all bindings |
| `:q` / `:exit` | *(exits the REPL)* | Same as Ctrl-D |

### Tab Completion

The REPL has context-aware tab completion. Press `<Tab>` while typing to:

- complete **meta-commands** after `:` (`:v<Tab>` → `:vars`),
- complete **identifiers, builtins, keywords, types, and std module names**
  on a bare prefix (`prin<Tab>` → `println`),
- complete **module members** after a dot — both standard-library modules
  (`fs.re<Tab>` → `read`, `read_lines`, `remove_dir`) and modules brought
  into scope with `import` (`json.par<Tab>` → `parse`),
- complete **importable module names** after the `import` keyword
  (`import js<Tab>` → `import json`), including any packages found in
  `que_packages/`,
- complete the identifier after `?` (`?prin<Tab>` → `?println`).

Example session:

```text
que> let xs = [1, 2, 3]
que> ?xs
Value of type `List`
  = [1, 2, 3]

Methods (28):
  len, push, pop, map, filter, fold, ...
que> :t xs.first()
=> Int
```

---

## 34. A Real-World Deployment Pipeline

Every feature so far has been introduced in isolation. This chapter puts them
together into one realistic artifact: a `Quefile` that checks, builds,
containerizes, and deploys a small internal API service to a fleet of
servers over SSH, with structured logging, a health-check gate, and an
automatic rollback if the new version doesn't come up healthy.

Nothing here is exotic — this is close to what a real platform team's
`Quefile` looks like once a service graduates from "runs on my laptop" to
"has an on-call rotation."

### The scenario

`ferry` is a small HTTP API. Its pipeline needs to:

1. Run lint, unit tests, and a type check — in parallel, because none of them
   depend on each other.
2. Build a container image tagged with the git commit and push it to a
   registry.
3. Deploy the new image to each host in a fleet, one at a time, over SSH.
4. Poll each host's `/health` endpoint after the restart; if any host doesn't
   come up healthy within the timeout, roll every already-updated host back
   to the previous image and fail loudly.
5. Record a checksum of the release manifest so a later audit can prove
   exactly what was deployed.

### `deploy.toml` — the release manifest

```toml
# deploy.toml
[service]
name = "ferry"
health_path = "/health"
port = 8080

[fleet]
hosts = ["api-1.internal", "api-2.internal", "api-3.internal"]
user = "deploy"

[registry]
host = "registry.example.com"
repo = "acme/ferry"
```

### `Quefile`

```que
// Quefile
import std.config
import std.container
import std.ssh
import std.net
import std.git
import std.hash
import std.log
import std.time

let manifest = config.read(script_dir() / "deploy.toml").unwrap()
let service  = manifest.service.name
let hosts    = manifest.fleet.hosts
let registry = "${manifest.registry.host}/${manifest.registry.repo}"

let logger = log.new({ service: service })

// ---- Stage 1: checks, run concurrently ------------------------------

@description("Lint, test, and type-check — all three run in parallel")
task verify {
    if git.is_dirty() {
        fail("working tree is dirty — commit or stash before deploying")
    }

    let results = parallel {
        lint:      `que lint`.try(),
        test:      `que test`.try(),
        typecheck: `que --strict src/main.que`.try(),
    }

    for (name, r) in results {
        if r.exit_code != 0 {
            fail("${name} failed:\n${r.stdout}${r.stderr}")
        }
    }
    logger.info("verification passed")
}

// ---- Stage 2: build and push the image -------------------------------

@description("Build and tag the container image with the git commit")
@deps([verify])
task build {
    let tag = "${registry}:${git.short_commit()}"
    container.build({
        tag: tag,
        context: ".",
        build_args: { "GIT_SHA": git.commit() },
    })?
    logger.info("image built", { tag: tag })
}

@description("Push the image to the registry")
@deps([build])
task push {
    let tag = "${registry}:${git.short_commit()}"
    let user = env.get("REGISTRY_USER") ?? fail("REGISTRY_USER not set")
    let pass = env.secret("REGISTRY_PASSWORD").unwrap()

    container.login(manifest.registry.host, user, pass)?
    container.push(tag)?
    logger.info("image pushed", { tag: tag })
}

// ---- Stage 3: roll the fleet, one host at a time ----------------------

fn current_tag(host) {
    ssh.out(host, "docker inspect --format='{{.Config.Image}}' ${service}",
        { user: manifest.fleet.user }).unwrap_or(null)
}

fn deploy_to(host, tag) {
    logger.info("deploying", { host: host, tag: tag })

    ssh.cmd(host, `docker pull ${tag}`, { user: manifest.fleet.user })
        .timeout(2m)
        .run()

    ssh.cmd(host,
        `docker run -d --rm --name ${service} -p ${str(manifest.service.port)}:${str(manifest.service.port)} ${tag}`,
        { user: manifest.fleet.user }
    ).run()

    let url = "http://${host}:${manifest.service.port}${manifest.service.health_path}"
    retry(5, |attempt| {
        sleep(2s)
        net.wait_for_url(url, 20s, 2s)?
    })
    logger.info("host healthy", { host: host })
}

fn rollback(host, previous_tag) {
    if previous_tag == null {
        logger.warn("no previous image recorded, cannot roll back", { host: host })
        return
    }
    logger.warn("rolling back", { host: host, tag: previous_tag })
    ssh.cmd(host, `docker stop ${service}`, { user: manifest.fleet.user }).try()
    ssh.cmd(host,
        `docker run -d --rm --name ${service} -p ${str(manifest.service.port)}:${str(manifest.service.port)} ${previous_tag}`,
        { user: manifest.fleet.user }
    ).run()
}

@description("Roll the new image out to every host, one at a time")
@deps([push])
task deploy {
    let tag = "${registry}:${git.short_commit()}"
    mut rolled_back = []

    for host in hosts {
        let previous = current_tag(host)

        try {
            deploy_to(host, tag)
        } catch e {
            logger.error("deploy failed, rolling back this host and every host already updated",
                { host: host, error: e })
            rollback(host, previous)
            for done_host in rolled_back {
                rollback(done_host, previous)
            }
            fail("deploy aborted: ${e}")
        }

        rolled_back = rolled_back.push(host)
    }

    logger.info("fleet fully deployed", { tag: tag, hosts: hosts.len() })
}

// ---- Stage 4: record what shipped -------------------------------------

@description("Record a signed record of this release")
@deps([deploy])
@outputs(["./releases/${git.short_commit()}.json"])
task record {
    let record = {
        service: service,
        version: git.tag() ?? "0.0.0-dev+${git.short_commit()}",
        commit: git.commit(),
        hosts: hosts,
        deployed_at: time.now().to_iso(),
        manifest_sha256: hash.sha256(script_dir() / "deploy.toml"),
    }
    path("./releases").mkdir()
    config.write(path("./releases/${git.short_commit()}.json"), record)
    logger.info("release recorded", record)
}

@description("Full pipeline: verify, build, push, deploy, record")
@deps([record])
task release {
    println("✓ ${service} ${git.tag() ?? git.short_commit()} deployed to ${hosts.len()} hosts")
}
```

Run it with:

```sh
que run release -- \
    2>&1 | tee release.log
```

Or gate it behind a sandbox, since a deploy task is exactly the kind of thing
you want to double-check the reach of before it runs unattended in CI:

```sh
que run release \
    --allow read \
    --allow write=releases \
    --allow exec \
    --allow net=registry.example.com,api-1.internal,api-2.internal,api-3.internal \
    --allow env=REGISTRY_USER,REGISTRY_PASSWORD
```

And preview it before trusting it on a Friday afternoon:

```sh
que run release --dry-run
```

### What this example is actually demonstrating

- **`parallel` for independent checks** ([Section 18](#18-background-processes-and-parallel-execution)) —
  lint, test, and typecheck share no state, so they run concurrently and the
  task fails fast on whichever finds a problem first, with deterministic
  error ordering.
- **`@deps` builds the pipeline as a graph, not a script** ([Section 30](#30-tasks-and-the-build-graph)) —
  running `que run record` alone re-runs `verify` → `build` → `push` →
  `deploy` in order; you never have to remember the sequence by hand.
- **`defer`-free rollback, on purpose.** `defer` unwinds one function's scope;
  a rollback here needs to reach back across *already-completed* iterations
  of a loop, so it's modeled explicitly as a `rolled_back` list and a
  `try`/`catch` around each host instead. Reach for `defer` when cleanup is
  local to a scope ([Section 11](#11-error-handling)); reach for an explicit
  list when it isn't.
- **Secrets never touch a log line or the command line** ([Section 16](#16-regex-and-secrets)) —
  `env.secret()` returns a `Secret`, and `container.login` takes it directly
  rather than shelling out with the password inline.
- **`retry` absorbs the ordinary flakiness of "container just started"**
  ([Section 19](#19-resilience--retry-and-timeout)) instead of a hand-rolled
  polling loop.
- **`ssh.cmd` is a `Cmd`** ([Section 28](#28-containers-and-remote-execution)), so
  `.timeout()` from [Section 17](#17-running-commands) works on it exactly
  like it would on a local command.
- **The task graph is inert until you run it** — `que run release --dry-run`
  walks the same graph and prints what each stage *would* do
  ([Section 32](#32-strict-mode-dry-runs-and-sandboxing)), which is the last
  check before a real fleet rollout.
- **`--allow` makes the blast radius of "deploy to prod" explicit in the CI
  job definition itself**, not just in the script — anyone reviewing the CI
  config can see exactly which hosts and which env vars this pipeline can
  touch, without reading the Que source.

---

## 35. Language Grammar Summary

### Keywords

```
let  mut  fn  pub  task  struct  enum  impl  trait  type
if  else  match  for  in  while  loop
return  break  continue
import  as
true  false  null
defer  try  catch  finally
with  spawn  parallel
```

`env`, `sleep`, and `strict()` are *not* reserved words — `env` is an
ordinary identifier bound to a builtin namespace object, and `sleep`/`strict`
are ordinary builtin functions. You could shadow any of them with a `let`,
though there is rarely a reason to.

Conversely, `where` and `from` *are* reserved by the lexer even though
nothing in the grammar currently uses them — so `let where = ...` is a parse
error today, and both names are worth avoiding as variable names to stay
compatible with what they're reserved for.

### Type Aliases

`type` declares a name for a type expression:

```que
type UserId = Int
type Handler = Function
```

It parses at the top level but currently has no runtime effect, and it does
not yet feed [strict-mode](#32-strict-mode-dry-runs-and-sandboxing) checking
— annotating a parameter with an alias (`id: UserId`) makes strict mode
reject every call, including ones passing a real `Int`, because `UserId` is
not one of the recognized built-in type names. Until that's wired up, use
`type` for documentation only, and annotate parameters you want strict mode
to actually enforce with the built-in names from the
[strict-mode table](#32-strict-mode-dry-runs-and-sandboxing) directly.

### Operators (by precedence, low to high)

| Operator | Description |
|----------|-------------|
| `\|>` | Pipe |
| `??` | Null coalescing |
| `?.` | Optional chaining |
| `\|\|` | Logical or |
| `&&` | Logical and |
| `\|` | Bitwise or |
| `^` | Bitwise xor |
| `&` | Bitwise and |
| `==` `!=` | Equality |
| `<` `>` `<=` `>=` | Comparison |
| `<<` `>>` | Bit shift |
| `..` `..=` | Range |
| `+` `-` | Addition / subtraction |
| `*` `/` `%` | Multiplication / division / modulo |
| `**` | Power |
| `!` `-` `~` | Unary (not, negate, bitnot) |
| `.` `()` `[]` `?` | Postfix (access, call, index, try) |

### Literal Forms

| Syntax | Type |
|--------|------|
| `42`, `0xFF`, `0o755` | Int |
| `3.14` | Float |
| `"hello"`, `"x = ${expr}"` | String (interpolated) |
| `r"raw"`, `r#"raw"#` | Raw string |
| `"""triple"""` | Triple-quoted string |
| `true` / `false` | Bool |
| `null` | Null |
| `[1, 2, 3]` | List |
| `{ "k": v }` | Map |
| `(a, b)` | Tuple |
| `#{1, 2, 3}` | Set |
| `p"/usr/local"`, `p"./config"` | Path literal |
| `p"/opt/${app}/bin"` | Path literal with interpolation |
| `path("./p")`, `path("~/config")` | Path (runtime, expands `~`) |
| `g"*.log"`, `g"/var/**/*.log"` | Glob literal |
| `g"/etc/${app}/*.conf"` | Glob literal with interpolation |
| `glob("*.rs")`, `glob(path_val)` | Glob (runtime / from Path) |
| `` `command` `` | Cmd (lazy) |
| `30s`, `500ms`, `2m`, `1h`, `7d` | Duration |
| `v"1.2.3"` | Semver |
| `re"pattern"` | Regex |
| `secret("val")` | Secret |
| `Ok(v)`, `Err(e)` | Result |

### Statement Forms

```
let x = expr
mut x = expr
let [a, ...b] = expr
let { key, ...rest } = expr
fn name(params) { body }
pub fn name(params) { body }
@deps([...])
task name { body }
struct Name { field: Type = default }
enum Name { Variant, DataVariant { field: Type } }
impl Name { methods }
trait Name { methods }
impl Trait for Name { methods }
import .module
import std.module { name1, name2 }
for x in expr { body }
while cond { body }
loop { body }
return expr
break expr
continue
defer expr
try { } catch e { } finally { }
with Expr as name { body }
```

### Expression Forms

```
if cond { } else if cond { } else { }
match expr { pattern => expr, ... }
|params| expr
|params| { body }
fn(params) { body }
parallel { expr, expr }
parallel { name: expr, name: expr }
spawn `cmd`
{ block }
expr |> fn
expr..expr
expr..=expr
```

### Pattern Forms

```
_                           // wildcard
42, "str", true, null       // literal
x                           // binding
[a, b, ...rest]             // list
{ key, ...rest }            // map
(a, b)                      // tuple
Ok(p), Err(p)               // result
StructName { field, ... }   // struct instance
Variant { field, ... }      // enum variant (named fields)
Variant {}                  // unit enum variant
EnumName.variant            // qualified unit enum variant
EnumName.variant(p)         // qualified positional enum variant
EnumName.variant { field }  // qualified named-field enum variant
glob("*.rs")                // glob
p1 | p2                     // or-pattern
x @ pattern                 // binding with sub-pattern
pattern if guard            // guard
```

---

## 36. Quick Reference

### CLI Commands

| Command | Description |
|---------|-------------|
| `que` | Start interactive REPL |
| `que <script.que>` | Run a script file |
| `que run <task>` | Run a task from a Quefile (with dependencies) |
| `que run <task> --help` | Show task usage and argument table |
| `que run <task> -- arg1 arg2` | Run task with positional arguments |
| `que run <task> -- key=val` | Run task with named arguments |
| `que run -f <file> <task>` | Run task from a specific file |
| `que run -g <task>` | Run task from the global Quefile (`~/.que/Quefile`) |
| `que tasks` | List available tasks |
| `que tasks -f <file>` | List tasks in a specific file |
| `que tasks -g` | List tasks in the global Quefile |
| `que fmt` | Format source files |
| `que fmt --check` | Check formatting (CI) |
| `que lint` | Lint source files |
| `que test` | Run the test suite |
| `que install` | Fetch dependencies declared in `que.toml` |
| `que install --locked` | Fetch, but fail on anything not already pinned |
| `que --strict <script.que>` | Run with type annotations enforced |
| `que <script.que> --dry-run` | Preview effects without performing them |
| `que <script.que> --allow <cap>` | Run sandboxed, granting one capability |
| `que help` | Show help message |

### Built-in Functions (Global)

| Function | Description |
|----------|-------------|
| `println(...)` | Print values with newline |
| `print(...)` | Print values without newline |
| `typeof(x)` | Type name as string |
| `str(x)` | Convert to string |
| `int(x)` | Convert to integer |
| `float(x)` | Convert to float |
| `bool(x)` | Convert to boolean |
| `abs(x)` | Absolute value |
| `min(a, b)` | Minimum of two values |
| `max(a, b)` | Maximum of two values |
| `range(a, b)` | List of integers `a` to `b` (exclusive) |
| `chr(n)` / `ord(c)` | Code point to character and back |
| `Ok(val)` | Wrap as success Result |
| `Err(val)` | Wrap as error Result |
| `env.get(key, default?)` | Read environment variable |
| `input(prompt?)` | Read a line from stdin (prompt to stderr) |
| `confirm(prompt?)` | Ask a yes/no question, returns `Bool` |
| `open(path, mode?)` | Open file (returns FileHandle) |
| `path(str)` | Create a typed Path |
| `path.home()` | Home directory as Path |
| `script_dir()` / `quefile_dir()` | Directory of the running script, as Path |
| `cd(dir)` | Change the process working directory, returns the old one |
| `dir(path)` | A directory change scoped to a `with` block |
| `glob(pattern)` | Create a typed Glob |
| `regex(str)` | Create a Regex from a runtime string (returns `Result`) |
| `secret(str)` | Create a Secret value |
| `assert(cond, msg?)` | Assert condition; reports the expression and its values on failure |
| `fail(msg, code?)` | Raise a fatal error, optionally pinning the exit code |
| `dry_run()` | True when the run was started with `--dry-run` |
| `sleep(duration)` | Pause execution |
| `compose(f, g)` | Return `\|x\| f(g(x))` |
| `retry(n, fn)` | Retry closure up to n times |
| `timeout(dur, fn)` | Run with time limit |
| `semver_parse(str)` | Parse semver string (returns `Result`) |
| `which(name)` | Find executable on PATH |
| `dbg(x)` | Debug-print and pass through |
| `strict()` | Report whether strict mode is on (set by `#!strict` or `--strict`) |
| `tasks()` | Map of all tasks |
| `run_task(name, args...)` | Execute task by name |
| `args()` | List of command-line arguments after the script name |

### Standard Library Modules

| Module | Functions |
|--------|-----------|
| `std.fs` | `read`, `write`, `exists`, `atomic_write`, `temp_file`, `temp_dir`, `copy_dir`, `remove_dir`, `find`, `read_lines`, `write_lines`, `transform`, `read_secret` |
| `std.json` | `parse`, `stringify`, `edit` |
| `std.yaml` | `parse`, `stringify`, `edit` |
| `std.toml` | `parse`, `stringify`, `edit` |
| `std.http` | `get`, `post`, `put`, `patch`, `delete`, `request`, `download`, `upload`, `url_encode`, `url_decode`, `query_string` |
| `std.archive` | `tar_gz`, `zip`, `extract`, `list` |
| `std.hash` | `sha256`, `sha512`, `md5`, `verify`, `write_checksums`, `verify_checksums` |
| `std.csv` | `parse`, `parse_str`, `write`, `to_string` |
| `std.log` | `debug`, `info`, `warn`, `error`, `set_level`, `set_format`, `add_file_sink`, `add_console_sink`, `remove_sinks`, `new` |
| `std.git` | `branch`, `commit`, `short_commit`, `tag`, `tags`, `is_dirty`, `is_clean`, `remote_url` |
| `std.dotenv` | `load`, `load_overwrite`, `parse`, `write` |
| `std.template` | `render`, `render_file` |
| `std.time` | `now`, `timestamp`, `of`, `parse`, `from_timestamp`, `timezone` |
| `std.net` | `ping`, `port_open`, `resolve`, `wait_for_port`, `wait_for_url` |
| `std.stream` | `file`, `of`, `stdout`, `stderr`, `stdin` |
| `std.config` | `read`, `write` |
| `std.reflect` | `type_info`, `fields`, `has_method`, `vars`, `var_info`, `scope_depth`, `modules` |
| `std.container` | `engine`, `build`, `run`, `exec`, `stop`, `remove`, `logs`, `exists`, `is_running`, `wait_healthy`, `pull`, `push`, `login` |
| `std.ssh` | `cmd`, `run`, `out`, `check`, `upload`, `download` |
| `std.watch` | `run`, `wait`, `snapshot` |
| `std.tty` | `is_stdin`, `is_stdout`, `is_stderr`, `size`, `supports_ansi` |
| `std.prompt` | `read_line`, `read_key` |

### The `os` Object

| Field | Description |
|-------|-------------|
| `os.name` | OS name (`"linux"`, `"macos"`, `"windows"`) |
| `os.family` | OS family (`"unix"` or `"windows"`) |
| `os.arch` | CPU architecture |
| `os.path_separator` | Path-list separator (`:` or `;`) |
| `os.dir_separator` | Directory separator (`/` or `\`) |
| `os.exit(code?)` | Exit the process |

---

Happy que-ing!
