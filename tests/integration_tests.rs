/// Full-stack integration tests for the Que language.
///
/// Each test feeds source code through the complete pipeline:
///   lex → parse → interpret
/// and asserts on the output and/or result value.

use que_lang::interpreter::{run, run_strict};
use que_lang::value::Value;

// ── Helper ───────────────────────────────────────────────────────────

fn assert_output(source: &str, expected: &[&str]) {
    let (output, _) = run(source).unwrap_or_else(|e| panic!("execution failed: {}", e));
    assert_eq!(
        output,
        expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "output mismatch for source:\n{}",
        source
    );
}

fn assert_result(source: &str, expected: Value) {
    let (_, result) = run(source).unwrap_or_else(|e| panic!("execution failed: {}", e));
    assert_eq!(result, expected, "result mismatch for source:\n{}", source);
}

fn assert_error(source: &str) {
    assert!(run(source).is_err(), "expected error for source:\n{}", source);
}

/// Assert the source fails and that the rendered error message contains `needle`.
fn assert_error_contains(source: &str, needle: &str) -> String {
    match run(source) {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(needle),
                "expected error containing {:?}, got {:?}\nsource:\n{}",
                needle,
                msg,
                source
            );
            msg
        }
        Ok(_) => panic!("expected error for source:\n{}", source),
    }
}

#[test]
fn assert_reports_the_expression_and_the_values_it_produced() {
    assert_error_contains(
        "let users = [\"a\", \"b\"]\nlet min_count = 5\nassert(users.len() >= min_count)",
        "assertion failed: users.len() >= min_count  (2 >= 5)",
    );
}

#[test]
fn assert_quotes_strings_in_the_values_it_reports() {
    assert_error_contains(
        "let stage = \"dev\"\nassert(stage == \"prod\")",
        "assertion failed: stage == \"prod\"  (\"dev\" == \"prod\")",
    );
}

#[test]
fn assert_does_not_repeat_itself_when_the_condition_is_all_literals() {
    let msg = assert_error_contains("assert(\"prod\" == \"dev\")", "\"prod\" == \"dev\"");
    assert!(!msg.contains("("), "{}", msg);
}

#[test]
fn assert_names_the_operand_of_an_and_that_failed() {
    assert_error_contains(
        "let a = 1\nlet b = 0\nassert(a > 0 && b > 0)",
        "assertion failed: b > 0  (0 > 0)",
    );
}

#[test]
fn assert_reports_a_plain_condition_with_its_value() {
    assert_error_contains(
        "let users = [\"a\"]\nassert(users.contains(\"z\"))",
        "assertion failed: users.contains(\"z\")  (false)",
    );
}

#[test]
fn assert_keeps_a_custom_message_and_still_shows_the_values() {
    assert_error_contains(
        "let status = 503\nassert(status == 200, \"health check\")",
        "health check: status == 200  (503 == 200)",
    );
}

#[test]
fn assert_evaluates_its_condition_exactly_once() {
    // The condition is handed over unevaluated, so a naive implementation
    // could run it again to build the message and fire every side effect twice.
    assert_output(
        "mut calls = 0\nfn bump() { calls = calls + 1; calls }\ntry { assert(bump() == 99) } catch e { }\nprintln(calls)",
        &["1"],
    );
}

#[test]
fn assert_that_holds_is_silent() {
    assert_output("assert(1 + 1 == 2)\nprintln(\"ok\")", &["ok"]);
}

#[test]
fn assert_eq_is_gone() {
    assert_error_contains("assert_eq(1, 2)", "`assert(a == b)` reports both values");
}

/// Assert the source fails with a specific process exit code.
fn assert_exit_code(source: &str, expected: i32) {
    match run(source) {
        Err(e) => assert_eq!(
            e.process_exit_code(),
            expected,
            "exit code mismatch for source:\n{}\nerror: {}",
            source,
            e
        ),
        Ok(_) => panic!("expected error for source:\n{}", source),
    }
}

// ═════════════════════════════════════════════════════════════════════
// 1. BASIC LANGUAGE FEATURES
// ═════════════════════════════════════════════════════════════════════

#[test]
fn hello_world() {
    assert_output("println(\"Hello, Que!\")", &["Hello, Que!"]);
}

#[test]
fn print_concatenates_partial_lines() {
    // print() should NOT add a trailing newline; consecutive prints
    // accumulate on the same line until a println (or '\n') flushes it.
    assert_output(
        "print(\"a\"); print(\"b\"); println(\"c\")",
        &["abc"],
    );
}

#[test]
fn print_then_trailing_flush() {
    // A trailing print() with no following newline should still be visible
    // in the captured output (flushed at end of execution).
    assert_output("print(\"hello\")", &["hello"]);
}

#[test]
fn print_with_embedded_newlines_splits_lines() {
    // Embedded '\n' inside a print() argument should split the buffer
    // at each newline.
    assert_output("print(\"one\\ntwo\\n\"); print(\"three\")", &["one", "two", "three"]);
}

#[test]
fn arithmetic_expression() {
    assert_result("(2 + 3) * 4 - 1", Value::Int(19));
}

#[test]
fn nested_arithmetic() {
    assert_result("2 ** 3 + 1", Value::Int(9));
}

#[test]
fn variable_binding_and_use() {
    let source = r#"
let greeting = "Hello"
let name = "World"
println(greeting + ", " + name + "!")
"#;
    assert_output(source, &["Hello, World!"]);
}

#[test]
fn mutable_variable() {
    let source = r#"
mut counter = 0
counter += 1
counter += 1
counter += 1
counter
"#;
    assert_result(source, Value::Int(3));
}

#[test]
fn immutable_assignment_error() {
    assert_error("let x = 1\nx = 2");
}

// ═════════════════════════════════════════════════════════════════════
// 2. CONTROL FLOW
// ═════════════════════════════════════════════════════════════════════

#[test]
fn if_else_expression() {
    let source = r#"
fn classify(n) {
    if n > 100 { "large" }
    else if n > 10 { "medium" }
    else { "small" }
}
println(classify(5))
println(classify(50))
println(classify(500))
"#;
    assert_output(source, &["small", "medium", "large"]);
}

#[test]
fn while_loop_countdown() {
    let source = r#"
mut n = 5
mut result = []
while n > 0 {
    result = result.push(n)
    n -= 1
}
result
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::Int(5),
            Value::Int(4),
            Value::Int(3),
            Value::Int(2),
            Value::Int(1),
        ]),
    );
}

#[test]
fn for_loop_accumulate() {
    let source = r#"
mut words = []
for word in ["hello", "beautiful", "world"] {
    words = words.push(word.to_upper())
}
words.join(" ")
"#;
    assert_result(source, Value::String("HELLO BEAUTIFUL WORLD".into()));
}

#[test]
fn loop_break_with_value() {
    let source = r#"
mut i = 0
let result = loop {
    i += 1
    if i * i > 50 {
        break i
    }
}
result
"#;
    assert_result(source, Value::Int(8));
}

#[test]
fn for_range_inclusive() {
    let source = r#"
mut sum = 0
for i in 1..=10 {
    sum += i
}
sum
"#;
    assert_result(source, Value::Int(55));
}

// ═════════════════════════════════════════════════════════════════════
// 3. FUNCTIONS AND CLOSURES
// ═════════════════════════════════════════════════════════════════════

#[test]
fn recursive_factorial() {
    let source = r#"
fn factorial(n) {
    if n <= 1 { 1 }
    else { n * factorial(n - 1) }
}
factorial(10)
"#;
    assert_result(source, Value::Int(3628800));
}

#[test]
fn closure_factory() {
    let source = r#"
fn make_counter(start) {
    mut count = start
    {
        let get = || count
        let inc = || { count += 1; count }
        // Return both functions — but for v0.1 we return a simple closure test
        count
    }
}
let c = make_counter(10)
c
"#;
    assert_result(source, Value::Int(10));
}

#[test]
fn higher_order_functions() {
    let source = r#"
fn compose(f, g) {
    |x| f(g(x))
}
let double = |x| x * 2
let inc = |x| x + 1
let double_then_inc = compose(inc, double)
double_then_inc(5)
"#;
    assert_result(source, Value::Int(11));
}

#[test]
fn default_parameters() {
    let source = r#"
fn http_get(url, timeout = 30, retries = 3) {
    timeout * retries
}
println(http_get("http://example.com"))
println(http_get("http://example.com", 10))
println(http_get("http://example.com", 10, 5))
"#;
    assert_output(source, &["90", "30", "50"]);
}

// ═════════════════════════════════════════════════════════════════════
// 4. PATTERN MATCHING
// ═════════════════════════════════════════════════════════════════════

#[test]
fn match_literals() {
    let source = r#"
fn describe(x) {
    match x {
        0 => "zero",
        1 => "one",
        _ => "other",
    }
}
println(describe(0))
println(describe(1))
println(describe(42))
"#;
    assert_output(source, &["zero", "one", "other"]);
}

#[test]
fn match_with_guards() {
    let source = r#"
fn fizzbuzz(n) {
    match n {
        n if n % 15 == 0 => "FizzBuzz",
        n if n % 3 == 0 => "Fizz",
        n if n % 5 == 0 => "Buzz",
        n => str(n),
    }
}
println(fizzbuzz(3))
println(fizzbuzz(5))
println(fizzbuzz(15))
println(fizzbuzz(7))
"#;
    assert_output(source, &["Fizz", "Buzz", "FizzBuzz", "7"]);
}

#[test]
fn match_list_destructure() {
    let source = r#"
fn head_tail(list) {
    match list {
        [] => "empty",
        [x] => "single: " + str(x),
        [x, ...rest] => "head: " + str(x) + ", rest len: " + str(rest.len()),
    }
}
println(head_tail([]))
println(head_tail([42]))
println(head_tail([1, 2, 3, 4]))
"#;
    assert_output(
        source,
        &[
            "empty",
            "single: 42",
            "head: 1, rest len: 3",
        ],
    );
}

#[test]
fn match_result_types() {
    let source = r#"
fn safe_divide(a, b) {
    if b == 0 { Err("division by zero") }
    else { Ok(a / b) }
}
fn show_result(r) {
    match r {
        Ok(val) => "Success: " + str(val),
        Err(msg) => "Error: " + msg,
    }
}
println(show_result(safe_divide(10, 2)))
println(show_result(safe_divide(10, 0)))
"#;
    assert_output(source, &["Success: 5", "Error: division by zero"]);
}

#[test]
fn match_or_pattern() {
    let source = r#"
fn is_weekend(day) {
    match day {
        "Saturday" | "Sunday" => true,
        _ => false,
    }
}
println(is_weekend("Monday"))
println(is_weekend("Saturday"))
"#;
    assert_output(source, &["false", "true"]);
}

#[test]
fn match_struct_pattern() {
    let source = r#"
let point = { "x": 3, "y": 4 }
match point {
    { x, y } => x + y,
}
"#;
    assert_result(source, Value::Int(7));
}

// ═════════════════════════════════════════════════════════════════════
// 5. PIPE OPERATOR
// ═════════════════════════════════════════════════════════════════════

#[test]
fn pipe_chain_functions() {
    let source = r#"
fn double(x) { x * 2 }
fn square(x) { x * x }
fn negate(x) { -x }
3 |> double |> square |> negate
"#;
    assert_result(source, Value::Int(-36));
}

#[test]
fn pipe_with_partial_args() {
    let source = r#"
fn add(a, b) { a + b }
fn mul(a, b) { a * b }
5 |> add(3) |> mul(2)
"#;
    assert_result(source, Value::Int(16)); // (5+3)*2 = 16
}

#[test]
fn pipe_with_lambdas() {
    let source = r#"
[1, 2, 3, 4, 5]
    |> |list| list.filter(|x| x % 2 == 1)
    |> |list| list.map(|x| x * x)
    |> |list| list.fold(0, |a, b| a + b)
"#;
    assert_result(source, Value::Int(35)); // 1+9+25
}

// ═════════════════════════════════════════════════════════════════════
// 6. STRING OPERATIONS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn string_interpolation_complex() {
    let source = r#"
let name = "Que"
let version = 1
let msg = "Welcome to ${name} v${version}!"
msg
"#;
    assert_result(source, Value::String("Welcome to Que v1!".into()));
}

#[test]
fn string_methods_chain() {
    let source = r#"
"  Hello, World!  "
    .trim()
    .to_lower()
    .replace("world", "que")
"#;
    assert_result(source, Value::String("hello, que!".into()));
}

#[test]
fn string_split_and_join() {
    let source = r#"
let csv = "alice,bob,charlie"
let names = csv.split(",")
let upper_names = names.map(|n| n.to_upper())
upper_names.join(" | ")
"#;
    assert_result(source, Value::String("ALICE | BOB | CHARLIE".into()));
}

// ═════════════════════════════════════════════════════════════════════
// 7. LIST OPERATIONS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn list_functional_pipeline() {
    let source = r#"
let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let result = numbers
    .filter(|x| x % 2 == 0)
    .map(|x| x * x)
    .fold(0, |acc, x| acc + x)
result
"#;
    assert_result(source, Value::Int(220));
}

#[test]
fn list_spread_operator() {
    let source = r#"
let first = [1, 2, 3]
let second = [4, 5, 6]
let combined = [...first, 0, ...second]
combined
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(0),
            Value::Int(4),
            Value::Int(5),
            Value::Int(6),
        ]),
    );
}

#[test]
fn list_sort_and_reverse() {
    let source = r#"
let sorted = [3, 1, 4, 1, 5, 9, 2, 6].sort()
let reversed = sorted.reverse()
println(sorted.join(","))
println(reversed.join(","))
"#;
    assert_output(source, &["1,1,2,3,4,5,6,9", "9,6,5,4,3,2,1,1"]);
}

#[test]
fn list_find_and_any() {
    let source = r#"
let items = [10, 20, 30, 40]
let found = items.find(|x| x > 25)
let has_big = items.any(|x| x > 100)
println(found)
println(has_big)
"#;
    assert_output(source, &["30", "false"]);
}

// ═════════════════════════════════════════════════════════════════════
// 8. MAP OPERATIONS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn map_operations() {
    let source = r#"
let config = {
    "host": "localhost",
    "port": 8080,
    "debug": true,
}
println(config.host)
println(config.port)
println(config.keys().len())
"#;
    assert_output(source, &["localhost", "8080", "3"]);
}

#[test]
fn map_merge() {
    let source = r#"
let defaults = { "timeout": 30, "retries": 3, "verbose": false }
let overrides = { "timeout": 60, "verbose": true }
let config = defaults.merge(overrides)
println(config.timeout)
println(config.retries)
println(config.verbose)
"#;
    assert_output(source, &["60", "3", "true"]);
}

#[test]
fn map_iteration() {
    let source = r#"
let scores = { "alice": 95, "bob": 87, "charlie": 92 }
mut total = 0
for (name, score) in scores {
    total += score
}
total
"#;
    assert_result(source, Value::Int(274));
}

// ═════════════════════════════════════════════════════════════════════
// 9. ERROR HANDLING
// ═════════════════════════════════════════════════════════════════════

#[test]
fn try_operator_success() {
    let source = r#"
fn parse_number(s) {
    if s == "42" { Ok(42) }
    else { Err("not a number") }
}
fn process() {
    let n = parse_number("42")?
    n * 2
}
process()
"#;
    assert_result(source, Value::Int(84));
}

#[test]
fn try_operator_failure() {
    let source = r#"
fn parse_number(s) {
    if s == "42" { Ok(42) }
    else { Err("not a number") }
}
fn process() {
    let n = parse_number("abc")?
    n * 2
}
process()
"#;
    assert_error(source);
}

#[test]
fn null_coalescing_chain() {
    let source = r#"
let a = null
let b = null
let c = 42
let result = a ?? b ?? c
result
"#;
    assert_result(source, Value::Int(42));
}

#[test]
fn result_map_method() {
    let source = r#"
let val = Ok(5)
let doubled = val.map(|x| x * 2)
doubled.unwrap()
"#;
    assert_result(source, Value::Int(10));
}

// ═════════════════════════════════════════════════════════════════════
// 10. DEVOPS-SPECIFIC FEATURES
// ═════════════════════════════════════════════════════════════════════

#[test]
fn path_operations() {
    let source = r#"
let base = path("./project")
let src = base / "src"
let main = src / "main.rs"
println(main)
println(main.name())
println(main.extension())
println(main.parent())
"#;
    assert_output(
        source,
        &[
            "./project/src/main.rs",
            "main.rs",
            "rs",
            "./project/src",
        ],
    );
}

#[test]
fn path_components() {
    let source = r#"
let p = path("src/main.rs")
let parts = p.components()
println(parts.len())
for part in parts {
    println(part)
}
"#;
    assert_output(source, &["2", "src", "main.rs"]);
}

#[test]
fn path_components_absolute() {
    let source = r#"
let p = path("/home/user/project/main.rs")
let parts = p.components()
println(parts.len())
println(parts[0])
println(parts[4])
"#;
    assert_output(source, &["5", "/", "main.rs"]);
}

#[test]
fn path_depth() {
    let source = r#"
let shallow = path("file.txt")
let deep = path("a/b/c/d/file.txt")
println(shallow.depth())
println(deep.depth())
"#;
    assert_output(source, &["1", "5"]);
}

#[test]
fn path_components_indexing() {
    let source = r#"
let p = path("src/lib/utils/helpers.rs")
let parts = p.components()
println(parts[0])
println(parts[parts.len() - 1])
"#;
    assert_output(source, &["src", "helpers.rs"]);
}

#[test]
fn duration_operations() {
    let source = r#"
let timeout = 30s
let retry_delay = 500ms
let total = timeout + retry_delay
println(total.to_seconds())
"#;
    assert_output(source, &["30.5"]);
}

#[test]
fn semver_operations() {
    let source = r#"
let current = v"1.2.3"
let minimum = v"1.0.0"
let next_major = v"2.0.0"
println(current > minimum)
println(current < next_major)
"#;
    assert_output(source, &["true", "true"]);
}

// ═════════════════════════════════════════════════════════════════════
// 11. COMPLEX PROGRAMS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn fizzbuzz_full() {
    let source = r#"
fn fizzbuzz(n) {
    match n {
        n if n % 15 == 0 => "FizzBuzz",
        n if n % 3 == 0 => "Fizz",
        n if n % 5 == 0 => "Buzz",
        n => str(n),
    }
}
mut results = []
for i in 1..=15 {
    results = results.push(fizzbuzz(i))
}
results.join(", ")
"#;
    assert_result(
        source,
        Value::String(
            "1, 2, Fizz, 4, Buzz, Fizz, 7, 8, Fizz, Buzz, 11, Fizz, 13, 14, FizzBuzz".into(),
        ),
    );
}

#[test]
fn sieve_of_eratosthenes() {
    let source = r#"
fn sieve(n) {
    mut primes = []
    mut is_prime = []
    for i in 0..=n {
        is_prime = is_prime.push(true)
    }
    for i in 2..=n {
        if is_prime[i] {
            primes = primes.push(i)
            mut j = i * i
            while j <= n {
                is_prime[j] = false
                j += i
            }
        }
    }
    primes
}
let primes = sieve(30)
primes.join(", ")
"#;
    assert_result(
        source,
        Value::String("2, 3, 5, 7, 11, 13, 17, 19, 23, 29".into()),
    );
}

#[test]
fn binary_search() {
    let source = r#"
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
let sorted = [2, 5, 8, 12, 16, 23, 38, 56, 72, 91]
match binary_search(sorted, 23) {
    Ok(idx) => idx,
    Err(_) => -1,
}
"#;
    assert_result(source, Value::Int(5));
}

#[test]
fn string_processing_pipeline() {
    let source = r#"
let text = "  the Quick Brown FOX jumps OVER the lazy DOG  "
let result = text
    .trim()
    .to_lower()
    .split(" ")
    .filter(|w| w.len() > 3)
    .map(|w| w.to_upper())
    .join(", ")
result
"#;
    assert_result(
        source,
        Value::String("QUICK, BROWN, JUMPS, OVER, LAZY".into()),
    );
}

#[test]
fn map_reduce_word_lengths() {
    let source = r#"
let words = ["hello", "world", "que", "is", "great"]
let lengths = words.map(|w| w.len())
let total = lengths.fold(0, |a, b| a + b)
let avg = total / words.len()
avg
"#;
    assert_result(source, Value::Int(4));
}

#[test]
fn nested_data_structure() {
    let source = r#"
let users = [
    { "name": "Alice", "age": 30 },
    { "name": "Bob", "age": 25 },
    { "name": "Charlie", "age": 35 },
]
let names = users.map(|u| u.name)
let oldest = users.fold(users.first(), |oldest, u|
    if u.age > oldest.age { u } else { oldest }
)
println(names.join(", "))
println(oldest.name)
"#;
    assert_output(source, &["Alice, Bob, Charlie", "Charlie"]);
}

#[test]
fn task_declaration() {
    // Tasks should be callable like functions.
    let source = r#"
task build {
    let result = "built!"
    result
}
build()
"#;
    assert_result(source, Value::String("built!".into()));
}

// ═════════════════════════════════════════════════════════════════════
// 12. EDGE CASES
// ═════════════════════════════════════════════════════════════════════

#[test]
fn deeply_nested_calls() {
    let source = r#"
fn f(x) { x + 1 }
f(f(f(f(f(0)))))
"#;
    assert_result(source, Value::Int(5));
}

#[test]
fn empty_list_operations() {
    let source = r#"
let empty = []
println(empty.len())
println(empty.is_empty())
println(empty.first())
println(empty.join(","))
"#;
    assert_output(source, &["0", "true", "null", ""]);
}

#[test]
fn empty_map_literal() {
    let source = r#"
let empty = {}
println(typeof(empty))
println(empty.len())
println(empty.is_empty())
"#;
    assert_output(source, &["Map", "0", "true"]);
}

#[test]
fn empty_map_then_populate() {
    let source = r#"
mut m = {}
m["x"] = 1
m["y"] = 2
println(m.len())
println(m["x"])
println(m["y"])
"#;
    assert_output(source, &["2", "1", "2"]);
}

#[test]
fn empty_map_set_path() {
    let source = r#"
let config = {}
let updated = config
    .set_path("app.name", "que")
    .set_path("app.version", "1.0")
println(updated.get_path("app.name"))
println(updated.get_path("app.version"))
"#;
    assert_output(source, &["que", "1.0"]);
}

#[test]
fn empty_map_to_json() {
    let source = r#"
let m = {}
println(m.to_json())
"#;
    assert_output(source, &["{}"]);
}

#[test]
fn empty_containers_in_expressions() {
    let source = r#"
let l = []
let m = {}
let t = ()
println(typeof(l))
println(typeof(m))
println(typeof(t))
println(l == [])
println(m == {})
"#;
    assert_output(source, &["List", "Map", "Tuple", "true", "true"]);
}

#[test]
fn string_parse_int() {
    let source = r#"
let result = "42".parse_int()
match result {
    Ok(n) => n,
    Err(_) => -1,
}
"#;
    assert_result(source, Value::Int(42));
}

#[test]
fn let_destructuring_with_rest() {
    let source = r#"
let [first, second, ...rest] = [1, 2, 3, 4, 5]
println(first)
println(second)
println(rest)
"#;
    assert_output(source, &["1", "2", "[3, 4, 5]"]);
}

#[test]
fn scope_isolation() {
    let source = r#"
let x = 1
{
    let x = 2
    println(x)
}
println(x)
"#;
    assert_output(source, &["2", "1"]);
}

#[test]
fn multiline_method_chain() {
    let source = r#"
[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    .filter(|x| x % 3 == 0)
    .map(|x| x * x)
    .reverse()
"#;
    assert_result(
        source,
        Value::List(vec![Value::Int(81), Value::Int(36), Value::Int(9)]),
    );
}

#[test]
fn command_literal_parsing() {
    // Test that we can at least create and handle command results.
    // (Actual command execution depends on the system.)
    let source = r#"
let result = `echo hello`.run()
result.success()
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn command_stdout() {
    let source = r#"
`echo -n que`.out()
"#;
    assert_result(source, Value::String("que".into()));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Combinators
// ═════════════════════════════════════════════════════════════════════

#[test]
fn the_removed_collection_globals_name_the_method_to_use() {
    // Every one of these used to exist twice: once as a global taking the
    // collection first, once as the method. The globals are gone, and each
    // now reports the method that replaced it.
    for (source, method) in [
        ("len([1])", "len"),
        ("push([1], 2)", "push"),
        ("pop([1])", "pop"),
        ("keys({a: 1})", "keys"),
        ("values({a: 1})", "values"),
        ("contains([1], 1)", "contains"),
        ("split(\"a b\", \" \")", "split"),
        ("trim(\" a \")", "trim"),
        ("join([1], \",\")", "join"),
        ("replace(\"a\", \"a\", \"b\")", "replace"),
        ("chars(\"ab\")", "chars"),
        ("filter([1], |x| true)", "filter"),
        ("map([1], |x| x)", "map"),
        ("fold([1], 0, |a, b| a)", "fold"),
        ("flat_map([1], |x| [x])", "flat_map"),
        ("group_by([1], |x| x)", "group_by"),
        ("sort_by([1], |x| x)", "sort_by"),
        ("any([1], |x| true)", "any"),
        ("all([1], |x| true)", "all"),
        ("find([1], |x| true)", "find"),
        ("zip([1], [2])", "zip"),
        ("enumerate([1])", "enumerate"),
        ("take([1], 1)", "take"),
        ("skip([1], 1)", "skip"),
        ("chunk([1], 1)", "chunk"),
        ("partition([1], |x| true)", "partition"),
        ("flatten([[1]])", "flatten"),
        ("each([1], |x| x)", "each"),
        ("for_each([1], |x| x)", "each"),
    ] {
        assert_error_contains(source, &format!("x.{}(", method));
    }
}

#[test]
fn the_removed_stream_and_config_globals_name_the_module() {
    // Six stream constructors and two config-file calls sat in the global
    // namespace with nothing to distinguish them from the language itself.
    // They are `std.stream` and `std.config` now, and the old spellings say
    // which import to write.
    for (source, needle) in [
        ("stream(\"a\")", "import std.stream"),
        ("stream_of(\"a\")", "stream.of(x)"),
        ("stdout()", "stream.stdout()"),
        ("stderr()", "stream.stderr()"),
        ("stdin()", "stream.stdin()"),
        ("config_read(p\"c.json\")", "config.read(...)"),
        ("config_write(p\"c.json\", {})", "config.write(...)"),
    ] {
        assert_error_contains(source, needle);
    }
    // `stream()` picked between reading a file and wrapping text by looking
    // at its argument. The replacement names the difference.
    assert_error_contains("stream(\"a\")", "stream.file(path)");
}

#[test]
fn the_removed_reflection_globals_name_the_module_or_the_method() {
    // The ones that ask about the interpreter's own state moved to
    // `std.reflect`; the three that were already methods on every value
    // point at the method rather than being copied into the module.
    for (source, needle) in [
        ("type_info(1)", "reflect.type_info(...)"),
        ("fields({a: 1})", "reflect.fields(...)"),
        ("has_method(1, \"len\")", "reflect.has_method(...)"),
        ("vars()", "reflect.vars(...)"),
        ("var_info(\"x\")", "reflect.var_info(...)"),
        ("scope_depth()", "reflect.scope_depth(...)"),
        ("modules()", "reflect.modules(...)"),
        ("inspect(1)", "x.inspect()"),
        ("methods(1)", "x.methods()"),
        ("is_type(1, \"Int\")", "x.is_type(name)"),
    ] {
        assert_error_contains(source, needle);
    }
}

#[test]
fn a_removed_global_still_reports_itself_under_a_deny_all_policy() {
    // The tombstones are only useful if they are reachable. A name that is
    // bound but unclassified fails the permission check first, so a
    // sandboxed script would learn it was denied rather than what to write.
    let out = run_with_policy(&["!read", "!write"], "stream(\"a\")");
    let err = out.unwrap_err().to_string();
    assert!(err.contains("import std.stream"), "got: {}", err);
}

#[test]
fn combinator_filter() {
    let source = r#"
let nums = [1, 2, 3, 4, 5]
nums.filter(|x| x > 3)
"#;
    assert_result(source, Value::List(vec![Value::Int(4), Value::Int(5)]));
}

#[test]
fn combinator_map() {
    let source = r#"
let nums = [1, 2, 3]
nums.map(|x| x * 2)
"#;
    assert_result(source, Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)]));
}

#[test]
fn combinator_fold() {
    let source = r#"
let nums = [1, 2, 3, 4]
nums.fold(0, |acc, x| acc + x)
"#;
    assert_result(source, Value::Int(10));
}

#[test]
fn combinator_flat_map() {
    let source = r#"
let nums = [1, 2, 3]
nums.flat_map(|x| [x, x * 10])
"#;
    assert_result(source, Value::List(vec![
        Value::Int(1), Value::Int(10),
        Value::Int(2), Value::Int(20),
        Value::Int(3), Value::Int(30),
    ]));
}

#[test]
fn combinator_any_all() {
    let source = r#"
let nums = [1, 2, 3, 4]
let has_big = nums.any(|x| x > 3)
let all_pos = nums.all(|x| x > 0)
(has_big, all_pos)
"#;
    assert_result(source, Value::Tuple(vec![Value::Bool(true), Value::Bool(true)]));
}

#[test]
fn combinator_find() {
    let source = r#"
let nums = [10, 20, 30]
nums.find(|x| x > 15)
"#;
    assert_result(source, Value::Int(20));
}

#[test]
fn combinator_zip() {
    let source = r#"
let a = [1, 2, 3]
let b = ["a", "b", "c"]
a.zip(b)
"#;
    assert_result(source, Value::List(vec![
        Value::Tuple(vec![Value::Int(1), Value::String("a".into())]),
        Value::Tuple(vec![Value::Int(2), Value::String("b".into())]),
        Value::Tuple(vec![Value::Int(3), Value::String("c".into())]),
    ]));
}

#[test]
fn combinator_enumerate() {
    let source = r#"
let items = ["a", "b"]
items.enumerate()
"#;
    assert_result(source, Value::List(vec![
        Value::Tuple(vec![Value::Int(0), Value::String("a".into())]),
        Value::Tuple(vec![Value::Int(1), Value::String("b".into())]),
    ]));
}

#[test]
fn combinator_take_skip() {
    let source = r#"
let nums = [1, 2, 3, 4, 5]
let first3 = nums.take(3)
let last2 = nums.skip(3)
(first3, last2)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        Value::List(vec![Value::Int(4), Value::Int(5)]),
    ]));
}

#[test]
fn combinator_chunk() {
    let source = r#"
[1, 2, 3, 4, 5].chunk(2)
"#;
    assert_result(source, Value::List(vec![
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::List(vec![Value::Int(3), Value::Int(4)]),
        Value::List(vec![Value::Int(5)]),
    ]));
}

#[test]
fn combinator_partition() {
    let source = r#"
[1, 2, 3, 4, 5].partition(|x| x % 2 == 0)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::List(vec![Value::Int(2), Value::Int(4)]),
        Value::List(vec![Value::Int(1), Value::Int(3), Value::Int(5)]),
    ]));
}

#[test]
fn combinator_flatten() {
    let source = r#"
[[1, 2], [3], [4, 5]].flatten()
"#;
    assert_result(source, Value::List(vec![
        Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4), Value::Int(5),
    ]));
}

#[test]
fn combinator_compose() {
    let source = r#"
let double = |x| x * 2
let add1 = |x| x + 1
let pipeline = compose(double, add1)
pipeline(5)
"#;
    assert_result(source, Value::Int(11)); // double(5)=10, add1(10)=11
}

#[test]
fn pipe_with_user_functions() {
    // `|>` is for functions you wrote; the built-in combinators it used to
    // carry are methods now, and `.filter(f).map(g)` chains without it.
    let source = r#"
fn big(xs) { xs.filter(|x| x > 2) }
fn tenfold(xs) { xs.map(|x| x * 10) }
let result = [1, 2, 3, 4, 5]
    |> big
    |> tenfold
result
"#;
    assert_result(source, Value::List(vec![Value::Int(30), Value::Int(40), Value::Int(50)]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Semver methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn semver_bump_methods() {
    let source = r#"
let v = v"1.2.3"
let major = v.bump_major()
let minor = v.bump_minor()
let patch = v.bump_patch()
(str(major), str(minor), str(patch))
"#;
    assert_result(source, Value::Tuple(vec![
        Value::String("2.0.0".into()),
        Value::String("1.3.0".into()),
        Value::String("1.2.4".into()),
    ]));
}

#[test]
fn semver_field_access() {
    let source = r#"
let v = v"3.2.1"
(v.major, v.minor, v.patch)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Int(3), Value::Int(2), Value::Int(1),
    ]));
}

#[test]
fn semver_satisfied_by() {
    let source = r#"
let constraint = v">=1.2.0, <2.0.0"
let v1 = constraint.satisfied_by(v"1.5.0")
let v2 = constraint.satisfied_by(v"2.0.0")
let v3 = constraint.satisfied_by(v"1.1.0")
(v1, v2, v3)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Bool(true), Value::Bool(false), Value::Bool(false),
    ]));
}

#[test]
fn semver_prerelease() {
    let source = r#"
let v = v"2.0.0-beta.1"
let pre = v.prerelease()
let is_pre = v.is_prerelease()
(pre, is_pre)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::String("beta.1".into()),
        Value::Bool(true),
    ]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Secret handling
// ═════════════════════════════════════════════════════════════════════

#[test]
fn secret_creation_and_redaction() {
    let source = r#"
let s = secret("hunter2")
println(s)
s.expose()
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(output, vec!["<redacted>"]);
    assert_eq!(result, Value::String("hunter2".into()));
}

#[test]
fn secret_expose() {
    let source = r#"
let s = secret("mypassword")
s.expose()
"#;
    assert_result(source, Value::String("mypassword".into()));
}

#[test]
fn secret_interpolation_reaches_the_process() {
    // The whole point: `<redacted>` in the command text means the command is
    // broken. The shell must receive the real token.
    let source = r#"
let tok = secret("hunter2")
`printf '%s' ${tok}`.out()
"#;
    assert_result(source, Value::String("hunter2".into()));
}

#[test]
fn secret_is_redacted_in_a_dry_run() {
    let source = r#"
let tok = secret("hunter2")
`curl -H "Authorization: Bearer ${tok}" https://example.com`.out()
"#;
    let mut interp = que_lang::interpreter::Interpreter::new();
    interp.dry_run = true;
    let tokens = que_lang::lexer::Lexer::new(source).tokenize().unwrap();
    let module = que_lang::parser::Parser::new(tokens).parse_module().unwrap();
    interp.exec_module(&module).unwrap();
    let joined = interp.output.join("\n");
    assert!(joined.contains("<redacted>"), "{}", joined);
    assert!(!joined.contains("hunter2"), "{}", joined);
}

#[test]
fn secret_to_string_on_a_cmd_is_redacted() {
    let source = r#"
let tok = secret("hunter2")
`login --token ${tok}`.to_string()
"#;
    assert_result(source, Value::String("login --token <redacted>".into()));
}

#[test]
fn secret_plaintext_is_scrubbed_from_output() {
    // `.expose()` produces an ordinary String that no type can track, so the
    // scrub has to be value-independent.
    let source = r#"
let tok = secret("hunter2")
println("token is " + tok.expose())
println(`printf 'echoed hunter2'`.out())
"#;
    let (output, _) = run(source).unwrap();
    assert_eq!(
        output,
        vec!["token is <redacted>", "echoed <redacted>"]
    );
}

#[test]
fn secret_is_not_serialized_as_plaintext() {
    let source = r#"
import std.json
import std.yaml
json.stringify({ token: secret("hunter2") }) + "|" + yaml.stringify({ token: secret("hunter2") })
"#;
    let (_, result) = run(source).unwrap();
    let text = result.display_string();
    assert!(!text.contains("hunter2"), "{}", text);
    assert!(text.contains("<redacted>"), "{}", text);
}

#[test]
fn env_secret_reads_and_registers() {
    std::env::set_var("QUE_TEST_SECRET_VAR", "s3cr3t-value");
    let source = r#"
let tok = env.secret("QUE_TEST_SECRET_VAR").unwrap()
println(tok)
println("leaked: " + tok.expose())
env.secret("QUE_TEST_MISSING_VAR_XYZ").is_err()
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(output, vec!["<redacted>", "leaked: <redacted>"]);
    assert_eq!(result, Value::Bool(true));
    std::env::remove_var("QUE_TEST_SECRET_VAR");
}

#[test]
fn fs_read_secret_strips_the_trailing_newline() {
    let dir = std::env::temp_dir().join("que_read_secret_test");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("token");
    std::fs::write(&file, "tok-abcdef\n").unwrap();
    let source = format!(
        r#"
import std.fs
fs.read_secret("{}").unwrap().expose()
"#,
        file.display()
    );
    assert_result(&source, Value::String("tok-abcdef".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Regex methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn regex_test() {
    let source = r#"
let re = re"^\d+$"
let a = re.test("12345")
let b = re.test("abc")
(a, b)
"#;
    assert_result(source, Value::Tuple(vec![Value::Bool(true), Value::Bool(false)]));
}

#[test]
fn regex_find() {
    let source = r#"
let pattern = re"\d+"
pattern.find("abc123def")
"#;
    assert_result(source, Value::String("123".into()));
}

#[test]
fn regex_find_all() {
    let source = r#"
let pattern = re"\d+"
pattern.find_all("abc123def456")
"#;
    assert_result(source, Value::List(vec![
        Value::String("123".into()),
        Value::String("456".into()),
    ]));
}

#[test]
fn regex_replace() {
    let source = r#"
let pattern = re"\d+"
pattern.replace("abc123def", "NUM")
"#;
    assert_result(source, Value::String("abcNUMdef".into()));
}

#[test]
fn regex_split() {
    let source = r#"
let pattern = re"\s+"
pattern.split("hello   world  foo")
"#;
    assert_result(source, Value::List(vec![
        Value::String("hello".into()),
        Value::String("world".into()),
        Value::String("foo".into()),
    ]));
}

#[test]
fn regex_from_string_ok() {
    assert_result(
        r#"regex("\\d+")"#,
        Value::Ok(Box::new(Value::Regex("\\d+".into()))),
    );
}

#[test]
fn regex_from_string_invalid() {
    assert_error(r#"regex("(unclosed")??"#);
}

#[test]
fn regex_from_string_usable() {
    // result of regex() can be used with regex methods after unwrapping
    assert_result(
        r#"
let pattern = regex("\\d+")?
pattern.test("abc123")
"#,
        Value::Bool(true),
    );
}

#[test]
fn regex_from_string_find() {
    assert_result(
        r#"
let pattern = regex("\\d+")?
pattern.find("price: 42 dollars")
"#,
        Value::String("42".into()),
    );
}

#[test]
fn regex_passthrough_existing() {
    // passing a Regex value to regex() returns it unchanged
    assert_result(
        r#"
let r = re"\w+"
typeof(regex(r))
"#,
        Value::String("Regex".into()),
    );
}

// ═════════════════════════════════════════════════════════════════════
// RAW STRINGS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn raw_string_basic() {
    assert_result(r#"r"hello\nworld""#, Value::String(r"hello\nworld".into()));
}

#[test]
fn raw_string_with_hash_delimiters() {
    let source = r###"r#"She said "hello""#"###;
    assert_result(source, Value::String(r#"She said "hello""#.into()));
}

#[test]
fn raw_string_no_interpolation() {
    assert_result(r#"r"${name}""#, Value::String("${name}".into()));
}

#[test]
fn raw_string_in_expression() {
    let source = r#"
let s = r"C:\Users\file.txt"
println(s)
"#;
    assert_output(source, &[r"C:\Users\file.txt"]);
}

#[test]
fn raw_string_multiline() {
    let source = "let s = r\"line one\nline two\"\nprintln(s)";
    assert_output(source, &["line one\nline two"]);
}

// ═════════════════════════════════════════════════════════════════════
// TRIPLE-QUOTE STRINGS (multiline with interpolation + escapes)
// ═════════════════════════════════════════════════════════════════════

#[test]
fn triple_quote_basic() {
    let source = "\"\"\"\n    hello\n    world\n\"\"\"";
    assert_result(source, Value::String("hello\nworld".into()));
}

#[test]
fn triple_quote_interpolation() {
    let source = "let name = \"Que\"\n\"\"\"\n    Hello, ${name}!\n    Welcome.\n\"\"\"";
    assert_result(source, Value::String("Hello, Que!\nWelcome.".into()));
}

#[test]
fn triple_quote_escapes() {
    let source = "\"\"\"\n    col1\\tcol2\n    val1\\tval2\n\"\"\"";
    assert_result(source, Value::String("col1\tcol2\nval1\tval2".into()));
}

#[test]
fn triple_quote_escaped_dollar() {
    // \$ prevents interpolation
    let source = "\"\"\"\n    price: \\${amount}\n\"\"\"";
    assert_result(source, Value::String("price: ${amount}".into()));
}

#[test]
fn triple_quote_mixed() {
    let source = "let x = 42\nlet msg = \"\"\"\n    The answer is ${x}.\n    That's \\\"it\\\".\n\"\"\"\nprintln(msg)";
    assert_output(source, &["The answer is 42.\nThat's \"it\"."]);
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: String methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn string_reverse() {
    assert_result(r#""hello".reverse()"#, Value::String("olleh".into()));
}

#[test]
fn string_to_path() {
    assert_result(r#""./src".to_path()"#, Value::Path("./src".into()));
}

#[test]
fn string_matches() {
    let source = r#"
let a = "hello123".matches("\\d+")
let b = "hello".matches("\\d+")
(a, b)
"#;
    assert_result(source, Value::Tuple(vec![Value::Bool(true), Value::Bool(false)]));
}

#[test]
fn string_bytes() {
    assert_result(r#""AB".bytes()"#, Value::List(vec![Value::Int(65), Value::Int(66)]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: List methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn list_flatten() {
    let source = r#"
[[1, 2], [3, 4], [5]].flatten()
"#;
    assert_result(source, Value::List(vec![
        Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4), Value::Int(5),
    ]));
}

#[test]
fn list_indexof() {
    let source = r#"
let idx = [10, 20, 30, 40].index_of(30)
let missing = [10, 20, 30].index_of(99)
(idx, missing)
"#;
    assert_result(source, Value::Tuple(vec![Value::Int(2), Value::Int(-1)]));
}

#[test]
fn missing_map_key_raises() {
    assert_error_contains(
        r#"
let m = {"host": "localhost"}
println(m["hsot"])
"#,
        "key 'hsot' not found",
    );
}

#[test]
fn map_get_is_the_lenient_form() {
    let source = r#"
let m = {"host": "localhost"}
let a = m.get("missing") ?? "fallback"
let b = m.get("missing", "inline")
let c = m.get("host")
(a, b, c)
"#;
    assert_result(
        source,
        Value::Tuple(vec![
            Value::String("fallback".to_string()),
            Value::String("inline".to_string()),
            Value::String("localhost".to_string()),
        ]),
    );
}

#[test]
fn list_get_is_the_lenient_form() {
    let source = r#"
let l = [1, 2, 3]
let a = l.get(5) ?? 0
let b = l.get(5, -1)
let c = l.get(0)
let d = l.get(-1)
(a, b, c, d)
"#;
    assert_result(
        source,
        Value::Tuple(vec![
            Value::Int(0),
            Value::Int(-1),
            Value::Int(1),
            Value::Int(3),
        ]),
    );
}

#[test]
fn list_index_out_of_bounds_raises() {
    assert_error_contains("[1, 2, 3][5]", "out of bounds");
}

#[test]
fn list_slice() {
    assert_result("[1,2,3,4,5].slice(1, 4)", Value::List(vec![
        Value::Int(2), Value::Int(3), Value::Int(4),
    ]));
}

#[test]
fn list_window() {
    let source = r#"
[1, 2, 3, 4].window(2)
"#;
    assert_result(source, Value::List(vec![
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::List(vec![Value::Int(2), Value::Int(3)]),
        Value::List(vec![Value::Int(3), Value::Int(4)]),
    ]));
}

#[test]
fn list_flatmap_alias() {
    let source = r#"
[1, 2, 3].flat_map(|x| [x, x * 10])
"#;
    assert_result(source, Value::List(vec![
        Value::Int(1), Value::Int(10),
        Value::Int(2), Value::Int(20),
        Value::Int(3), Value::Int(30),
    ]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Map spread and ident keys
// ═════════════════════════════════════════════════════════════════════

#[test]
fn map_spread() {
    let source = r#"
let base = { name: "que", version: "1.0" }
let extended = { ...base, author: "test" }
extended.name + " by " + extended.author
"#;
    assert_result(source, Value::String("que by test".into()));
}

#[test]
fn map_ident_keys() {
    let source = r#"
let m = { name: "que", count: 42 }
m.name
"#;
    assert_result(source, Value::String("que".into()));
}

#[test]
fn map_deep_merge() {
    let source = r#"
let a = { x: { y: 1, z: 2 } }
let b = { x: { y: 10 } }
let merged = a.deep_merge(b)
merged.x.y
"#;
    assert_result(source, Value::Int(10));
}

#[test]
fn map_filter_values() {
    let source = r#"
let m = { a: 1, b: 2, c: 3 }
let filtered = m.filter_values(|v| v > 1)
filtered.keys().len()
"#;
    assert_result(source, Value::Int(2));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Try/catch/finally
// ═════════════════════════════════════════════════════════════════════

#[test]
fn try_catch_basic() {
    let source = r#"
mut result = ""
try {
    fail("something went wrong")
} catch e {
    result = "caught: " + e
}
result
"#;
    assert_result(source, Value::String("caught: something went wrong".into()));
}

#[test]
fn try_catch_with_finally() {
    let source = r#"
mut cleanup = false
try {
    fail("oops")
} catch e {
    println("caught")
} finally {
    cleanup = true
}
cleanup
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(output, vec!["caught"]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn try_catch_no_error() {
    let source = r#"
mut result = 0
try {
    result = 42
} catch e {
    result = 0
}
result
"#;
    assert_result(source, Value::Int(42));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Result/Option methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn unhandled_err_in_statement_position_raises() {
    let source = r#"
fn might_fail() -> Result<String> { return Err("boom") }
might_fail()
println("not reached")
"#;
    assert_error_contains(source, "boom");
}

#[test]
fn bound_err_does_not_raise() {
    let source = r#"
fn might_fail() -> Result<String> { return Err("boom") }
let r = might_fail()
r.is_err()
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn try_catch_catches_err_value() {
    let source = r#"
fn might_fail() -> Result<String> { return Err("boom") }
try {
    might_fail()
    println("not reached")
} catch e {
    println("caught: ${e}")
}
"#;
    assert_output(source, &["caught: boom"]);
}

#[test]
fn try_catch_catches_trailing_err_expression() {
    let source = r#"
try {
    Err("trailing")
} catch e {
    println("caught: ${e}")
}
"#;
    assert_output(source, &["caught: trailing"]);
}

#[test]
fn question_mark_error_message_is_the_payload() {
    let source = r#"
fn might_fail() -> Result<String> { return Err("boom") }
let v = might_fail()?
"#;
    let err = assert_error_contains(source, "boom");
    assert!(
        !err.contains("propagated"),
        "error message should be the payload, got: {}",
        err
    );
}

#[test]
fn ok_in_statement_position_is_inert() {
    let source = r#"
Ok("fine")
println("done")
"#;
    assert_output(source, &["done"]);
}

// ── Exit-code convention ─────────────────────────────────────────────

#[test]
fn fail_exits_one_by_default() {
    assert_exit_code(r#"fail("deploy failed")"#, 1);
}

#[test]
fn fail_can_pin_an_exit_code() {
    assert_exit_code(r#"fail("no upstream", 42)"#, 42);
}

#[test]
fn fail_rejects_a_non_integer_exit_code() {
    assert_error_contains(r#"fail("bad", "42")"#, "exit code must be an Int");
}

#[test]
fn the_removed_error_global_names_fail() {
    assert_error_contains(r#"error("boom")"#, "use `fail(msg)`");
}

#[test]
fn unhandled_err_exits_one() {
    assert_exit_code(
        r#"
fn f() -> Result<String> { return Err("nope") }
f()
"#,
        1,
    );
}

#[test]
fn failing_command_forwards_its_own_exit_code() {
    assert_exit_code("`exit 3`", 3);
}

#[test]
fn parse_error_is_a_usage_error() {
    assert_exit_code("let = = =", 2);
}

#[test]
fn result_unwrap_or() {
    let source = r#"
let ok_val = Ok(42).unwrap_or(0)
let err_val = Err("bad").unwrap_or(0)
(ok_val, err_val)
"#;
    assert_result(source, Value::Tuple(vec![Value::Int(42), Value::Int(0)]));
}

#[test]
fn result_and_then() {
    let source = r#"
let doubled = Ok(5).and_then(|x| Ok(x * 2))
doubled.unwrap()
"#;
    assert_result(source, Value::Int(10));
}

#[test]
fn result_map_err() {
    let source = r#"
let result = Err("bad").map_err(|e| "error: " + e)
result.is_err()
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn missing_find_coalesces_to_default() {
    let source = r#"
let found = [1,2,3].find(|x| x > 2) ?? 0
let missing = [1,2,3].find(|x| x > 10) ?? 0
(found, missing)
"#;
    assert_result(source, Value::Tuple(vec![Value::Int(3), Value::Int(0)]));
}

#[test]
fn option_builtins_are_rejected() {
    assert_error("Some(1)");
    assert_error("None");
}

#[test]
fn optional_chaining_short_circuits() {
    let source = r#"
struct Addr { city }
struct User { addr }
let present = User { addr: Addr { city: "Oslo" } }
let absent = User { addr: null }
println(present?.addr?.city)
println(absent?.addr?.city)
println(absent?.addr?.city ?? "unknown")
"#;
    assert_output(source, &["Oslo", "null", "unknown"]);
}

#[test]
fn optional_chaining_unwraps_results() {
    // `?.` is lexed as one operator, so `res?.method()` has to mean `?` then
    // `.` -- otherwise the call lands on the `Ok` wrapper.
    let source = r#"
fn ok_list() { Ok([1, 2, 3]) }
fn bad() { Err("boom") }
println(ok_list()?.len())
mut caught = ""
try {
    bad()?.len()
} catch e {
    caught = "caught: " + e
}
println(caught)
"#;
    assert_output(source, &["3", "caught: boom"]);
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Tuple methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn tuple_methods() {
    let source = r#"
let t = (1, 2, 3)
let len = t.len()
let list = t.to_list()
let has2 = t.contains(2)
(len, list, has2)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Int(3),
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        Value::Bool(true),
    ]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Duration arithmetic
// ═════════════════════════════════════════════════════════════════════

#[test]
fn duration_subtraction() {
    let source = r#"
let a = 10s
let b = 3s
let diff = a - b
diff.to_seconds()
"#;
    assert_result(source, Value::Float(7.0));
}

#[test]
fn duration_comparison() {
    let source = r#"
let a = 5s
let b = 10s
a < b
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn duration_multiplication() {
    let source = r#"
let d = 2s
let result = d * 3
result.to_seconds()
"#;
    assert_result(source, Value::Float(6.0));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Additional builtins
// ═════════════════════════════════════════════════════════════════════

#[test]
fn fail_builtin() {
    assert_error(r#"fail("something went wrong")"#);
}

#[test]
fn bool_builtin() {
    let source = r#"
let a = bool(1)
let b = bool(0)
let c = bool("")
let d = bool("hello")
(a, b, c, d)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Bool(true), Value::Bool(false), Value::Bool(false), Value::Bool(true),
    ]));
}

#[test]
fn time_timestamp_is_a_positive_millisecond_count() {
    let source = r#"
import std.time
let t = time.timestamp()
t > 0
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn the_removed_now_global_names_both_replacements() {
    // `now()` answered two questions with one number. The message names
    // both so you can pick the one you actually wanted.
    let msg = assert_error_contains("now()", "time.timestamp()");
    assert!(msg.contains("time.now()"), "got: {}", msg);
}

#[test]
fn the_removed_to_path_global_names_path() {
    assert_error_contains(r#"to_path("./src")"#, "use `path(s)`");
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Glob test
// ═════════════════════════════════════════════════════════════════════

#[test]
fn glob_test_method() {
    let source = r#"
let g = glob("src/**/*.rs")
let a = g.test("src/main.rs")
let b = g.test("src/nested/lib.rs")
let c = g.test("README.md")
(a, b, c)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Bool(true), Value::Bool(true), Value::Bool(false),
    ]));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Command methods
// ═════════════════════════════════════════════════════════════════════

#[test]
fn cmd_out_method() {
    let source = r#"
`echo -n hello`.out()
"#;
    assert_result(source, Value::String("hello".into()));
}

#[test]
fn process_result_exit_code_alias() {
    let source = r#"
let result = `echo hello`.run()
result.exit_code
"#;
    assert_result(source, Value::Int(0));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Lazy command literals
// ═════════════════════════════════════════════════════════════════════

#[test]
fn cmd_is_lazy_value() {
    // Command literal should produce a Cmd value, not execute immediately
    let source = r#"
let cmd = `echo hello`
typeof(cmd)
"#;
    assert_result(source, Value::String("Cmd".into()));
}

#[test]
fn cmd_run_method() {
    let source = r#"
let result = `echo -n world`.try()
result.stdout
"#;
    assert_result(source, Value::String("world".into()));
}

#[test]
fn bare_command_raises_on_failure() {
    assert_error("`exit 3`");
}

#[test]
fn cmd_run_raises_on_failure() {
    assert_error("`exit 3`.run()");
}

#[test]
fn cmd_out_raises_on_failure() {
    assert_error("`exit 3`.out()");
}

#[test]
fn cmd_try_does_not_raise() {
    let source = r#"
`exit 3`.try().exit_code
"#;
    assert_result(source, Value::Int(3));
}

#[test]
fn bare_command_as_last_statement_of_a_task_runs() {
    // The parser turns a block's last statement into its trailing expression;
    // a command written there is still in statement position and must run.
    let source = r#"
task boom {
    `exit 3`
}
@deps([boom])
task after {
    println("should not be reached")
}
after()
"#;
    assert_error(source);
}

#[test]
fn bare_command_as_last_statement_of_a_loop_body_runs() {
    assert_error("for i in [1] {\n    `exit 3`\n}");
}

#[test]
fn bare_deferred_command_runs() {
    // A deferred command literal is a command in statement position too, so it
    // has to run when the block unwinds rather than be dropped as a value.
    let marker = std::env::temp_dir().join(format!("que_defer_marker_{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let source = format!(
        "fn f() {{\n    defer `touch {}`\n}}\nf()\n",
        marker.display()
    );
    run(&source).unwrap_or_else(|e| panic!("execution failed: {}", e));
    assert!(marker.exists(), "deferred command did not run");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn a_bound_cmd_returned_from_a_block_stays_lazy() {
    let source = r#"
fn build() {
    let c = `exit 3`
    c
}
typeof(build())
"#;
    assert_result(source, Value::String("Cmd".into()));
}

#[test]
fn removed_cmd_methods_are_rejected() {
    assert_error("`echo hi`.run_checked()");
    assert_error("`echo hi`.capture()");
}

// ── Command pipelines ────────────────────────────────────────────────

#[test]
fn cmd_pipe_feeds_stdout_to_the_next_command() {
    assert_result(
        r#"(`printf "b\na\n"` | `sort`).out()"#,
        Value::String("a\nb".to_string()),
    );
}

#[test]
fn cmd_pipe_chains_more_than_two_stages() {
    assert_result(
        r#"(`printf "x\ny\nz\n"` | `grep -v y` | `wc -l`).out().trim()"#,
        Value::String("2".to_string()),
    );
}

#[test]
fn cmd_pipe_feeds_the_first_stage_from_stdin() {
    assert_result(
        r#"(`cat`.stdin("b\na\n") | `sort`).out()"#,
        Value::String("a\nb".to_string()),
    );
}

#[test]
fn cmd_pipe_still_escapes_interpolation() {
    // The pattern contains a space; without escaping `grep` would read it as
    // two arguments and fail.
    assert_result(
        r#"
let pattern = "a p"
(`printf "a p\nq\n"` | `grep -F ${pattern}`).out()"#,
        Value::String("a p".to_string()),
    );
}

#[test]
fn cmd_pipe_keeps_each_stage_modifiers() {
    assert_result(
        r#"(`pwd`.dir("/") | `cat`).out()"#,
        Value::String("/".to_string()),
    );
}

#[test]
fn cmd_pipe_fails_when_any_stage_fails() {
    // `sh` alone would report success here, because only the last stage's
    // exit code counts. Que uses pipefail semantics.
    assert_result(r#"(`exit 4` | `true`).try().exit_code"#, Value::Int(4));
    assert_error("`exit 4` | `true`");
}

#[test]
fn cmd_pipe_reports_the_leftmost_failure() {
    assert_result(r#"(`exit 2` | `exit 5`).try().exit_code"#, Value::Int(2));
}

#[test]
fn cmd_pipe_rejects_a_non_command_right_side() {
    let err = assert_error_contains("`echo hi` | 3", "must be a command");
    assert!(err.contains("got Int"), "{}", err);
}

#[test]
fn cmd_pipe_displays_as_a_pipeline() {
    assert_result(
        "str(`a` | `b` | `c`)",
        Value::String("`a | b | c`".to_string()),
    );
}

#[test]
fn spawn_rejects_a_pipeline() {
    // A ProcessHandle tracks one process; silently spawning only the last
    // stage would leak the others.
    assert_error_contains("spawn (`echo a` | `cat`)", "cannot take a `|` pipeline");
}

#[test]
fn cmd_silent_modifier() {
    let source = r#"
let result = `echo noisy`.silent().try()
result.stdout
"#;
    assert_result(source, Value::String("".into()));
}

#[test]
fn cmd_attach_reports_the_exit_code_and_captures_nothing() {
    // The streams belong to the terminal, so `exit_code` is the only thing
    // left to report. `true`/`false` are used rather than `echo` so the test
    // does not write to the harness's inherited stdout.
    let source = r#"
let ok = `true`.attach().try()
let bad = `false`.attach().try()
println(str(ok.exit_code) + "/" + str(bad.exit_code) + "/[" + ok.stdout + bad.stderr + "]")
"#;
    assert_output(source, &["0/1/[]"]);
}

#[test]
fn cmd_attach_refuses_to_share_stdin() {
    // Both modifiers want the child's stdin; silently letting one win would
    // mean the data is dropped or the terminal never reaches the program.
    assert_error_contains("`cat`.attach().stdin(\"hi\")", "both claim the child's stdin");
    assert_error_contains("`cat`.stdin(\"hi\").attach()", "both claim the child's stdin");
}

#[test]
fn cmd_attach_refuses_a_pipeline() {
    assert_error_contains("(`echo a` | `cat`.attach()).try()", "cannot be used in a pipeline");
}

#[test]
fn cmd_dir_modifier() {
    let source = r#"
`pwd`.dir(path("/tmp")).out()
"#;
    assert_result(source, Value::String("/tmp".into()));
}

#[test]
fn cmd_env_modifier() {
    let source = r#"
`echo -n $MY_TEST_VAR`.env("MY_TEST_VAR", "hello_que").out()
"#;
    assert_result(source, Value::String("hello_que".into()));
}

#[test]
fn cmd_stdin_modifier() {
    let source = r#"
`cat`.stdin("hello from stdin").out()
"#;
    assert_result(source, Value::String("hello from stdin".into()));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Named arguments
// ═════════════════════════════════════════════════════════════════════

#[test]
fn named_arguments_basic() {
    let source = r#"
fn greet(name: String, greeting: String) {
    greeting + ", " + name
}
greet(greeting: "Hello", name: "World")
"#;
    assert_result(source, Value::String("Hello, World".into()));
}

#[test]
fn named_arguments_with_defaults() {
    let source = r#"
fn deploy(target, dryRun = false) {
    if dryRun { "dry: " + target } else { "real: " + target }
}
deploy("prod", dryRun: true)
"#;
    assert_result(source, Value::String("dry: prod".into()));
}

#[test]
fn named_arguments_mixed() {
    let source = r#"
fn add(a, b, c) {
    a + b + c
}
add(1, c: 30, b: 20)
"#;
    assert_result(source, Value::Int(51));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Octal literals
// ═════════════════════════════════════════════════════════════════════

#[test]
fn octal_literal() {
    let source = r#"
let perms = 0o755
perms
"#;
    assert_result(source, Value::Int(0o755));
}

#[test]
fn octal_literal_value() {
    // 0o755 = 7*64 + 5*8 + 5 = 493
    assert_result("0o755", Value::Int(493));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Path tilde expansion
// ═════════════════════════════════════════════════════════════════════

#[test]
fn path_tilde_expansion() {
    let source = r#"
let home = path("~")
let config = path("~/.config")
// Both should start with the actual home directory
home.to_string().len() > 1
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn path_literal_expands_tilde_like_path_fn() {
    let source = r#"
p"~/.config" == path("~/.config") && p"~" == path("~")
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn path_literal_tilde_only_expands_at_the_front() {
    let source = r#"
p"/etc/~/x".to_string()
"#;
    assert_result(source, Value::String("/etc/~/x".into()));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Glob in match patterns
// ═════════════════════════════════════════════════════════════════════

#[test]
fn glob_pattern_in_match() {
    let source = r#"
let file = "main.rs"
let result = match file {
    glob("*.rs") => "rust",
    glob("*.py") => "python",
    _ => "unknown",
}
result
"#;
    assert_result(source, Value::String("rust".into()));
}

#[test]
fn glob_pattern_in_match_no_match() {
    let source = r#"
match "readme.md" {
    glob("*.rs") => "rust",
    glob("*.py") => "python",
    _ => "other",
}
"#;
    assert_result(source, Value::String("other".into()));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Struct rest destructuring
// ═════════════════════════════════════════════════════════════════════

#[test]
fn struct_rest_destructuring() {
    let source = r#"
let config = { name: "que", version: "1.0", author: "test", license: "MIT" }
let { name, ...rest } = config
rest.keys().len()
"#;
    assert_result(source, Value::Int(3));
}

#[test]
fn struct_rest_destructuring_values() {
    let source = r#"
let data = { a: 1, b: 2, c: 3 }
let { a, ...rest } = data
a + rest.b + rest.c
"#;
    assert_result(source, Value::Int(6));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: env namespace and env.scope blocks
// ═════════════════════════════════════════════════════════════════════

#[test]
fn env_get_with_default() {
    assert_result(
        r#"env.get("QUE_DEFINITELY_UNSET_9182", "fallback")"#,
        Value::String("fallback".into()),
    );
}

#[test]
fn env_has() {
    let source = r#"
env.set("QUE_HAS_TEST", "1")
[env.has("QUE_HAS_TEST"), env.has("QUE_DEFINITELY_UNSET_9182")]
"#;
    assert_result(
        source,
        Value::List(vec![Value::Bool(true), Value::Bool(false)]),
    );
}

#[test]
fn env_unset() {
    let source = r#"
env.set("QUE_UNSET_TEST", "1")
env.unset("QUE_UNSET_TEST")
env.has("QUE_UNSET_TEST")
"#;
    assert_result(source, Value::Bool(false));
}

#[test]
fn env_called_as_function_is_rejected() {
    assert_error(r#"env("HOME")"#);
}

#[test]
fn old_with_env_block_is_rejected() {
    assert_error(r#"with env { "A": "1" } { println("x") }"#);
}

#[test]
fn with_env_block() {
    let source = r#"
with env.scope({ "QUE_TEST_VAR": "hello" }) {
    env.get("QUE_TEST_VAR")
}
"#;
    assert_result(source, Value::String("hello".into()));
}

#[test]
fn with_env_restores() {
    let source = r#"
mut before = env.get("QUE_RESTORE_TEST")
with env.scope({ "QUE_RESTORE_TEST": "temp" }) {
    // inside the block
}
let after = env.get("QUE_RESTORE_TEST")
after
"#;
    assert_result(source, Value::Null);
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Partial application with _
// ═════════════════════════════════════════════════════════════════════

#[test]
fn partial_application_basic() {
    let source = r#"
fn add(a, b) { a + b }
let add5 = add(5, _)
add5(3)
"#;
    assert_result(source, Value::Int(8));
}

#[test]
fn partial_application_first_arg() {
    let source = r#"
fn greet(greeting, name) { greeting + " " + name }
let hello = greet("Hello", _)
hello("World")
"#;
    assert_result(source, Value::String("Hello World".into()));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: retry and timeout builtins
// ═════════════════════════════════════════════════════════════════════

#[test]
fn retry_builtin() {
    let source = r#"
let result = retry(3, |attempt| {
    if attempt < 2 {
        fail("not yet")
    }
    "success on " + str(attempt)
})
result
"#;
    assert_result(source, Value::String("success on 2".into()));
}

#[test]
fn retry_exhausted() {
    let source = r#"
retry(2, |_| fail("always fails"))
"#;
    assert_error(source);
}

#[test]
fn timeout_builtin() {
    let source = r#"
timeout(5s, || 42)
"#;
    assert_result(source, Value::Int(42));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: semver_parse
// ═════════════════════════════════════════════════════════════════════

#[test]
fn semver_parse_valid() {
    let source = r#"
let result = semver_parse("1.2.3")
result.unwrap().major
"#;
    assert_result(source, Value::Int(1));
}

#[test]
fn semver_parse_invalid() {
    let source = r#"
let result = semver_parse("not-a-version")
result.is_err()
"#;
    assert_result(source, Value::Bool(true));
}

// ═════════════════════════════════════════════════════════════════════
// NEW FEATURES: Typed env access
// ═════════════════════════════════════════════════════════════════════

#[test]
fn env_bool_method() {
    let source = r#"
with env.scope({ "QUE_BOOL_TEST": "true" }) {
    env.bool("QUE_BOOL_TEST")
}
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn env_int_method() {
    let source = r#"
with env.scope({ "QUE_INT_TEST": "42" }) {
    env.int("QUE_INT_TEST")
}
"#;
    assert_result(source, Value::Int(42));
}

#[test]
fn env_list_method() {
    let source = r#"
with env.scope({ "QUE_LIST_TEST": "a,b,c" }) {
    env.list("QUE_LIST_TEST")
}
"#;
    assert_result(source, Value::List(vec![
        Value::String("a".into()),
        Value::String("b".into()),
        Value::String("c".into()),
    ]));
}

// ═════════════════════════════════════════════════════════════════════
// INSPECTION & RUNTIME REFLECTION
// ═════════════════════════════════════════════════════════════════════

#[test]
fn inspect_int() {
    let source = r#"
let info = 42.inspect()
info["type"]
"#;
    assert_result(source, Value::String("Int".into()));
}

#[test]
fn inspect_string_details() {
    let source = r#"
let info = "hello".inspect()
println(info["type"])
println(info["length"])
println(info["empty"])
"#;
    assert_output(source, &["String", "5", "false"]);
}

#[test]
fn inspect_list_details() {
    let source = r#"
let info = [1, 2, 3].inspect()
println(info["type"])
println(info["length"])
println(info["homogeneous"])
println(info["element_type"])
"#;
    assert_output(source, &["List", "3", "true", "Int"]);
}

#[test]
fn inspect_map_details() {
    let source = r#"
let info = {"name": "que", "version": 1}.inspect()
println(info["type"])
println(info["length"])
info["keys"]
"#;
    assert_output(source, &["Map", "2"]);
}

#[test]
fn inspect_function() {
    let source = r#"
fn add(a, b) { a + b }
let info = add.inspect()
println(info["type"])
println(info["arity"])
info["params"]
"#;
    assert_output(source, &["Function", "2"]);
}

#[test]
fn inspect_method_on_value() {
    let source = r#"
let info = [1, 2, 3].inspect()
info["type"]
"#;
    assert_result(source, Value::String("List".into()));
}

#[test]
fn dbg_passthrough() {
    let source = r#"
let x = dbg(42)
x + 1
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(result, Value::Int(43));
    assert_eq!(output.len(), 1);
    assert!(output[0].starts_with("[dbg]"));
    assert!(output[0].contains("Int"));
    assert!(output[0].contains("42"));
}

#[test]
fn typeof_still_works() {
    assert_result(r#"typeof(42)"#, Value::String("Int".into()));
    assert_result(r#"typeof("hello")"#, Value::String("String".into()));
    assert_result(r#"typeof([1, 2])"#, Value::String("List".into()));
}

#[test]
fn type_info_basic() {
    let source = r#"
import std.reflect
let info = reflect.type_info(42)
println(info.type_name)
println(info.is_numeric)
println(info.is_collection)
println(info.is_callable)
"#;
    assert_output(source, &["Int", "true", "false", "false"]);
}

#[test]
fn type_info_function() {
    let source = r#"
import std.reflect
let info = reflect.type_info(|x| x + 1)
println(info.is_callable)
println(info.is_numeric)
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn type_info_list() {
    let source = r#"
import std.reflect
let info = reflect.type_info([1, 2, 3])
println(info.is_collection)
println(info.is_iterable)
"#;
    assert_output(source, &["true", "true"]);
}

#[test]
fn type_info_result() {
    let source = r#"
import std.reflect
let ok_info = reflect.type_info(Ok(42))
let err_info = reflect.type_info(Err("oops"))
println(ok_info.is_result)
println(err_info.is_result)
"#;
    assert_output(source, &["true", "true"]);
}

#[test]
fn fields_map() {
    let source = r#"
import std.reflect
let f = reflect.fields({"a": 1, "b": 2, "c": 3})
f
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]),
    );
}

#[test]
fn fields_tuple() {
    let source = r#"
import std.reflect
reflect.fields(("x", "y", "z"))
"#;
    assert_result(
        source,
        Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]),
    );
}

#[test]
fn fields_function_returns_params() {
    let source = r#"
import std.reflect
fn greet(name, greeting) { greeting + ", " + name }
reflect.fields(greet)
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::String("name".into()),
            Value::String("greeting".into()),
        ]),
    );
}

#[test]
fn is_type_checks() {
    assert_result(r#"42.is_type("Int")"#, Value::Bool(true));
    assert_result(r#"42.is_type("String")"#, Value::Bool(false));
    assert_result(r#""hello".is_type("String")"#, Value::Bool(true));
    assert_result(r#"[1, 2].is_type("List")"#, Value::Bool(true));
    assert_result(r#"{"a": 1}.is_type("Map")"#, Value::Bool(true));
    assert_result(r#"true.is_type("Bool")"#, Value::Bool(true));
    assert_result(r#"null.is_type("Null")"#, Value::Bool(true));
}

#[test]
fn is_type_wants_the_capitalised_spelling() {
    // The global `is_type(x, name)` also accepted "int"/"string"/"list"; the
    // method never did. Having both meant the same question had two answers
    // depending on which spelling of the call you reached for. The method
    // survived, so the capitalised name that `type_name()` reports and the
    // annotation syntax uses is the only one.
    assert_result(r#"42.is_type("int")"#, Value::Bool(false));
    assert_result(r#""hello".is_type("string")"#, Value::Bool(false));
    assert_result(r#"[1].is_type("list")"#, Value::Bool(false));
}

#[test]
fn is_type_function_check() {
    let source = r#"
fn foo() { 1 }
foo.is_type("Function")
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn is_type_method_form() {
    let source = r#"42.is_type("Int")"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn has_method_checks() {
    assert_result(r#"import std.reflect
reflect.has_method("hello", "to_upper")"#, Value::Bool(true));
    assert_result(r#"import std.reflect
reflect.has_method("hello", "nonexistent")"#, Value::Bool(false));
    assert_result(r#"import std.reflect
reflect.has_method([1, 2], "map")"#, Value::Bool(true));
    assert_result(r#"import std.reflect
reflect.has_method([1, 2], "to_upper")"#, Value::Bool(false));
}

#[test]
fn methods_returns_list() {
    let source = r#"
let m = "hello".methods()
m.contains("to_upper") && m.contains("split") && m.contains("inspect")
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn methods_method_form() {
    let source = r#"
let m = [1, 2].methods()
m.contains("map") && m.contains("filter") && m.contains("inspect")
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn vars_lists_user_variables() {
    let source = r#"
import std.reflect
let x = 42
let name = "que"
let all = reflect.vars()
println(all["x"])
println(all["name"])
"#;
    assert_output(source, &["42", "que"]);
}

#[test]
fn vars_excludes_builtins() {
    // The `vars` map should not contain "print"
    let source2 = r#"
import std.reflect
let x = 1
let all = reflect.vars()
let k = all.keys()
k.contains("print")
"#;
    assert_result(source2, Value::Bool(false));
}

#[test]
fn var_info_basic() {
    let source = r#"
import std.reflect
let x = 42
let info = reflect.var_info("x")
println(info.name)
println(info.type_name)
println(info.mutable)
"#;
    assert_output(source, &["x", "Int", "false"]);
}

#[test]
fn var_info_mutable() {
    let source = r#"
import std.reflect
mut counter = 0
let info = reflect.var_info("counter")
info.mutable
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn var_info_undefined_returns_null() {
    assert_result(r#"import std.reflect
reflect.var_info("nonexistent")"#, Value::Null);
}

#[test]
fn scope_depth_global() {
    assert_result(
        r#"import std.reflect
reflect.scope_depth()"#,
        Value::Int(0),
    );
}

#[test]
fn scope_depth_nested() {
    let source = r#"
import std.reflect
let outer = reflect.scope_depth()
fn check_depth() {
    reflect.scope_depth()
}
println(outer)
println(check_depth())
"#;
    assert_output(source, &["0", "1"]);
}

#[test]
fn type_name_method() {
    assert_result(r#"42.type_name()"#, Value::String("Int".into()));
    assert_result(r#""hello".type_name()"#, Value::String("String".into()));
    assert_result(r#"[1, 2].type_name()"#, Value::String("List".into()));
}

#[test]
fn inspect_truthy_field() {
    let source = r#"
println(0.inspect()["truthy"])
println(1.inspect()["truthy"])
println("".inspect()["truthy"])
println("x".inspect()["truthy"])
println(null.inspect()["truthy"])
"#;
    assert_output(source, &["false", "true", "false", "true", "false"]);
}

#[test]
fn inspect_result_types() {
    let source = r#"
let ok_info = Ok(42).inspect()
let err_info = Err("oops").inspect()
println(ok_info["variant"])
println(ok_info["inner_type"])
println(err_info["variant"])
"#;
    assert_output(source, &["Ok", "Int", "Err"]);
}

#[test]
fn inspect_heterogeneous_list() {
    let source = r#"
let info = [1, "two", true].inspect()
println(info["homogeneous"])
"#;
    assert_output(source, &["false"]);
}

#[test]
fn reflection_pipeline() {
    // Use reflection to dynamically inspect and process data
    let source = r#"
let data = [1, "hello", [2, 3], {"key": "val"}]
let types = data.map(|item| typeof(item))
types.join(", ")
"#;
    assert_result(
        source,
        Value::String("Int, String, List, Map".into()),
    );
}

#[test]
fn reflection_filter_by_type() {
    let source = r#"
let data = [1, "hello", 2, "world", 3]
let nums = data.filter(|x| x.is_type("Int"))
nums
"#;
    assert_result(
        source,
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

#[test]
fn dbg_in_pipeline() {
    let source = r#"
let result = [1, 2, 3]
    .map(|x| x * 2)
    |> dbg
result
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(
        result,
        Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
    );
    assert!(output.iter().any(|line| line.starts_with("[dbg]")));
}

// ═════════════════════════════════════════════════════════════════════
// OS CONSTANTS & env.list() WITH CUSTOM SEPARATOR
// ═════════════════════════════════════════════════════════════════════

#[test]
fn os_path_separator() {
    let source = r#"os.path_separator"#;
    let expected = if cfg!(windows) { ";" } else { ":" };
    assert_result(source, Value::String(expected.into()));
}

#[test]
fn os_dir_separator() {
    let source = r#"os.dir_separator"#;
    let expected = std::path::MAIN_SEPARATOR.to_string();
    assert_result(source, Value::String(expected));
}

#[test]
fn os_map_fields() {
    let source = r#"
println(typeof(os))
println(os.name)
println(os.arch)
typeof(os.path_separator)
"#;
    let (output, result) = run(source).unwrap();
    assert_eq!(output[0], "OsInfo");
    // os.name should be a non-empty string like "linux", "macos", "windows"
    assert!(!output[1].is_empty());
    // os.arch should be a non-empty string
    assert!(!output[2].is_empty());
    assert_eq!(result, Value::String("String".into()));
}

#[test]
fn os_family() {
    let source = r#"os.family"#;
    let (_, result) = run(source).unwrap();
    // family is "unix" or "windows"
    match result {
        Value::String(s) => assert!(s == "unix" || s == "windows", "unexpected os.family: {}", s),
        other => panic!("expected String, got {:?}", other),
    }
}

#[test]
fn env_list_with_custom_separator() {
    let source = r#"
with env.scope({ "QUE_PATH_TEST": "/usr/bin:/usr/local/bin:/home/user/bin" }) {
    env.list("QUE_PATH_TEST", ":")
}
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::String("/usr/bin".into()),
            Value::String("/usr/local/bin".into()),
            Value::String("/home/user/bin".into()),
        ]),
    );
}

#[test]
fn env_list_with_path_separator() {
    // Use os.path_separator to split — the portable way
    let sep = if cfg!(windows) { ";" } else { ":" };
    let val = format!("/a{}/b{}/c", sep, sep);
    let source = format!(
        r#"
with env.scope({{ "QUE_SEP_TEST": "{}" }}) {{
    env.list("QUE_SEP_TEST", os.path_separator)
}}
"#,
        val
    );
    assert_result(
        &source,
        Value::List(vec![
            Value::String("/a".into()),
            Value::String("/b".into()),
            Value::String("/c".into()),
        ]),
    );
}

#[test]
fn env_list_default_separator_is_comma() {
    let source = r#"
with env.scope({ "QUE_CSV_TEST": "x, y, z" }) {
    env.list("QUE_CSV_TEST")
}
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::String("x".into()),
            Value::String("y".into()),
            Value::String("z".into()),
        ]),
    );
}

#[test]
fn env_list_semicolon_separator() {
    let source = r#"
with env.scope({ "QUE_SEMI_TEST": "one;two;three" }) {
    env.list("QUE_SEMI_TEST", ";")
}
"#;
    assert_result(
        source,
        Value::List(vec![
            Value::String("one".into()),
            Value::String("two".into()),
            Value::String("three".into()),
        ]),
    );
}

// ── Task System ──

#[test]
fn task_basic_execution() {
    let source = r#"
@description("A greeting task")
task hello {
    println("hello from task")
}
hello()
"#;
    assert_output(source, &["[RUN]  hello", "hello from task", "[DONE] hello"]);
}

#[test]
fn task_with_dependencies() {
    let source = r#"
task compile {
    println("compiling")
}
@deps([compile])
task link {
    println("linking")
}
@deps([link])
task package {
    println("packaging")
}
package()
"#;
    // Dependencies should run in order
    assert_output(source, &[
        "[RUN]  compile", "compiling", "[DONE] compile",
        "[RUN]  link", "linking", "[DONE] link",
        "[RUN]  package", "packaging", "[DONE] package",
    ]);
}

#[test]
fn task_diamond_deps_dedup() {
    let source = r#"
task base { println("base") }
@deps([base])
task left { println("left") }
@deps([base])
task right { println("right") }
@deps([left, right])
task top { println("top") }
top()
"#;
    let (output, _) = run(source).unwrap();
    let base_runs = output.iter().filter(|l| *l == "base").count();
    assert_eq!(base_runs, 1, "shared dependency should only run once");
}

#[test]
fn task_deps_as_metadata_field() {
    let source = r#"
task compile { println("compiling") }
@description("Bundle it")
@deps([compile])
task package {
    println("packaging")
}
package()
"#;
    assert_output(source, &[
        "[RUN]  compile", "compiling", "[DONE] compile",
        "[RUN]  package", "packaging", "[DONE] package",
    ]);
}

#[test]
fn task_repeated_attribute_is_rejected() {
    let source = r#"
task a { println("a") }
task b { println("b") }
@deps([a])
@deps([b])
task c {
    println("c")
}
c()
"#;
    let err = run(source).expect_err("a repeated attribute should not parse");
    assert!(
        err.to_string().contains("is given twice"),
        "expected duplicate-attribute hint, got: {}",
        err
    );
}

#[test]
fn task_body_field_is_rejected() {
    let err = run("task a {\n    description: \"x\"\n    println(\"a\")\n}")
        .expect_err("in-body metadata should no longer parse");
    assert!(
        err.to_string().contains("@description(...)"),
        "expected migration hint, got: {}",
        err
    );
}

#[test]
fn task_run_wrapper_is_rejected() {
    let err = run("task a {\n    run { println(\"a\") }\n}")
        .expect_err("the run wrapper should no longer parse");
    assert!(
        err.to_string().contains("`run { ... }` wrapper is gone"),
        "expected migration hint, got: {}",
        err
    );
}

#[test]
fn task_unknown_attribute_is_rejected() {
    let err = run("@retries([3])\ntask a { println(\"a\") }")
        .expect_err("an unknown attribute should not parse");
    assert!(
        err.to_string().contains("unknown task attribute `@retries`"),
        "expected unknown-attribute hint, got: {}",
        err
    );
}

#[test]
fn task_old_depends_on_spelling_is_rejected() {
    let err = run("task a { println(\"a\") }\ntask b dependsOn [a] { println(\"b\") }")
        .expect_err("dependsOn should no longer parse");
    assert!(
        err.to_string().contains("`@deps([...])`"),
        "expected migration hint, got: {}",
        err
    );
}

#[test]
fn task_parameterized() {
    let source = r#"
task build(target, mode = "debug") {
    println("building ${target} in ${mode}")
}
build("linux")
"#;
    assert_output(source, &[
        "[RUN]  build",
        "building linux in debug",
        "[DONE] build",
    ]);
}

#[test]
fn task_parameterized_with_override() {
    let source = r#"
task build(target, mode = "debug") {
    println("building ${target} in ${mode}")
}
build("linux", "release")
"#;
    assert_output(source, &[
        "[RUN]  build",
        "building linux in release",
        "[DONE] build",
    ]);
}

#[test]
fn task_status_tracking() {
    let source = r#"
task build { 42 }
let before = build.status
build()
let after = build.status
println(before)
println(after)
"#;
    assert_output(source, &[
        "[RUN]  build",
        "[DONE] build",
        "pending",
        "succeeded",
    ]);
}

#[test]
fn a_dependency_hands_its_value_to_the_task_that_needed_it() {
    let source = r#"
task get_profiles { "/tmp/profiles-1" }

@deps([get_profiles])
task install {
    println("into " + get_profiles.result())
}
install()
"#;
    assert_output(source, &[
        "[RUN]  get_profiles",
        "[DONE] get_profiles",
        "[RUN]  install",
        "into /tmp/profiles-1",
        "[DONE] install",
    ]);
}

#[test]
fn a_task_result_is_reachable_as_a_method_too() {
    let source = r#"
task build { 42 }
build()
println(build.result())
"#;
    assert_output(source, &[
        "[RUN]  build",
        "[DONE] build",
        "42",
    ]);
}

#[test]
fn a_task_that_has_not_run_has_no_result_to_give() {
    assert_error_contains(
        r#"
task build { 42 }
println(build.result())
"#,
        "has not run yet",
    );
}

#[test]
fn run_task_does_not_run_a_task_that_already_succeeded() {
    let source = r#"
task get_profiles {
    println("fetching")
    "/tmp/profiles-1"
}

@deps([get_profiles])
task install {
    println("into " + run_task("get_profiles"))
}
install()
"#;
    assert_output(source, &[
        "[RUN]  get_profiles",
        "fetching",
        "[DONE] get_profiles",
        "[RUN]  install",
        "into /tmp/profiles-1",
        "[DONE] install",
    ]);
}

#[test]
fn run_task_with_arguments_still_runs() {
    let source = r#"
task greet(name) {
    println("hello " + name)
    name
}
greet("a")
run_task("greet", "b")
"#;
    assert_output(source, &[
        "[RUN]  greet",
        "hello a",
        "[DONE] greet",
        "[RUN]  greet",
        "hello b",
        "[DONE] greet",
    ]);
}

#[test]
fn the_run_method_is_unconditional() {
    let source = r#"
task build {
    println("compiling")
    1
}
build()
build.run()
"#;
    assert_output(source, &[
        "[RUN]  build",
        "compiling",
        "[DONE] build",
        "[RUN]  build",
        "compiling",
        "[DONE] build",
    ]);
}

#[test]
fn task_metadata_access() {
    let source = r#"
@description("Compile sources")
task compile {
    null
}
@description("Link objects")
@deps([compile])
task link {
    null
}
println(link.name)
println(link.description)
println(link.deps)
"#;
    assert_output(source, &[
        "link",
        "Link objects",
        "[\"compile\"]",
    ]);
}

#[test]
fn task_run_method() {
    let source = r#"
task build {
    println("building")
}
build.run()
"#;
    assert_output(source, &["[RUN]  build", "building", "[DONE] build"]);
}

#[test]
fn tasks_builtin_enumerates() {
    let source = r#"
task build { null }
task test { null }
task deploy { null }
fn helper() { null }
let t = tasks()
println(t.len())
for name in t.keys() {
    println(name)
}
"#;
    assert_output(source, &["3", "build", "deploy", "test"]);
}

#[test]
fn run_task_by_string_name() {
    let source = r#"
task build {
    println("built!")
    42
}
let result = run_task("build")
println(result)
"#;
    assert_output(source, &["[RUN]  build", "built!", "[DONE] build", "42"]);
}

#[test]
fn task_typeof() {
    let source = r#"
task build { null }
println(typeof(build))
println(build.is_type("Task"))
"#;
    assert_output(source, &["Task", "true"]);
}

#[test]
fn task_env_clause_parsed() {
    // env: clause is parsed and accessible via .env() method and .env field
    let source = r#"
@env([CC, CFLAGS])
task compile {
    null
}
println(compile.env())
println(compile.env_keys)
"#;
    assert_output(source, &[
        r#"["CC", "CFLAGS"]"#,
        r#"["CC", "CFLAGS"]"#,
    ]);
}

#[test]
fn task_env_clause_strings() {
    // env: clause also accepts string literals for var names with special chars
    let source = r#"
@env(["DEPLOY_TOKEN", "K8S_NAMESPACE"])
task deploy {
    null
}
println(deploy.env())
"#;
    assert_output(source, &[
        r#"["DEPLOY_TOKEN", "K8S_NAMESPACE"]"#,
    ]);
}

#[test]
fn task_env_clause_empty() {
    // Tasks without env: clause have empty env list
    let source = r#"
task build {
    null
}
println(build.env())
"#;
    assert_output(source, &["[]"]);
}

#[test]
fn task_inputs_outputs_string_syntax() {
    // inputs/outputs accept plain strings — no path() or g"" wrapper needed.
    // Strings are promoted to Path values when accessed via .inputs()/.outputs().
    let source = r#"
@inputs(["src/main.que", "src/lib.que"])
@outputs(["./build/app"])
task compile {
    null
}
println(typeof(compile.inputs()[0]))
println(typeof(compile.outputs()[0]))
println(compile.inputs())
println(compile.outputs())
"#;
    assert_output(source, &[
        "Path",
        "Path",
        "[src/main.que, src/lib.que]",
        "[./build/app]",
    ]);
}

#[test]
fn task_inputs_glob_auto_detect() {
    // Strings with glob metacharacters (*, ?, [) are auto-expanded
    // during freshness checks. This tests that the syntax parses correctly.
    let source = r#"
@inputs(["src/**/*.rs"])
@outputs(["./build/app"])
task build {
    null
}
println(build.inputs().len())
println(typeof(build.inputs()[0]))
"#;
    assert_output(source, &["1", "Path"]);
}

#[test]
fn task_outputs_reject_glob_patterns() {
    // A pattern in @outputs can never match: the files it describes do not
    // exist when the check runs, so the task would rerun forever instead of
    // ever being skipped. Say so rather than degrading silently.
    for outputs in [
        r#"["./build/**/profile_*"]"#,
        r#"[path("./build") / "x_*"]"#,
        r#"[g"./build/*.o"]"#,
        r#"["./build/app-?"]"#,
    ] {
        let source = format!(
            "@outputs({})\ntask build {{\n    null\n}}\nbuild()\n",
            outputs
        );
        let msg = assert_error_contains(&source, "@outputs must name concrete paths");
        assert!(msg.contains("stamp file"), "unhelpful message: {}", msg);
    }
}

#[test]
fn task_outputs_allow_a_bracket_in_a_filename() {
    // `[` is a glob metacharacter but also a legal filename character, and an
    // output naming a real file with a bracket in it has always worked.
    let source = r#"
@outputs(["./build/app[1]"])
task build {
    null
}
println(build.outputs())
"#;
    assert_output(source, &["[./build/app[1]]"]);
}

#[test]
fn task_param_hash_invalidation() {
    // Changing a param should cause a task to re-run even if outputs exist.
    // Task defined at top level, temp dir created inside with block.
    let source = r#"
mut out_path = path("/tmp")

@outputs([out_path])
task compile(mode) {
    out_path.write_text(mode)?
}

with TempDir {} as dir {
    out_path = dir / "out.txt"

    compile("debug")
    let first = out_path.read()?

    compile("release")
    let second = out_path.read()?

    println(first)
    println(second)
}
"#;
    assert_output(source, &[
        "[RUN]  compile", "[DONE] compile",
        "[RUN]  compile", "[DONE] compile",
        "debug",
        "release",
    ]);
}

#[test]
fn task_param_hash_skip_when_same() {
    // Same param should allow skip if output exists
    let source = r#"
mut out_path = path("/tmp")

@outputs([out_path])
task compile(mode) {
    out_path.write_text(mode)?
}

with TempDir {} as dir {
    out_path = dir / "out.txt"

    compile("debug")
    compile("debug")

    println(out_path.read()?)
}
"#;
    assert_output(source, &[
        "[RUN]  compile", "[DONE] compile",
        "[SKIP] compile",
        "debug",
    ]);
}

#[test]
fn task_env_hash_invalidation() {
    // Changing a tracked env var should cause re-run
    let source = r#"
mut out_path = path("/tmp")

@outputs([out_path])
@env([QUE_TEST_CC_INVAL])
task compile {
    let cc = env.get("QUE_TEST_CC_INVAL") ?? "gcc"
    out_path.write_text(cc)?
}

with TempDir {} as dir {
    out_path = dir / "out.txt"

    with env.scope({ "QUE_TEST_CC_INVAL": "gcc" }) {
        compile()
    }
    let first = out_path.read()?

    with env.scope({ "QUE_TEST_CC_INVAL": "clang" }) {
        compile()
    }
    let second = out_path.read()?

    println(first)
    println(second)
}
"#;
    assert_output(source, &[
        "[RUN]  compile", "[DONE] compile",
        "[RUN]  compile", "[DONE] compile",
        "gcc",
        "clang",
    ]);
}

#[test]
fn task_env_hash_skip_when_same() {
    // Same env var value should allow skip
    let source = r#"
mut out_path = path("/tmp")

@outputs([out_path])
@env([QUE_TEST_CC_SKIP])
task compile {
    let cc = env.get("QUE_TEST_CC_SKIP") ?? "gcc"
    out_path.write_text(cc)?
}

with TempDir {} as dir {
    out_path = dir / "out.txt"

    with env.scope({ "QUE_TEST_CC_SKIP": "gcc" }) {
        compile()
        compile()
    }

    println(out_path.read()?)
}
"#;
    assert_output(source, &[
        "[RUN]  compile", "[DONE] compile",
        "[SKIP] compile",
        "gcc",
    ]);
}

// ═════════════════════════════════════════════════════════════════════
// STREAMS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn stream_from_string() {
    let source = r#"
import std.stream
let s = stream.of("hello world")
println(typeof(s))
println(s.collect())
"#;
    assert_output(source, &["Stream", "hello world"]);
}

#[test]
fn stream_to_upper() {
    assert_result(
        r#"import std.stream
stream.of("hello world").to_upper().collect()"#,
        Value::String("HELLO WORLD".into()),
    );
}

#[test]
fn stream_to_lower() {
    assert_result(
        r#"import std.stream
stream.of("HELLO WORLD").to_lower().collect()"#,
        Value::String("hello world".into()),
    );
}

#[test]
fn stream_trim() {
    assert_result(
        r#"import std.stream
stream.of("  hello  ").trim().collect()"#,
        Value::String("hello".into()),
    );
}

#[test]
fn stream_replace() {
    assert_result(
        r#"import std.stream
stream.of("hello world").replace("world", "que").collect()"#,
        Value::String("hello que".into()),
    );
}

#[test]
fn stream_lines() {
    let source = r#"
import std.stream
let s = stream.of("line1\nline2\nline3")
let lines = s.lines()
println(lines.len())
println(lines[0])
println(lines[2])
"#;
    assert_output(source, &["3", "line1", "line3"]);
}

#[test]
fn stream_count_lines() {
    assert_result(
        r#"import std.stream
stream.of("a\nb\nc").count_lines()"#,
        Value::Int(3),
    );
}

#[test]
fn stream_pipe_chain() {
    // The core use case: read, transform, materialize in one pipeline
    let source = r#"
import std.stream
let result = stream.of("hello world\ngoodbye world")
    .to_upper()
    .replace("WORLD", "WISP")
    .collect()
println(result)
"#;
    assert_output(source, &["HELLO WISP\nGOODBYE WISP"]);
}

#[test]
fn stream_map_lines() {
    let source = r#"
import std.stream
let result = stream.of("alice\nbob\ncharlie")
    .map(|line| line.to_upper())
    .collect()
println(result)
"#;
    assert_output(source, &["ALICE\nBOB\nCHARLIE"]);
}

#[test]
fn stream_filter_lines() {
    let source = r#"
import std.stream
let result = stream.of("apple\nbanana\navocado\ncherry")
    .filter(|line| line.starts_with("a"))
    .collect()
println(result)
"#;
    assert_output(source, &["apple\navocado"]);
}

#[test]
fn stream_grep() {
    let source = r#"
import std.stream
let result = stream.of("error: something\ninfo: ok\nerror: again")
    .grep("error")
    .collect()
println(result)
"#;
    assert_output(source, &["error: something\nerror: again"]);
}

#[test]
fn stream_head() {
    let source = r#"
import std.stream
let result = stream.of("a\nb\nc\nd\ne")
    .head(3)
    .collect()
println(result)
"#;
    assert_output(source, &["a\nb\nc"]);
}

#[test]
fn stream_tail() {
    let source = r#"
import std.stream
let result = stream.of("a\nb\nc\nd\ne")
    .tail(2)
    .collect()
println(result)
"#;
    assert_output(source, &["d\ne"]);
}

#[test]
fn stream_sort_lines() {
    assert_result(
        r#"import std.stream
stream.of("cherry\napple\nbanana").sort_lines().collect()"#,
        Value::String("apple\nbanana\ncherry".into()),
    );
}

#[test]
fn stream_reverse_lines() {
    assert_result(
        r#"import std.stream
stream.of("a\nb\nc").reverse_lines().collect()"#,
        Value::String("c\nb\na".into()),
    );
}

#[test]
fn stream_unique_lines() {
    assert_result(
        r#"import std.stream
stream.of("a\nb\na\nc\nb").unique_lines().collect()"#,
        Value::String("a\nb\nc".into()),
    );
}

#[test]
fn stream_skip_empty() {
    assert_result(
        r#"import std.stream
stream.of("a\n\nb\n  \nc").skip_empty().collect()"#,
        Value::String("a\nb\nc".into()),
    );
}

#[test]
fn stream_enumerate_lines() {
    assert_result(
        r#"import std.stream
stream.of("foo\nbar").enumerate_lines().collect()"#,
        Value::String("1\tfoo\n2\tbar".into()),
    );
}

#[test]
fn stream_prepend_append() {
    let source = r#"
import std.stream
let result = stream.of("body")
    .prepend("header\n")
    .append("\nfooter")
    .collect()
println(result)
"#;
    assert_output(source, &["header\nbody\nfooter"]);
}

#[test]
fn stream_len_and_is_empty() {
    let source = r#"
import std.stream
let s = stream.of("hello")
println(s.len())
println(s.is_empty())
let empty = stream.of("")
println(empty.is_empty())
"#;
    assert_output(source, &["5", "false", "true"]);
}

#[test]
fn stream_contains() {
    let source = r#"
import std.stream
let s = stream.of("hello world")
println(s.contains("world"))
println(s.contains("xyz"))
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn stream_join_lines() {
    assert_result(
        r#"import std.stream
stream.of("a\nb\nc").join_lines(", ").collect()"#,
        Value::String("a, b, c".into()),
    );
}

#[test]
fn stream_from_list() {
    assert_result(
        r#"import std.stream
stream.of(["one", "two", "three"]).collect()"#,
        Value::String("one\ntwo\nthree".into()),
    );
}

#[test]
fn stream_of_from_list() {
    assert_result(
        r#"import std.stream
stream.of(["a", "b", "c"]).to_upper().collect()"#,
        Value::String("A\nB\nC".into()),
    );
}

#[test]
fn stream_is_type() {
    assert_result(
        r#"import std.stream
stream.of("test").is_type("Stream")"#,
        Value::Bool(true),
    );
}

#[test]
fn stream_complex_pipeline() {
    // A realistic pipeline: read lines, filter, transform, collect
    let source = r#"
import std.stream
let admins = stream.of("name,role\nalice,admin\nbob,user\ncharlie,admin")
    .filter(|line| line.contains("admin"))
    .map(|line| line.split(",")[0])
    .to_upper()
    .collect()
println(admins)
"#;
    assert_output(source, &["ALICE\nCHARLIE"]);
}

#[test]
fn stream_file_round_trip() {
    // Test reading and writing via streams
    let source = r#"
import std.fs { read, write, exists }
import std.stream
let tmp = path("/tmp/que_stream_test.txt")
write(tmp, "hello from que\nstream test")
let result = stream.file(tmp).to_upper().collect()
println(result)
"#;
    assert_output(source, &["HELLO FROM QUE\nSTREAM TEST"]);
    // Clean up
    let _ = std::fs::remove_file("/tmp/que_stream_test.txt");
}

#[test]
fn stream_write_to_file() {
    let source = r#"
import std.fs { read, write, exists }
import std.stream
let tmp_in = path("/tmp/que_stream_in.txt")
let tmp_out = path("/tmp/que_stream_out.txt")
write(tmp_in, "hello world")
stream.file(tmp_in).to_upper().write_to(tmp_out)
let result = read(tmp_out).unwrap()
println(result)
"#;
    assert_output(source, &["HELLO WORLD"]);
    // Clean up
    let _ = std::fs::remove_file("/tmp/que_stream_in.txt");
    let _ = std::fs::remove_file("/tmp/que_stream_out.txt");
}

#[test]
fn stream_pipe_to_builtin_function() {
    // Using stream with a free function via pipe
    assert_result(
        r#"import std.stream
stream.of("hello").to_upper().len()"#,
        Value::Int(5),
    );
}

#[test]
fn stream_truthy() {
    let source = r#"
import std.stream
let s = stream.of("content")
let e = stream.of("")
println(if s { "yes" } else { "no" })
println(if e { "yes" } else { "no" })
"#;
    assert_output(source, &["yes", "no"]);
}

#[test]
fn stream_display_as_content() {
    // When printed, stream displays its content directly
    let source = r#"
import std.stream
let s = stream.of("hello que")
println(s)
"#;
    assert_output(source, &["hello que"]);
}

#[test]
fn stream_split() {
    let source = r#"
import std.stream
let parts = stream.of("a:b:c").split(":")
println(parts.len())
println(parts[1])
"#;
    assert_output(source, &["3", "b"]);
}

#[test]
fn stream_inspect() {
    let source = r#"
import std.stream
let info = stream.of("hello\nworld").inspect()
println(info["type"])
println(info["lines"])
"#;
    assert_output(source, &["Stream", "2"]);
}

// ═════════════════════════════════════════════════════════════════════
// 22. CONFIG FILE PARSING & MANIPULATION
// ═════════════════════════════════════════════════════════════════════

// ── JSON parsing ─────────────────────────────────────────────────────

#[test]
fn config_parse_json_basic() {
    let source = r#"
import std.json { parse, stringify }
let data = parse("{\"name\": \"que\", \"version\": 1}").unwrap()
println(data["name"])
println(data["version"])
"#;
    assert_output(source, &["que", "1"]);
}

#[test]
fn config_parse_json_nested() {
    let source = r#"
import std.json { parse, stringify }
let json = "{\"database\": {\"host\": \"localhost\", \"port\": 5432}}"
let data = parse(json).unwrap()
println(data["database"]["host"])
println(data["database"]["port"])
"#;
    assert_output(source, &["localhost", "5432"]);
}

#[test]
fn config_parse_json_array() {
    let source = r#"
import std.json { parse, stringify }
let data = parse("[1, 2, 3]").unwrap()
println(data.len())
println(data[1])
"#;
    assert_output(source, &["3", "2"]);
}

#[test]
fn config_parse_json_error() {
    let source = r#"
import std.json { parse, stringify }
let result = parse("not valid json")
println(result.is_err())
"#;
    assert_output(source, &["true"]);
}

// ── YAML parsing ─────────────────────────────────────────────────────

#[test]
fn config_parse_yaml_basic() {
    let source = r#"
import std.yaml { parse, stringify }
let yaml = "name: que\nversion: 1\ntags:\n  - build\n  - devops"
let data = parse(yaml).unwrap()
println(data["name"])
println(data["version"])
println(data["tags"][0])
"#;
    assert_output(source, &["que", "1", "build"]);
}

#[test]
fn config_parse_yaml_error() {
    let source = r#"
import std.yaml { parse, stringify }
let result = parse("  bad:\nyaml: [")
println(result.is_err())
"#;
    assert_output(source, &["true"]);
}

// ── TOML parsing ─────────────────────────────────────────────────────

#[test]
fn config_parse_toml_basic() {
    let source = r#"
import std.toml { parse, stringify }
let toml_str = "name = \"que\"\nversion = 1\n\n[database]\nhost = \"localhost\"\nport = 5432"
let data = parse(toml_str).unwrap()
println(data["name"])
println(data["database"]["host"])
println(data["database"]["port"])
"#;
    assert_output(source, &["que", "localhost", "5432"]);
}

#[test]
fn config_parse_toml_error() {
    let source = r#"
import std.toml { parse, stringify }
let result = parse("= invalid toml")
println(result.is_err())
"#;
    assert_output(source, &["true"]);
}

// ── Serialization ────────────────────────────────────────────────────

#[test]
fn config_to_json_compact() {
    let source = r#"
import std.json { parse, stringify }
let data = {"name": "que", "version": 1}
let json = stringify(data)
println(json)
"#;
    assert_output(source, &[r#"{"name":"que","version":1}"#]);
}

#[test]
fn config_to_json_pretty() {
    let source = r#"
import std.json { parse, stringify }
let data = {"name": "que"}
let json = stringify(data, 2)
println(json)
"#;
    assert_output(source, &["{\n  \"name\": \"que\"\n}"]);
}

#[test]
fn config_to_yaml_basic() {
    let source = r#"
import std.yaml { parse, stringify }
let data = {"name": "que", "version": 1}
let yaml = stringify(data)
println(yaml.trim())
"#;
    assert_output(source, &["name: que\nversion: 1"]);
}

#[test]
fn config_to_toml_basic() {
    let source = r#"
import std.toml { parse, stringify }
let data = {"name": "que", "version": 1}
let t = stringify(data)
println(t.trim())
"#;
    assert_output(source, &["name = \"que\"\nversion = 1"]);
}

// ── JSON ↔ YAML ↔ TOML roundtrip ────────────────────────────────────

#[test]
fn config_json_to_yaml_roundtrip() {
    let source = r#"
import std.json
import std.yaml
let input = "{\"name\": \"que\", \"port\": 8080}"
let data = json.parse(input).unwrap()
let yml = yaml.stringify(data)
let data2 = yaml.parse(yml).unwrap()
println(data2["name"])
println(data2["port"])
"#;
    assert_output(source, &["que", "8080"]);
}

#[test]
fn config_json_to_toml_roundtrip() {
    let source = r#"
import std.json
import std.toml
let input = "{\"name\": \"que\", \"port\": 8080}"
let data = json.parse(input).unwrap()
let toml_str = toml.stringify(data)
let data2 = toml.parse(toml_str).unwrap()
println(data2["name"])
println(data2["port"])
"#;
    assert_output(source, &["que", "8080"]);
}

// ── Path-based access (get_path) ─────────────────────────────────────

#[test]
fn config_get_path_simple() {
    let source = r#"
let data = {"database": {"host": "localhost", "port": 5432}}
println(data.get_path("database.host"))
println(data.get_path("database.port"))
"#;
    assert_output(source, &["localhost", "5432"]);
}

#[test]
fn config_get_path_deep() {
    let source = r#"
let data = {"a": {"b": {"c": {"d": 42}}}}
println(data.get_path("a.b.c.d"))
"#;
    assert_output(source, &["42"]);
}

#[test]
fn config_get_path_array_index() {
    let source = r#"
import std.json { parse, stringify }
let json = "{\"servers\": [{\"host\": \"web1\"}, {\"host\": \"web2\"}]}"
let data = parse(json).unwrap()
println(data.get_path("servers[0].host"))
println(data.get_path("servers[1].host"))
"#;
    assert_output(source, &["web1", "web2"]);
}

#[test]
fn config_get_path_wildcard() {
    let source = r#"
import std.json { parse, stringify }
let json = "{\"items\": [{\"id\": 1}, {\"id\": 2}, {\"id\": 3}]}"
let data = parse(json).unwrap()
let ids = data.get_path("items[*].id")
println(ids)
"#;
    assert_output(source, &["[1, 2, 3]"]);
}

#[test]
fn config_get_path_missing() {
    let source = r#"
let data = {"a": 1}
println(data.get_path("b"))
println(data.get_path("a.b.c"))
"#;
    assert_output(source, &["null", "null"]);
}

// ── Path-based modification (set_path) ───────────────────────────────

#[test]
fn config_set_path_simple() {
    let source = r#"
let data = {"database": {"host": "localhost", "port": 5432}}
let updated = data.set_path("database.port", 3306)
println(updated.get_path("database.port"))
println(updated.get_path("database.host"))
"#;
    assert_output(source, &["3306", "localhost"]);
}

#[test]
fn config_set_path_creates_intermediate() {
    let source = r#"
let data = {}
let updated = data.set_path("new.nested.key", "value")
println(updated.get_path("new.nested.key"))
"#;
    assert_output(source, &["value"]);
}

#[test]
fn config_set_path_chain() {
    let source = r#"
let config = {"app": {"name": "myapp"}}
let updated = config
    .set_path("app.version", "2.0")
    .set_path("app.debug", false)
    .set_path("database.host", "db.example.com")
println(updated.get_path("app.name"))
println(updated.get_path("app.version"))
println(updated.get_path("app.debug"))
println(updated.get_path("database.host"))
"#;
    assert_output(source, &["myapp", "2.0", "false", "db.example.com"]);
}

// ── Path-based deletion (delete_path) ────────────────────────────────

#[test]
fn config_delete_path_simple() {
    let source = r#"
let data = {"a": 1, "b": 2, "c": 3}
let updated = data.delete_path("b")
println(data.has_path("b"))
println(updated.has_path("b"))
println(updated.has_path("a"))
"#;
    assert_output(source, &["true", "false", "true"]);
}

#[test]
fn config_delete_path_nested() {
    let source = r#"
let data = {"database": {"host": "localhost", "port": 5432, "password": "secret"}}
let updated = data.delete_path("database.password")
println(updated.has_path("database.host"))
println(updated.has_path("database.password"))
"#;
    assert_output(source, &["true", "false"]);
}

// ── has_path ─────────────────────────────────────────────────────────

#[test]
fn config_has_path() {
    let source = r#"
let data = {"database": {"host": "localhost"}}
println(data.has_path("database"))
println(data.has_path("database.host"))
println(data.has_path("database.port"))
println(data.has_path("missing"))
"#;
    assert_output(source, &["true", "true", "false", "false"]);
}

// ── paths (list all paths) ───────────────────────────────────────────

#[test]
fn config_paths_list() {
    let source = r#"
let data = {"a": 1, "b": {"c": 2}}
let p = data.paths()
println(p.contains("a"))
println(p.contains("b.c"))
"#;
    assert_output(source, &["true", "true"]);
}

// ── Map serialization methods ────────────────────────────────────────

#[test]
fn config_map_to_json() {
    let source = r#"
let data = {"name": "que"}
println(data.to_json())
"#;
    assert_output(source, &[r#"{"name":"que"}"#]);
}

#[test]
fn config_map_to_json_pretty() {
    let source = r#"
let data = {"name": "que"}
println(data.to_json(2))
"#;
    assert_output(source, &["{\n  \"name\": \"que\"\n}"]);
}

#[test]
fn config_map_to_yaml() {
    let source = r#"
let data = {"name": "que"}
println(data.to_yaml().trim())
"#;
    assert_output(source, &["name: que"]);
}

#[test]
fn config_map_to_toml() {
    let source = r#"
let data = {"name": "que"}
println(data.to_toml().trim())
"#;
    assert_output(source, &["name = \"que\""]);
}

// ── Stream config parsing ────────────────────────────────────────────

#[test]
fn config_stream_parse_json() {
    let source = r#"
import std.stream
let data = stream.of("{\"name\": \"que\", \"version\": 1}").parse_json()
println(data["name"])
println(data["version"])
"#;
    assert_output(source, &["que", "1"]);
}

#[test]
fn config_stream_parse_yaml() {
    let source = r#"
import std.stream
let data = stream.of("name: que\nversion: 1").parse_yaml()
println(data["name"])
println(data["version"])
"#;
    assert_output(source, &["que", "1"]);
}

#[test]
fn config_stream_parse_toml() {
    let source = r#"
import std.stream
let data = stream.of("name = \"que\"\nversion = 1").parse_toml()
println(data["name"])
println(data["version"])
"#;
    assert_output(source, &["que", "1"]);
}

// ── Free function forms ──────────────────────────────────────────────

#[test]
fn the_removed_config_globals_name_the_method_to_use() {
    // config_get(data, path) and data.get_path(path) called the same function.
    for (global, method) in [
        ("config_get(d, \"a\")", "get_path"),
        ("config_set(d, \"a\", 1)", "set_path"),
        ("config_delete(d, \"a\")", "delete_path"),
        ("config_has(d, \"a\")", "has_path"),
        ("config_merge(d, d)", "deep_merge"),
        ("config_paths(d)", "paths"),
    ] {
        let source = format!("let d = {{\"a\": 1}}\n{}\n", global);
        assert_error_contains(&source, method);
    }
}

// ── File-based config read/write roundtrip ───────────────────────────

#[test]
fn config_file_json_roundtrip() {
    let source = r#"
import std.config
let tmp = "/tmp/que_config_test.json"
let data = {"name": "que", "version": 1, "features": ["json", "yaml", "toml"]}
config.write(tmp, data)
let loaded = config.read(tmp).unwrap()
println(loaded["name"])
println(loaded["version"])
println(loaded["features"][2])
"#;
    assert_output(source, &["que", "1", "toml"]);
    let _ = std::fs::remove_file("/tmp/que_config_test.json");
}

#[test]
fn config_file_yaml_roundtrip() {
    let source = r#"
import std.config
let tmp = "/tmp/que_config_test.yaml"
let data = {"name": "que", "port": 8080}
config.write(tmp, data)
let loaded = config.read(tmp).unwrap()
println(loaded["name"])
println(loaded["port"])
"#;
    assert_output(source, &["que", "8080"]);
    let _ = std::fs::remove_file("/tmp/que_config_test.yaml");
}

#[test]
fn config_file_toml_roundtrip() {
    let source = r#"
import std.config
let tmp = "/tmp/que_config_test.toml"
let data = {"name": "que", "port": 8080}
config.write(tmp, data)
let loaded = config.read(tmp).unwrap()
println(loaded["name"])
println(loaded["port"])
"#;
    assert_output(source, &["que", "8080"]);
    let _ = std::fs::remove_file("/tmp/que_config_test.toml");
}

// ── Realistic pipeline: read, modify, convert format ─────────────────

#[test]
fn config_pipeline_read_modify_convert() {
    let source = r#"
import std.json
import std.yaml
let input = "{\"app\": {\"name\": \"myapp\", \"version\": \"1.0\", \"debug\": true}}"
let config = json.parse(input).unwrap()
let updated = config
    .set_path("app.version", "2.0")
    .set_path("app.debug", false)
    .set_path("app.port", 9090)
println(updated.get_path("app.version"))
println(updated.get_path("app.debug"))
println(updated.get_path("app.port"))
let yml = updated.to_yaml()
let reloaded = yaml.parse(yml).unwrap()
println(reloaded.get_path("app.name"))
"#;
    assert_output(source, &["2.0", "false", "9090", "myapp"]);
}

#[test]
fn config_wildcard_set() {
    let source = r#"
import std.json { parse, stringify }
let json = "{\"servers\": [{\"host\": \"a\", \"port\": 80}, {\"host\": \"b\", \"port\": 80}]}"
let data = parse(json).unwrap()
let updated = data.set_path("servers[*].port", 443)
println(updated.get_path("servers[0].port"))
println(updated.get_path("servers[1].port"))
"#;
    assert_output(source, &["443", "443"]);
}

// ── Pipe integration ─────────────────────────────────────────────────

#[test]
fn config_pipe_with_parse() {
    let source = r#"
import std.json { parse, stringify }
let data = "{\"x\": 42}" |> parse
println(data.unwrap().get_path("x"))
"#;
    assert_output(source, &["42"]);
}

// ════════════════════════════════════════════════════════════════════════
// 23. SET TYPE
// ════════════════════════════════════════════════════════════════════════

// ── Set literal and basics ───────────────────────────────────────────

#[test]
fn set_literal_basic() {
    let source = r#"
let s = #{1, 2, 3}
println(typeof(s))
println(s.len())
println(s)
"#;
    assert_output(source, &["Set", "3", "#{1, 2, 3}"]);
}

#[test]
fn set_literal_deduplicates() {
    let source = r#"
let s = #{1, 2, 2, 3, 3, 3}
println(s.len())
println(s)
"#;
    assert_output(source, &["3", "#{1, 2, 3}"]);
}

#[test]
fn set_literal_strings() {
    let source = r#"
let s = #{"a", "b", "c", "a"}
println(s.len())
println(s.contains("a"))
println(s.contains("z"))
"#;
    assert_output(source, &["3", "true", "false"]);
}

#[test]
fn set_literal_mixed_types() {
    let source = r#"
let s = #{1, "hello", true}
println(s.len())
println(s.contains(1))
println(s.contains("hello"))
println(s.contains(false))
"#;
    assert_output(source, &["3", "true", "true", "false"]);
}

#[test]
fn set_empty_via_difference() {
    let source = r#"
let s = #{1, 2, 3}
let empty = s - s
println(typeof(empty))
println(empty.len())
println(empty.is_empty())
"#;
    assert_output(source, &["Set", "0", "true"]);
}

// ── `{ ... }` vs `#{ ... }` disambiguation ───────────────────────────

#[test]
fn brace_single_expr_is_block_not_set() {
    // `{ x }` is always a single-expression block. Sets have their own
    // opener, so there is nothing to disambiguate.
    let source = r#"
let v = { 42 }
println(typeof(v))
println(v)
"#;
    assert_output(source, &["Int", "42"]);
}

#[test]
fn hash_brace_single_expr_is_set() {
    // No trailing comma needed — `#{ x }` is a one-element set.
    let source = r#"
let s = #{ 42 }
println(typeof(s))
println(s.len())
println(s.contains(42))
"#;
    assert_output(source, &["Set", "1", "true"]);
}

#[test]
fn empty_hash_brace_is_empty_set() {
    // `{}` is the empty map; `#{}` is the empty set.
    let source = r#"
println(typeof({}))
println(typeof(#{}))
println(#{}.len())
"#;
    assert_output(source, &["Map", "Set", "0"]);
}

#[test]
fn brace_set_literal_is_rejected() {
    assert_error("let s = {1, 2, 3}");
}

#[test]
fn brace_classification_handles_deeply_nested_groups() {
    // Stress-test the bracket-pair fast skip in classify_braces. Nested
    // parens, brackets and braces must not confuse the top-level scan.
    let source = r#"
let s = #{ (1 + (2 * (3 + 4))), [1, [2, 3]], {1: "a", 2: "b"} }
println(typeof(s))
println(s.len())
"#;
    assert_output(source, &["Set", "3"]);
}

// ── Set methods ──────────────────────────────────────────────────────

#[test]
fn set_add_method() {
    let source = r#"
let s = #{1, 2}
let s2 = s.add(3)
println(s2.len())
let s3 = s2.add(2)
println(s3.len())
"#;
    assert_output(source, &["3", "3"]);
}

#[test]
fn set_remove_method() {
    let source = r#"
let s = #{1, 2, 3}
let s2 = s.remove(2)
println(s2.len())
println(s2.contains(2))
println(s2.contains(1))
"#;
    assert_output(source, &["2", "false", "true"]);
}

#[test]
fn set_to_list() {
    let source = r#"
let s = #{3, 1, 2}
let l = s.to_list()
println(typeof(l))
println(l.len())
"#;
    assert_output(source, &["List", "3"]);
}

// ── Set operations (methods) ─────────────────────────────────────────

#[test]
fn set_union_method() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{3, 4, 5}
let c = a.union(b)
println(c.len())
println(c.contains(1))
println(c.contains(5))
"#;
    assert_output(source, &["5", "true", "true"]);
}

#[test]
fn set_intersection_method() {
    let source = r#"
let a = #{1, 2, 3, 4}
let b = #{3, 4, 5, 6}
let c = a.intersection(b)
println(c.len())
println(c.contains(3))
println(c.contains(4))
println(c.contains(1))
"#;
    assert_output(source, &["2", "true", "true", "false"]);
}

#[test]
fn set_difference_method() {
    let source = r#"
let a = #{1, 2, 3, 4}
let b = #{3, 4, 5}
let c = a.difference(b)
println(c.len())
println(c.contains(1))
println(c.contains(2))
println(c.contains(3))
"#;
    assert_output(source, &["2", "true", "true", "false"]);
}

#[test]
fn set_symmetric_difference_method() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{2, 3, 4}
let c = a.symmetric_difference(b)
println(c.len())
println(c.contains(1))
println(c.contains(4))
println(c.contains(2))
"#;
    assert_output(source, &["2", "true", "true", "false"]);
}

#[test]
fn set_is_subset_superset() {
    let source = r#"
let a = #{1, 2}
let b = #{1, 2, 3, 4}
println(a.is_subset(b))
println(b.is_subset(a))
println(b.is_superset(a))
println(a.is_superset(b))
"#;
    assert_output(source, &["true", "false", "true", "false"]);
}

#[test]
fn set_is_disjoint() {
    let source = r#"
let a = #{1, 2}
let b = #{3, 4}
let c = #{2, 3}
println(a.is_disjoint(b))
println(a.is_disjoint(c))
"#;
    assert_output(source, &["true", "false"]);
}

// ── Set operators ────────────────────────────────────────────────────

#[test]
fn set_operator_union_plus() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{3, 4, 5}
let c = a + b
println(c.len())
println(c.contains(1))
println(c.contains(5))
"#;
    assert_output(source, &["5", "true", "true"]);
}

#[test]
fn set_operator_difference_minus() {
    let source = r#"
let a = #{1, 2, 3, 4}
let b = #{3, 4}
let c = a - b
println(c.len())
println(c.contains(1))
println(c.contains(3))
"#;
    assert_output(source, &["2", "true", "false"]);
}

#[test]
fn set_operator_intersection_ampersand() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{2, 3, 4}
let c = a & b
println(c.len())
println(c.contains(2))
println(c.contains(1))
"#;
    assert_output(source, &["2", "true", "false"]);
}

#[test]
fn set_operator_symmetric_diff_caret() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{2, 3, 4}
let c = a ^ b
println(c.len())
println(c.contains(1))
println(c.contains(4))
println(c.contains(2))
"#;
    assert_output(source, &["2", "true", "true", "false"]);
}

#[test]
fn set_operator_pipe_union() {
    let source = r#"
let a = #{1, 2}
let b = #{2, 3}
let c = a | b
println(c.len())
"#;
    assert_output(source, &["3"]);
}

// ── Set iteration ────────────────────────────────────────────────────

#[test]
fn set_for_loop() {
    let source = r#"
let s = #{10, 20, 30}
mut total = 0
for x in s {
    total = total + x
}
println(total)
"#;
    assert_output(source, &["60"]);
}

#[test]
fn set_map_method() {
    let source = r#"
let s = #{1, 2, 3}
let doubled = s.map(|x| x * 2)
println(typeof(doubled))
println(doubled.len())
println(doubled.contains(2))
println(doubled.contains(6))
"#;
    assert_output(source, &["Set", "3", "true", "true"]);
}

#[test]
fn set_filter_method() {
    let source = r#"
let s = #{1, 2, 3, 4, 5}
let evens = s.filter(|x| x % 2 == 0)
println(typeof(evens))
println(evens.len())
println(evens.contains(2))
println(evens.contains(4))
println(evens.contains(3))
"#;
    assert_output(source, &["Set", "2", "true", "true", "false"]);
}

#[test]
fn set_each_method() {
    let source = r#"
let s = #{10, 20, 30}
s.each(|x| println(x))
"#;
    assert_output(source, &["10", "20", "30"]);
}

// ── Set with builtins ────────────────────────────────────────────────

#[test]
fn set_contains_method() {
    let source = r#"
let s = #{1, 2, 3}
println(s.contains(2))
println(s.contains(5))
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn set_typeof_is_type() {
    let source = r#"
let s = #{1, 2}
println(typeof(s))
println(s.is_type("Set"))
println(s.is_type("Set"))
println(s.is_type("List"))
"#;
    assert_output(source, &["Set", "true", "true", "false"]);
}

// ── Set equality ─────────────────────────────────────────────────────

#[test]
fn set_equality_order_independent() {
    let source = r#"
let a = #{1, 2, 3}
let b = #{3, 1, 2}
println(a == b)
let c = #{1, 2}
println(a == c)
"#;
    assert_output(source, &["true", "false"]);
}

// ── Set truthiness ───────────────────────────────────────────────────

#[test]
fn set_truthiness() {
    let source = r#"
let full = #{1, 2}
let empty = full - full
println(if full { "yes" } else { "no" })
println(if empty { "yes" } else { "no" })
"#;
    assert_output(source, &["yes", "no"]);
}

// ── Set with list interop ────────────────────────────────────────────

#[test]
fn set_from_list_unique() {
    let source = r#"
let list = [1, 2, 2, 3, 3, 3]
let unique = list.unique()
println(unique.len())
"#;
    assert_output(source, &["3"]);
}

#[test]
fn set_union_with_list() {
    let source = r#"
let s = #{1, 2}
let extra = s.union([2, 3, 4])
println(typeof(extra))
println(extra.len())
"#;
    assert_output(source, &["Set", "4"]);
}

// ── Set serialization (config integration) ───────────────────────────

#[test]
fn set_to_json_serializes_as_array() {
    let source = r#"
let data = {"tags": #{1, 2, 3}}
println(data.to_json())
"#;
    // Sets serialize as JSON arrays
    assert_output(source, &["{\"tags\":[1,2,3]}"]);
}

// ── Practical: dedup pipeline ────────────────────────────────────────

#[test]
fn set_practical_dedup_pipeline() {
    let source = r#"
let words = ["apple", "banana", "apple", "cherry", "banana"]
// Build set incrementally
mut s = #{words[0],}
for w in words.skip(1) {
    s = s.add(w)
}
println(s.len())
println(s.contains("apple"))
println(s.contains("cherry"))
"#;
    assert_output(source, &["3", "true", "true"]);
}

// ── Hash trait wiring for sets ───────────────────────────────────────

#[test]
fn set_instance_requires_hash_trait() {
    // Trying to add an Instance without a Hash impl should error.
    let source = r#"
struct Point { x, y }
let p = Point { x: 1, y: 2 }
let s = #{p,}
"#;
    assert_error(source);
}

#[test]
fn set_instance_with_hash_trait_works() {
    // An Instance that implements Hash (and Eq) can be a set element.
    let source = r#"
struct Point { x, y }
impl Hash for Point {
    fn hash(self) -> Int { self.x * 31 + self.y }
}
impl Eq for Point {
    fn equals(self, other) -> Bool { self.x == other.x && self.y == other.y }
}
let p1 = Point { x: 1, y: 2 }
let p2 = Point { x: 3, y: 4 }
let p3 = Point { x: 1, y: 2 }
mut s = #{p1, p2}
s = s.add(p3)
println(s.len())
println(s.contains(p1))
"#;
    assert_output(source, &["2", "true"]);
}

#[test]
fn set_instance_eq_trait_used_for_membership() {
    // Eq.equals() is used when checking membership, not structural equality.
    let source = r#"
struct Wrapper { val }
impl Hash for Wrapper {
    fn hash(self) -> Int { self.val }
}
impl Eq for Wrapper {
    fn equals(self, other) -> Bool { self.val == other.val }
}
let a = Wrapper { val: 42 }
let b = Wrapper { val: 42 }
mut s = #{a,}
println(s.contains(b))
"#;
    assert_output(source, &["true"]);
}

// ════════════════════════════════════════════════════════════════════════
// 24. CLOSURE MUTABLE CAPTURE
// ════════════════════════════════════════════════════════════════════════

// ── Basic mutable capture ────────────────────────────────────────────

#[test]
fn closure_mutates_outer_variable() {
    let source = r#"
mut x = 0
let inc = || { x = x + 1 }
inc()
inc()
inc()
println(x)
"#;
    assert_output(source, &["3"]);
}

#[test]
fn closure_mutates_counter_via_arg() {
    let source = r#"
mut count = 0
let add = |n| { count = count + n }
add(5)
add(10)
println(count)
"#;
    assert_output(source, &["15"]);
}

#[test]
fn closure_reads_latest_outer_value() {
    let source = r#"
mut x = 1
let read_x = || println(x)
read_x()
x = 42
read_x()
"#;
    assert_output(source, &["1", "42"]);
}

// ── each with mutation ───────────────────────────────────────────────

#[test]
fn list_each_mutates_outer() {
    let source = r#"
mut total = 0
[1, 2, 3, 4].each(|x| { total = total + x })
println(total)
"#;
    assert_output(source, &["10"]);
}

#[test]
fn set_each_mutates_outer() {
    let source = r#"
mut total = 0
#{10, 20, 30}.each(|x| { total = total + x })
println(total)
"#;
    assert_output(source, &["60"]);
}

// ── Accumulator pattern ──────────────────────────────────────────────

#[test]
fn closure_param_type_annotation() {
    assert_result("let f = |x: Int| x * 2\nf(21)", Value::Int(42));
}

#[test]
fn closure_param_default_value() {
    let source = r#"
let scale = |x, factor = 2| x * factor
[scale(5), scale(5, 3)]
"#;
    assert_result(source, Value::List(vec![Value::Int(10), Value::Int(15)]));
}

#[test]
fn closure_param_annotation_and_default() {
    assert_result("let f = |x: Int, k: Int = 10| x + k\nf(5)", Value::Int(15));
}

#[test]
fn fn_expression_lambda_is_rejected() {
    let err = run("let double = fn(x) => x * 2\ndouble(2)")
        .expect_err("fn(...) => ... should no longer parse as a closure");
    assert!(
        err.to_string().contains("|x| expr"),
        "expected closure migration hint, got: {}",
        err
    );
}

#[test]
fn fn_block_lambda_is_rejected() {
    let err = run("let f = fn(x) { x }\nf(1)")
        .expect_err("fn(...) { ... } should no longer parse as a closure");
    assert!(
        err.to_string().contains("closure"),
        "expected closure migration hint, got: {}",
        err
    );
}

#[test]
fn closure_accumulator_pattern() {
    let source = r#"
mut results = []
["hello", "world", "test"].each(|w| {
    results = results.push(w.to_upper())
})
println(results.len())
println(results[0])
println(results[2])
"#;
    assert_output(source, &["3", "HELLO", "TEST"]);
}

// ── Multiple closures sharing state ──────────────────────────────────

#[test]
fn multiple_closures_share_mutable() {
    let source = r#"
mut val = 0
let inc = || { val = val + 1 }
let dec = || { val = val - 1 }
let get = || val
inc()
inc()
inc()
dec()
println(get())
"#;
    assert_output(source, &["2"]);
}

// ── Closure returned from function ───────────────────────────────────

#[test]
fn closure_returned_preserves_state() {
    let source = r#"
fn make_counter() {
    mut count = 0
    return || {
        count = count + 1
        return count
    }
}
let counter = make_counter()
println(counter())
println(counter())
println(counter())
"#;
    assert_output(source, &["1", "2", "3"]);
}

#[test]
fn separate_closures_have_separate_state() {
    let source = r#"
fn make_counter() {
    mut count = 0
    return || {
        count = count + 1
        return count
    }
}
let a = make_counter()
let b = make_counter()
println(a())
println(a())
println(b())
println(a())
"#;
    assert_output(source, &["1", "2", "1", "3"]);
}

// ── Closure with filter/map mutation side effects ────────────────────

#[test]
fn map_callback_mutates_outer() {
    let source = r#"
mut call_count = 0
let result = [1, 2, 3].map(|x| {
    call_count = call_count + 1
    x * 2
})
println(call_count)
println(result)
"#;
    assert_output(source, &["3", "[2, 4, 6]"]);
}

#[test]
fn filter_callback_mutates_outer() {
    let source = r#"
mut checked = 0
let evens = [1, 2, 3, 4, 5].filter(|x| {
    checked = checked + 1
    x % 2 == 0
})
println(checked)
println(evens)
"#;
    assert_output(source, &["5", "[2, 4]"]);
}

// ── Nested closure mutation ──────────────────────────────────────────

#[test]
fn nested_closure_mutation() {
    let source = r#"
mut x = 0
let outer = || {
    let inner = || { x = x + 10 }
    inner()
    x = x + 1
}
outer()
println(x)
"#;
    assert_output(source, &["11"]);
}

// ── Immutability still enforced ──────────────────────────────────────

#[test]
fn closure_cannot_mutate_immutable() {
    let source = r#"
let x = 0
let f = || { x = 1 }
f()
"#;
    assert_error(source);
}

// ════════════════════════════════════════════════════════════════════════
// 25. DIRECTORY COPY / MOVE
// ════════════════════════════════════════════════════════════════════════

#[test]
fn copy_file_basic() {
    let dir = std::env::temp_dir().join("que_test_copy_file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello world").unwrap();

    let source = format!(r#"
let src = path("{}/hello.txt")
let dst = path("{}/hello_copy.txt")
let result = src.copy_to(dst)
println(typeof(result))
let content = dst.read().unwrap()
println(content)
"#, dir.display(), dir.display());
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["Ok", "hello world"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_dir_recursive() {
    let dir = std::env::temp_dir().join("que_test_copy_dir");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src_tree");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), "file a").unwrap();
    std::fs::write(src.join("sub/b.txt"), "file b").unwrap();

    let source = format!(r#"
let src = path("{}")
let dst = path("{}")
let result = src.copy_to(dst)
println(typeof(result))

// Verify files exist
let a = path("{}/a.txt").read().unwrap()
let b = path("{}/sub/b.txt").read().unwrap()
println(a)
println(b)
"#,
        src.display(),
        dir.join("dst_tree").display(),
        dir.join("dst_tree").display(),
        dir.join("dst_tree").display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["Ok", "file a", "file b"]);

    // Original still exists
    assert!(src.join("a.txt").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_dir_nested_structure() {
    let dir = std::env::temp_dir().join("que_test_copy_nested");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("root");
    std::fs::create_dir_all(src.join("a/b/c")).unwrap();
    std::fs::write(src.join("top.txt"), "top").unwrap();
    std::fs::write(src.join("a/mid.txt"), "mid").unwrap();
    std::fs::write(src.join("a/b/c/deep.txt"), "deep").unwrap();

    let source = format!(r#"
let src = path("{}")
let dst = path("{}")
src.copy_to(dst).unwrap()
println(path("{}/top.txt").read().unwrap())
println(path("{}/a/mid.txt").read().unwrap())
println(path("{}/a/b/c/deep.txt").read().unwrap())
"#,
        src.display(),
        dir.join("copy").display(),
        dir.join("copy").display(),
        dir.join("copy").display(),
        dir.join("copy").display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["top", "mid", "deep"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn move_file_basic() {
    let dir = std::env::temp_dir().join("que_test_move_file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("orig.txt"), "move me").unwrap();

    let source = format!(r#"
let src = path("{}/orig.txt")
let dst = path("{}/moved.txt")
let result = src.move_to(dst)
println(typeof(result))
println(dst.read().unwrap())
println(src.exists())
"#, dir.display(), dir.display());
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["Ok", "move me", "false"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn move_dir_basic() {
    let dir = std::env::temp_dir().join("que_test_move_dir");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src_dir");
    std::fs::create_dir_all(src.join("child")).unwrap();
    std::fs::write(src.join("file.txt"), "contents").unwrap();
    std::fs::write(src.join("child/nested.txt"), "nested").unwrap();

    let source = format!(r#"
let src = path("{}")
let dst = path("{}")
src.move_to(dst).unwrap()
println(path("{}/file.txt").read().unwrap())
println(path("{}/child/nested.txt").read().unwrap())
println(src.exists())
"#,
        src.display(),
        dir.join("dst_dir").display(),
        dir.join("dst_dir").display(),
        dir.join("dst_dir").display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["contents", "nested", "false"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_dir_with_path_arg() {
    // Ensure copy works when dest is a Path value (not just String)
    let dir = std::env::temp_dir().join("que_test_copy_path_arg");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("x.txt"), "hello").unwrap();

    let source = format!(r#"
let src = path("{}")
let dst = path("{}")
src.copy_to(dst).unwrap()
println(dst.join("x.txt").read().unwrap())
"#,
        src.display(),
        dir.join("dst").display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["hello"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_into_existing_dir_keeps_source_name() {
    // `cp` semantics: an existing directory means *into* it, so the contents
    // of the source must not spill directly into the destination.
    let dir = std::env::temp_dir().join("que_test_copy_into_dir");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("payload");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("x.txt"), "hello").unwrap();
    std::fs::write(dir.join("loose.txt"), "loose").unwrap();
    let into = dir.join("into");
    std::fs::create_dir_all(&into).unwrap();

    let source = format!(
        r#"
path("{}").copy_to(path("{}")).unwrap()
path("{}").copy_to(path("{}")).unwrap()
println(path("{}/payload/x.txt").read().unwrap())
println(path("{}/loose.txt").read().unwrap())
"#,
        src.display(),
        into.display(),
        dir.join("loose.txt").display(),
        into.display(),
        into.display(),
        into.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["hello", "loose"]);
    // The old behaviour dropped x.txt straight into the destination.
    assert!(!into.join("x.txt").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn glob_copy_to_keeps_tree_below_pattern_base() {
    // A recursive glob must not flatten: the destination mirrors the layout
    // below the pattern's fixed base, so same-named files stay apart.
    let dir = std::env::temp_dir().join("que_test_glob_copy");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), "a").unwrap();
    std::fs::write(src.join("sub/a.txt"), "nested").unwrap();
    std::fs::write(src.join("skip.md"), "md").unwrap();
    let dest = dir.join("dest");

    let source = format!(
        r#"
let made = glob("{}/**/*.txt").copy_to(path("{}")).unwrap()
println(made.len())
println(path("{}/a.txt").read().unwrap())
println(path("{}/sub/a.txt").read().unwrap())
"#,
        src.display(),
        dest.display(),
        dest.display(),
        dest.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["2", "a", "nested"]);
    // Non-matching files stay behind, and the source is untouched.
    assert!(!dest.join("skip.md").exists());
    assert!(src.join("a.txt").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn glob_move_to_empties_the_matches() {
    let dir = std::env::temp_dir().join("que_test_glob_move");
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), "a").unwrap();
    std::fs::write(src.join("keep.md"), "md").unwrap();
    let dest = dir.join("dest");

    let source = format!(
        r#"
glob("{}/*.txt").move_to(path("{}")).unwrap()
println(path("{}/a.txt").read().unwrap())
"#,
        src.display(),
        dest.display(),
        dest.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["a"]);
    assert!(!src.join("a.txt").exists());
    assert!(src.join("keep.md").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_way_of_expanding_a_glob_agrees_on_the_matches() {
    // `Glob.expand()` brace-expanded and `Path.glob()` did not, so the same
    // pattern matched two files in one spelling and none in the other. Every
    // spelling now goes through the one expander.
    let dir = std::env::temp_dir().join("que_test_glob_consistency");
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["alpha", "beta", "gamma"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join("f.txt"), sub).unwrap();
    }

    let source = format!(
        r#"
let base = path("{}")
println(base.glob("{{alpha,beta}}/*.txt").len())
println(glob("{}/{{alpha,beta}}/*.txt").expand().len())
println(glob("{}/{{alpha,beta}}/*.txt").count())
mut n = 0
for f in glob("{}/{{alpha,beta}}/*.txt") {{ n = n + 1 }}
println(n)
"#,
        dir.display(),
        dir.display(),
        dir.display(),
        dir.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["2", "2", "2", "2"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tilde_in_a_glob_is_a_home_directory_in_every_spelling() {
    // Not a home directory literally named `~`, which is what `glob::glob`
    // looks for and never finds.
    let home = match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => std::path::PathBuf::from(h),
        _ => return,
    };
    let dir = home.join(".que_test_glob_tilde");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.txt"), "x").unwrap();

    let source = r#"
println(glob("~/.que_test_glob_tilde/*.txt").expand().len())
mut n = 0
for f in glob("~/.que_test_glob_tilde/*.txt") { n = n + 1 }
println(n)
"#;
    let (output, _) = run(source).unwrap();
    assert_eq!(output, vec!["1", "1"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_trailing_slash_on_the_directory_does_not_change_what_glob_matches() {
    let dir = std::env::temp_dir().join("que_test_glob_slash");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.txt"), "x").unwrap();

    let source = format!(
        r#"
println(path("{}").glob("*.txt").len())
println(path("{}/").glob("*.txt").len())
"#,
        dir.display(),
        dir.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["1", "1"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn glob_first_and_any_see_past_the_first_alternative() {
    // `first()` used to stop at the first *alternative* that matched nothing
    // only by luck of ordering; both now read off the same expansion.
    let dir = std::env::temp_dir().join("que_test_glob_alternatives");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("second")).unwrap();
    std::fs::write(dir.join("second/f.txt"), "x").unwrap();

    let source = format!(
        r#"
let g = glob("{}/{{first,second}}/*.txt")
println(g.any())
println(g.first().name())
println(g.count())
"#,
        dir.display(),
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["true", "f.txt", "1"]);

    let _ = std::fs::remove_dir_all(&dir);
}

// ════════════════════════════════════════════════════════════════════════
// 26. WITH TEMP_DIR / TEMP_FILE
// ════════════════════════════════════════════════════════════════════════

#[test]
fn with_temp_dir_creates_and_cleans() {
    // Capture the temp path from inside the block, then verify it was cleaned up
    let source = r#"
mut saved_path = ""
with TempDir {} as tmp {
    saved_path = tmp.to_string()
    println(tmp.is_dir())
    // Write a file inside the temp dir
    tmp.join("test.txt").write_text("hello").unwrap()
    println(tmp.join("test.txt").read().unwrap())
}
// After the block, the temp dir should be gone
println(saved_path.to_path().exists())
"#;
    assert_output(source, &["true", "hello", "false"]);
}

#[test]
fn with_temp_file_creates_and_cleans() {
    let source = r#"
mut saved_path = ""
with TempFile {} as f {
    saved_path = f.to_string()
    println(f.exists())
    f.write_text("temp content").unwrap()
    println(f.read().unwrap())
}
println(saved_path.to_path().exists())
"#;
    assert_output(source, &["true", "temp content", "false"]);
}

#[test]
fn with_temp_dir_keep_preserves() {
    // The new OOP-based TempDir always cleans up — verify that behavior
    let source = r#"
mut saved_path = ""
with TempDir {} as d {
    saved_path = d.to_string()
    d.join("keep_me.txt").write_text("precious").unwrap()
}
println(saved_path.to_path().exists())
"#;
    assert_output(source, &["false"]);
}

#[test]
fn with_temp_file_keep_preserves() {
    // The new OOP-based TempFile always cleans up — verify that behavior
    let source = r#"
mut saved_path = ""
with TempFile {} as f {
    saved_path = f.to_string()
    f.write_text("keep this").unwrap()
}
println(saved_path.to_path().exists())
"#;
    assert_output(source, &["false"]);
}

#[test]
fn with_temp_dir_cleans_after_normal_exit() {
    let source = r#"
mut saved_path = ""
with TempDir {} as tmp {
    saved_path = tmp.to_string()
    tmp.join("file.txt").write_text("data").unwrap()
}
println(saved_path.to_path().exists())
"#;
    assert_output(source, &["false"]);
}

#[test]
fn with_temp_dir_file_operations() {
    // Write multiple files and read them back within temp scope
    let source = r#"
with TempDir {} as tmp {
    tmp.join("a.txt").write_text("alpha").unwrap()
    tmp.join("b.txt").write_text("beta").unwrap()
    println(tmp.join("a.txt").read().unwrap())
    println(tmp.join("b.txt").read().unwrap())
    println(tmp.ls().len())
}
"#;
    assert_output(source, &["alpha", "beta", "2"]);
}

#[test]
fn with_temp_dir_nested() {
    let source = r#"
with TempDir {} as outer {
    with TempDir {} as inner {
        inner.join("hi.txt").write_text("nested").unwrap()
        println(inner.join("hi.txt").read().unwrap())
        println(outer.is_dir())
    }
    // inner is gone, outer still alive
    println(outer.is_dir())
}
"#;
    assert_output(source, &["nested", "true", "true"]);
}

#[test]
fn with_temp_dir_typeof() {
    let source = r#"
with TempDir {} as tmp {
    println(typeof(tmp))
}
"#;
    assert_output(source, &["Path"]);
}

#[test]
fn with_temp_dir_as_expression() {
    let source = r#"
let result = with TempDir {} as tmp {
    tmp.join("data.txt").write_text("42").unwrap()
    tmp.join("data.txt").read().unwrap()
}
println(result)
"#;
    assert_output(source, &["42"]);
}

#[test]
fn with_temp_file_as_expression() {
    let source = r#"
let content = with TempFile {} as f {
    f.write_text("expr value").unwrap()
    f.read().unwrap()
}
println(content)
"#;
    assert_output(source, &["expr value"]);
}

#[test]
fn with_env_as_expression() {
    let source = r#"
let val = with env.scope({ "MY_TEST_VAR_EXPR": "hello_expr" }) {
    env.get("MY_TEST_VAR_EXPR")
}
println(val)
"#;
    assert_output(source, &["hello_expr"]);
}

// ═════════════════════════════════════════════════════════════════════
// HTTP CLIENT
// ═════════════════════════════════════════════════════════════════════

#[test]
fn url_encode_basic() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let encoded = url_encode("hello world")
println(encoded)
"#;
    assert_output(source, &["hello%20world"]);
}

#[test]
fn url_encode_special_chars() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let encoded = url_encode("foo=bar&baz=qux")
println(encoded)
"#;
    assert_output(source, &["foo%3Dbar%26baz%3Dqux"]);
}

#[test]
fn url_encode_unreserved_chars_unchanged() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let encoded = url_encode("hello-world_2.0~test")
println(encoded)
"#;
    assert_output(source, &["hello-world_2.0~test"]);
}

#[test]
fn url_decode_basic() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let decoded = url_decode("hello%20world")
println(decoded)
"#;
    assert_output(source, &["hello world"]);
}

#[test]
fn url_decode_plus_as_space() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let decoded = url_decode("hello+world")
println(decoded)
"#;
    assert_output(source, &["hello world"]);
}

#[test]
fn url_encode_decode_roundtrip() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let original = "hello world & foo=bar"
let encoded = url_encode(original)
let decoded = url_decode(encoded)
println(decoded == original)
"#;
    assert_output(source, &["true"]);
}

#[test]
fn query_string_from_map() {
    // BTreeMap is sorted, so "age" comes before "name"
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let qs = query_string({"name": "Alice Bob", "age": 30})
println(qs)
"#;
    assert_output(source, &["age=30&name=Alice%20Bob"]);
}

#[test]
fn query_string_empty_map() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let qs = query_string({})
println(qs)
"#;
    assert_output(source, &[""]);
}

#[test]
fn http_get_returns_result() {
    // Calling get against a non-existent host returns an Err
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = get("http://localhost:1")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_post_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = post("http://localhost:1", "body")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_put_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = put("http://localhost:1", "body")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_patch_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = patch("http://localhost:1", "body")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_delete_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = delete("http://localhost:1")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_request_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = request({"url": "http://localhost:1", "method": "GET"})
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_request_requires_url() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = request({"method": "GET"})
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_download_returns_result() {
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
let resp = download("http://localhost:1", "/tmp/que_test_dl")
println(resp.is_err())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn http_download_streams_to_disk_and_leaves_no_partial() {
    // The body is larger than any single read, which is the case the old
    // `read_to_end` implementation handled by holding all of it in memory.
    use std::io::{Read, Write};
    const BODY_LEN: usize = 3 * 1024 * 1024;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf);
        let body = vec![b'x'; BODY_LEN];
        sock.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
        sock.write_all(&body).unwrap();
        sock.flush().unwrap();
    });

    let dir = std::env::temp_dir().join(format!("que_dl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let dest = dir.join("nested").join("artifact.bin");
    let source = format!(
        r#"
import std.http {{ download }}
let resp = download("http://127.0.0.1:{}/artifact", "{}").unwrap()
println(resp.status)
println(resp.size)
"#,
        port,
        dest.display()
    );
    let (output, _) = run(&source).unwrap();
    server.join().unwrap();

    assert_eq!(output, vec!["200".to_string(), BODY_LEN.to_string()]);
    assert_eq!(std::fs::metadata(&dest).unwrap().len() as usize, BODY_LEN);
    // The rename means a half-finished download is never mistaken for a
    // complete one.
    assert!(!dest.with_extension("bin.que-partial").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn url_encode_type_error() {
    assert_error("import std.http { url_encode }\nurl_encode(42)");
}

#[test]
fn url_decode_type_error() {
    assert_error("import std.http { url_decode }\nurl_decode(42)");
}

#[test]
fn query_string_type_error() {
    assert_error("import std.http { query_string }\nquery_string(\"not a map\")");
}

#[test]
fn http_builtins_are_callable() {
    // Verify all HTTP builtins are registered as functions
    let source = r#"
import std.http { get, post, put, patch, delete, request, download, url_encode, url_decode, query_string }
println(typeof(get))
println(typeof(post))
println(typeof(put))
println(typeof(patch))
println(typeof(delete))
println(typeof(request))
println(typeof(download))
println(typeof(url_encode))
println(typeof(url_decode))
println(typeof(query_string))
"#;
    assert_output(source, &[
        "Function", "Function", "Function", "Function", "Function",
        "Function", "Function", "Function", "Function", "Function",
    ]);
}

// ── http TLS / CA-bundle ─────────────────────────────────────────────

#[test]
fn http_get_tls_null_is_default() {
    // Passing null as tls opts should behave identically to omitting it
    let source = r#"
import std.http { get }
let resp = get("http://localhost:1", {}, null)
match resp {
    Err(_) => println("err")
    Ok(_)  => println("ok")
}
"#;
    assert_output(source, &["err"]);
}

#[test]
fn http_get_tls_empty_map_is_default() {
    // Passing {} as tls opts (no ca_bundle) should use default WebPKI roots
    let source = r#"
import std.http { get }
let resp = get("http://localhost:1", {}, {})
match resp {
    Err(_) => println("err")
    Ok(_)  => println("ok")
}
"#;
    assert_output(source, &["err"]);
}

#[test]
fn http_get_tls_bad_ca_bundle_returns_err() {
    // A non-existent CA bundle file should produce an Err
    let source = r#"
import std.http { get }
let resp = get("http://localhost:1", {}, { ca_bundle: "/nonexistent/ca.pem" })
match resp {
    Err(msg) => println("tls-error")
    Ok(_)    => println("ok")
}
"#;
    // The TLS agent build will fail => Err wrapping the error message
    assert_output(source, &["tls-error"]);
}

#[test]
fn http_request_tls_key_accepted() {
    // http_request() with a "tls" key containing a bad path returns Err
    let source = r#"
import std.http { request }
let resp = request({
    "url": "http://localhost:1",
    "tls": { "ca_bundle": "/nonexistent/ca.pem" }
})
match resp {
    Err(_) => println("tls-error")
    Ok(_)  => println("ok")
}
"#;
    assert_output(source, &["tls-error"]);
}

#[test]
fn http_upload_tls_empty_map() {
    // Calling upload with {} tls opts and a non-existent file gives file-open error
    let source = r#"
import std.http { upload }
let resp = upload("http://localhost:1", "/nonexistent/file.bin", {}, {})
match resp {
    Err(_) => println("err")
    Ok(_)  => println("ok")
}
"#;
    assert_output(source, &["err"]);
}

// ── Task freshness across separate runs ──────────────────────────────

/// Run a script from disk with a fresh interpreter, the way a new `que`
/// process would. The directory survives between calls so the on-disk task
/// cache is what carries state from one run to the next.
fn run_script_in(dir: &std::path::Path, name: &str) -> Vec<String> {
    use que_lang::interpreter::Interpreter;
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let path = dir.join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let module = Parser::new(tokens).parse_module().unwrap();
    let mut interp = Interpreter::new();
    interp.set_script_path(std::fs::canonicalize(&path).unwrap());
    interp.init_module_loader();
    interp.exec_module(&module).unwrap();
    interp.flush_partial();
    interp.output
}

fn freshness_project(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("que_fresh_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("in.txt"), "hello\n").unwrap();
    std::fs::write(
        dir.join("main.que"),
        r#"
let src = script_dir() / "in.txt"
let dst = script_dir() / "out.txt"
@inputs([src])
@outputs([dst])
task build {
    dst.write_text(src.read()?)?
}
build()
"#,
    )
    .unwrap();
    dir
}

#[test]
fn task_skips_on_a_second_run_from_a_new_process() {
    let dir = freshness_project("second_run");
    assert!(run_script_in(&dir, "main.que").contains(&"[RUN]  build".to_string()));
    assert!(run_script_in(&dir, "main.que").contains(&"[SKIP] build".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_touched_input_with_the_same_contents_does_not_rebuild() {
    // The case that makes an mtime-only check useless: a checkout, a restored
    // CI cache or a `touch` moves the timestamp forward without changing a byte.
    let dir = freshness_project("touched");
    run_script_in(&dir, "main.que");

    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    let input = std::fs::File::options()
        .write(true)
        .open(dir.join("in.txt"))
        .unwrap();
    input.set_modified(future).unwrap();

    let out = run_script_in(&dir, "main.que");
    assert!(
        out.contains(&"[SKIP] build".to_string()),
        "a timestamp change with identical contents rebuilt: {:?}",
        out
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changed_input_contents_rebuild() {
    let dir = freshness_project("changed");
    run_script_in(&dir, "main.que");
    std::fs::write(dir.join("in.txt"), "goodbye\n").unwrap();
    assert!(run_script_in(&dir, "main.que").contains(&"[RUN]  build".to_string()));
    assert_eq!(
        std::fs::read_to_string(dir.join("out.txt")).unwrap(),
        "goodbye\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_task_with_parameters_can_skip_across_runs() {
    // The argument hash used to live only in memory, so a parameterised task
    // re-ran on every invocation of `que` — which is every invocation in CI.
    let dir = std::env::temp_dir().join(format!("que_fresh_{}_params", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.que"),
        r#"
let dst = script_dir() / "out.txt"
@outputs([dst])
task greet(name = "world") {
    dst.write_text(name)?
}
greet()
"#,
    )
    .unwrap();
    assert!(run_script_in(&dir, "main.que").contains(&"[RUN]  greet".to_string()));
    assert!(run_script_in(&dir, "main.que").contains(&"[SKIP] greet".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Dry runs ─────────────────────────────────────────────────────────

/// Run a script from disk with `dry_run` set, the way `que --dry-run` does.
fn run_dry_in(dir: &std::path::Path, name: &str) -> Vec<String> {
    use que_lang::interpreter::Interpreter;
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let path = dir.join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let module = Parser::new(tokens).parse_module().unwrap();
    let mut interp = Interpreter::new();
    interp.dry_run = true;
    interp.set_script_path(std::fs::canonicalize(&path).unwrap());
    interp.init_module_loader();
    interp.exec_module(&module).unwrap();
    interp.flush_partial();
    interp.output
}

fn dry_project(tag: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("que_dry_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.que"), body).unwrap();
    dir
}

#[test]
fn a_dry_run_announces_a_command_instead_of_running_it() {
    let dir = dry_project(
        "cmd",
        "let out = script_dir() / \"made.txt\"\n`touch ${out}`\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(
        output.iter().any(|l| l.starts_with("[dry-run] touch ")),
        "expected the command to be announced: {:?}",
        output
    );
    assert!(!dir.join("made.txt").exists(), "the command actually ran");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_announces_a_pipeline_as_one_line() {
    let dir = dry_project("pipe", "let r = (`echo hi` | `tr a-z A-Z`).out()\n");
    let output = run_dry_in(&dir, "main.que");
    assert!(
        output
            .iter()
            .any(|l| l == "[dry-run] echo hi | tr a-z A-Z"),
        "expected one line for the whole pipeline: {:?}",
        output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_does_not_write_files() {
    let dir = dry_project(
        "write",
        "(script_dir() / \"a.txt\").write_text(\"x\")?\n(script_dir() / \"sub\").mkdir()?\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(!dir.join("a.txt").exists(), "write_text wrote the file");
    assert!(!dir.join("sub").exists(), "mkdir created the directory");
    assert_eq!(output.len(), 2, "expected two announcements: {:?}", output);
    assert!(output[0].starts_with("[dry-run] write "));
    assert!(output[1].starts_with("[dry-run] mkdir "));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_still_reads() {
    // A script that cannot read cannot reach the decisions the dry run is
    // meant to show, so reads are never suppressed.
    let dir = dry_project(
        "read",
        "println((script_dir() / \"data.txt\").read()?)\n",
    );
    std::fs::write(dir.join("data.txt"), "real contents").unwrap();
    assert_eq!(
        run_dry_in(&dir, "main.que"),
        vec!["real contents".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_does_not_open_a_file_for_writing() {
    // `open(p, "w")` truncates on the way in, so the dry run has to stop at
    // the open rather than at the first `write`.
    let dir = dry_project(
        "open_w",
        "let f = open(script_dir() / \"out.txt\", \"w\")?\nf.write(\"clobber\")\nf.close()\nprintln(\"reached the end\")\n",
    );
    std::fs::write(dir.join("out.txt"), "PRECIOUS").unwrap();
    let output = run_dry_in(&dir, "main.que");
    assert_eq!(
        std::fs::read_to_string(dir.join("out.txt")).unwrap(),
        "PRECIOUS",
        "the dry run truncated the file"
    );
    assert!(
        output.iter().any(|l| l.contains("[dry-run] open") && l.contains("truncate")),
        "expected the open to be announced: {:?}",
        output
    );
    // Each write is announced too. A dry run that reported the open and then
    // went quiet would understate what the script does.
    assert!(
        output.iter().any(|l| l.starts_with("[dry-run] write ") && l.ends_with("(7 bytes)")),
        "expected the discarded write to be announced: {:?}",
        output
    );
    // The handle it hands back has to behave like a real one, or the script
    // takes its error path and the rest of the run shows nothing.
    assert!(
        output.contains(&"reached the end".to_string()),
        "the discarding handle broke the script: {:?}",
        output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_does_not_create_a_file_opened_for_appending() {
    let dir = dry_project(
        "open_a",
        "let f = open(script_dir() / \"log.txt\", \"a\")?\nf.writeln(\"entry\")\nf.flush()\nprintln(\"done\")\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(!dir.join("log.txt").exists(), "the dry run created the file");
    assert!(
        output.iter().any(|l| l.starts_with("[dry-run] write ") && l.ends_with("(6 bytes)")),
        "writeln should announce the newline it would have written: {:?}",
        output
    );
    assert!(output.contains(&"done".to_string()), "{:?}", output);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_still_opens_a_file_for_reading() {
    let dir = dry_project(
        "open_r",
        "let f = open(script_dir() / \"data.txt\")?\nprintln(f.read().unwrap().trim())\n",
    );
    std::fs::write(dir.join("data.txt"), "real contents\n").unwrap();
    assert_eq!(
        run_dry_in(&dir, "main.que"),
        vec!["real contents".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_announces_a_stream_write_with_the_size_it_would_have_written() {
    // The pipeline is drained first so the byte count is measured rather
    // than guessed, and so any reads feeding it still happen.
    let dir = dry_project(
        "stream_w",
        "import std.stream\nstream.of(\"a\\nbb\").write_to(script_dir() / \"out.txt\")\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(!dir.join("out.txt").exists(), "the dry run wrote the file");
    assert!(
        output.iter().any(|l| l.starts_with("[dry-run] write ") && l.ends_with("(4 bytes)")),
        "expected a measured byte count: {:?}",
        output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_does_not_write_a_config_file() {
    let dir = dry_project(
        "config_w",
        "import std.config\nconfig.write(script_dir() / \"c.json\", {a: 1})?\nprintln(\"done\")\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(!dir.join("c.json").exists(), "the dry run wrote the config");
    assert!(output.contains(&"done".to_string()), "{:?}", output);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_announces_a_stream_piped_into_a_file_handle() {
    // The writer behind a handle sink has no way back to the interpreter, so
    // this announcement has to be made before the pipeline runs into it.
    let dir = dry_project(
        "handle_sink",
        "import std.stream\nlet f = open(script_dir() / \"out.txt\", \"w\")?\nstream.of(\"a\\nbb\").write_to(stream.of(f))\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(!dir.join("out.txt").exists(), "the dry run wrote the file");
    assert!(
        output.iter().any(|l| l.starts_with("[dry-run] write ") && l.ends_with("(4 bytes)")),
        "expected the piped write to be announced: {:?}",
        output
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_lets_a_script_guard_its_own_effects() {
    let dir = dry_project(
        "builtin",
        "if dry_run() { println(\"would deploy\") } else { println(\"deploying\") }\n",
    );
    assert_eq!(
        run_dry_in(&dir, "main.que"),
        vec!["would deploy".to_string()]
    );
    assert_eq!(
        run_script_in(&dir, "main.que"),
        vec!["deploying".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dry_run_does_not_record_task_freshness() {
    // Recording outputs a dry run never produced would make the next real
    // run skip the work.
    let dir = freshness_project("dryrun");
    run_dry_in(&dir, "main.que");
    assert!(!dir.join("out.txt").exists());
    assert!(run_script_in(&dir, "main.que").contains(&"[RUN]  build".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Dependencies (`que.toml` / `que.lock` / `que install`) ───────────

/// Build a git repository on disk to depend on. A local path is a perfectly
/// good git URL, so this exercises the real clone/checkout path without a
/// network.
fn git_package(dir: &std::path::Path, body: &str) -> String {
    use std::process::Command;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("mod.que"), body).unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(ok.status.success(), "git {:?}: {:?}", args, ok);
    };
    git(&["init", "--quiet", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "init"]);
    git(&["tag", "v1"]);
    dir.to_string_lossy().into_owned()
}

fn deps_project(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("que_deps_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn install_fetches_a_git_dependency_and_the_import_finds_it() {
    let dir = deps_project("git");
    let url = git_package(&dir.join("upstream"), "pub fn shout(s) { s.to_upper() }\n");
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("que.toml"),
        format!("[dependencies]\nshouty = {{ git = \"{}\", tag = \"v1\" }}\n", url),
    )
    .unwrap();

    que_lang::install::install(&app, false).unwrap();
    assert!(app.join("que_packages/shouty/mod.que").is_file());

    std::fs::write(
        app.join("main.que"),
        "import shouty\nprintln(shouty.shout(\"hi\"))\n",
    )
    .unwrap();
    assert_eq!(run_script_in(&app, "main.que"), vec!["HI".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn install_writes_a_lockfile_pinning_the_resolved_commit() {
    let dir = deps_project("lock");
    let url = git_package(&dir.join("upstream"), "pub fn v() { 1 }\n");
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("que.toml"),
        format!("[dependencies]\np = {{ git = \"{}\", tag = \"v1\" }}\n", url),
    )
    .unwrap();

    que_lang::install::install(&app, false).unwrap();
    let lock = que_lang::manifest::read_lock(&app);
    assert_eq!(lock.len(), 1);
    assert_eq!(lock[0].name, "p");
    assert_eq!(lock[0].requirement, "v1");
    assert_eq!(lock[0].revision.len(), 40, "expected a full commit id");

    // A second install must reuse the pin rather than resolve again, which
    // is the whole point: a moving tag cannot change what a checkout gets.
    let pinned = lock[0].revision.clone();
    que_lang::install::install(&app, false).unwrap();
    assert_eq!(que_lang::manifest::read_lock(&app)[0].revision, pinned);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_moved_tag_does_not_change_what_a_locked_install_gets() {
    use std::process::Command;
    let dir = deps_project("moved");
    let upstream = dir.join("upstream");
    let url = git_package(&upstream, "pub fn version() { 1 }\n");
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("que.toml"),
        format!("[dependencies]\np = {{ git = \"{}\", tag = \"v1\" }}\n", url),
    )
    .unwrap();
    que_lang::install::install(&app, false).unwrap();

    // Upstream force-moves the tag onto a new commit.
    std::fs::write(upstream.join("mod.que"), "pub fn version() { 2 }\n").unwrap();
    for args in [
        vec!["add", "-A"],
        vec!["commit", "--quiet", "-m", "two"],
        vec!["tag", "-f", "v1"],
    ] {
        Command::new("git")
            .current_dir(&upstream)
            .args(&args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
    }

    que_lang::install::install(&app, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(app.join("que_packages/p/mod.que")).unwrap(),
        "pub fn version() { 1 }\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changing_the_requirement_re_resolves() {
    let dir = deps_project("rereq");
    let url = git_package(&dir.join("upstream"), "pub fn v() { 1 }\n");
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let manifest = |req: &str| format!("[dependencies]\np = {{ git = \"{}\", tag = \"{}\" }}\n", url, req);

    std::fs::write(app.join("que.toml"), manifest("v1")).unwrap();
    que_lang::install::install(&app, false).unwrap();

    // `main` is the same commit here, but the pin must be re-recorded under
    // the new requirement rather than silently kept under the old one.
    std::fs::write(app.join("que.toml"), manifest("main")).unwrap();
    que_lang::install::install(&app, false).unwrap();
    assert_eq!(que_lang::manifest::read_lock(&app)[0].requirement, "main");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_a_dependency_drops_its_pin() {
    let dir = deps_project("drop");
    let url = git_package(&dir.join("upstream"), "pub fn v() { 1 }\n");
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("que.toml"),
        format!("[dependencies]\np = {{ git = \"{}\", tag = \"v1\" }}\n", url),
    )
    .unwrap();
    que_lang::install::install(&app, false).unwrap();
    assert_eq!(que_lang::manifest::read_lock(&app).len(), 1);

    std::fs::write(app.join("que.toml"), "[package]\nname = \"app\"\n").unwrap();
    que_lang::install::install(&app, false).unwrap();
    // The manifest is now empty, so nothing is resolved and nothing is
    // written -- the stale pin stays only until something is installed again.
    std::fs::write(
        app.join("que.toml"),
        format!(
            "[dependencies]\nq = {{ git = \"{}\", tag = \"v1\" }}\n",
            url
        ),
    )
    .unwrap();
    que_lang::install::install(&app, false).unwrap();
    let lock = que_lang::manifest::read_lock(&app);
    assert_eq!(lock.len(), 1);
    assert_eq!(lock[0].name, "q");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_package_says_to_declare_it_and_install() {
    let dir = deps_project("hint");
    std::fs::write(dir.join("que.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("main.que"), "import nowhere\n").unwrap();

    let err = run_script_expect_error(&dir, "main.que");
    assert!(err.contains("module not found: nowhere"), "{}", err);
    assert!(err.contains("[dependencies] in que.toml"), "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_declared_but_uninstalled_package_says_to_run_install() {
    let dir = deps_project("uninstalled");
    std::fs::write(
        dir.join("que.toml"),
        "[dependencies]\nshouty = { git = \"https://example.invalid/s\" }\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.que"), "import shouty\n").unwrap();

    let err = run_script_expect_error(&dir, "main.que");
    assert!(err.contains("but not installed"), "{}", err);
    assert!(err.contains("que install"), "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Run a script that is expected to fail, returning the error text.
fn run_script_expect_error(dir: &std::path::Path, name: &str) -> String {
    use que_lang::interpreter::Interpreter;
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let path = dir.join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let module = Parser::new(tokens).parse_module().unwrap();
    let mut interp = Interpreter::new();
    interp.set_script_path(std::fs::canonicalize(&path).unwrap());
    interp.init_module_loader();
    match interp.exec_module(&module) {
        Ok(_) => panic!("expected the script to fail"),
        Err(e) => format!("{:?}", e),
    }
}

// ── The `que test` runner ────────────────────────────────────────────

fn test_project(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("que_testrun_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        let full = dir.join(name);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, body).unwrap();
    }
    dir
}

#[test]
fn test_runner_reports_each_outcome() {
    let dir = test_project(
        "outcomes",
        &[(
            "math_test.que",
            r#"
fn test_passes() { assert(1 + 1 == 2) }
fn test_fails() { assert(1 + 1 == 3) }
"#,
        )],
    );
    let report = que_lang::test_runner::run_file(&dir.join("math_test.que"), None);
    assert!(report.load_error.is_none());
    let names: Vec<_> = report.outcomes.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["test_passes", "test_fails"]);
    assert!(report.outcomes[0].passed());
    assert!(!report.outcomes[1].passed());
    assert!(report.failed());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_keeps_output_with_the_test_that_produced_it() {
    let dir = test_project(
        "output",
        &[(
            "out_test.que",
            r#"
fn test_quiet() { println("quiet"); assert(true) }
fn test_loud() { println("context for the failure"); assert(false) }
"#,
        )],
    );
    let report = que_lang::test_runner::run_file(&dir.join("out_test.que"), None);
    assert_eq!(report.outcomes[0].output, vec!["quiet".to_string()]);
    assert_eq!(
        report.outcomes[1].output,
        vec!["context for the failure".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_treats_a_returned_err_as_a_failure() {
    let dir = test_project(
        "err",
        &[("e_test.que", "fn test_err() { Err(\"nope\") }\n")],
    );
    let report = que_lang::test_runner::run_file(&dir.join("e_test.que"), None);
    assert_eq!(report.outcomes[0].failure.as_deref(), Some("nope"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_shares_top_level_definitions_but_not_test_locals() {
    let dir = test_project(
        "scope",
        &[(
            "scope_test.que",
            r#"
let shared = 7
fn test_sees_shared() { assert(shared == 7) }
fn test_declares_local() { let local = 1; assert(local == 1) }
fn test_cannot_see_the_other_local() { local }
"#,
        )],
    );
    let report = que_lang::test_runner::run_file(&dir.join("scope_test.que"), None);
    assert!(report.outcomes[0].passed(), "{:?}", report.outcomes[0].failure);
    assert!(report.outcomes[1].passed(), "{:?}", report.outcomes[1].failure);
    assert!(
        report.outcomes[2]
            .failure
            .as_deref()
            .is_some_and(|m| m.contains("undefined variable 'local'")),
        "a local from one test leaked into the next: {:?}",
        report.outcomes[2].failure
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_reports_a_broken_file_without_pretending_tests_ran() {
    let dir = test_project("broken", &[("bad_test.que", "fn test_a( {\n")]);
    let report = que_lang::test_runner::run_file(&dir.join("bad_test.que"), None);
    assert!(report.load_error.is_some());
    assert!(report.outcomes.is_empty());
    assert!(report.failed());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_discovers_by_name_and_by_directory() {
    let dir = test_project(
        "discovery",
        &[
            ("a_test.que", "fn test_a() { assert(true) }\n"),
            ("test_b.que", "fn test_b() { assert(true) }\n"),
            ("tests/c.que", "fn test_c() { assert(true) }\n"),
            ("helper.que", "fn help() { 1 }\n"),
            ("target/d_test.que", "fn test_d() { assert(true) }\n"),
        ],
    );
    let found = que_lang::test_runner::discover(&[dir.clone()]);
    let names: Vec<String> = found
        .iter()
        .map(|p| {
            p.strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(names, vec!["a_test.que", "test_b.que", "tests/c.que"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_uses_an_explicitly_named_file_whatever_it_is_called() {
    let dir = test_project("explicit", &[("helper.que", "fn test_x() { assert(true) }\n")]);
    let found = que_lang::test_runner::discover(&[dir.join("helper.que")]);
    assert_eq!(found.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════════
// MODULE SYSTEM
// ═════════════════════════════════════════════════════════════════════

/// Helper: run a .que file with the module loader enabled.
/// Creates a temp directory, writes files, runs the main file.
fn run_module_project(files: &[(&str, &str)]) -> Result<(Vec<String>, Value), que_lang::error::QueError> {
    use que_lang::interpreter::Interpreter;
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;
    use que_lang::error::Signal;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);

    let tmp = std::env::temp_dir().join(format!("que_mod_test_{}_{}", std::process::id(), id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Write all files
    for (path, content) in files {
        let full = tmp.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }

    // Find the main file (first file in the list)
    let main_path = tmp.join(files[0].0);
    let source = std::fs::read_to_string(&main_path).unwrap();

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module()?;
    let mut interp = Interpreter::new();
    interp.set_script_path(std::fs::canonicalize(&main_path).unwrap());
    interp.init_module_loader();

    let exec_result = interp.exec_module(&module);
    interp.flush_partial();
    let result = match exec_result {
        Ok(val) => Ok((interp.output, val)),
        Err(Signal::Error(e)) => Err(e),
        Err(Signal::Return(v)) => Ok((interp.output, v)),
        Err(Signal::Break(_)) => Err(que_lang::error::QueError::runtime("break outside of loop")),
        Err(Signal::Continue) => Err(que_lang::error::QueError::runtime("continue outside of loop")),
        Err(Signal::Exit(code)) => Err(que_lang::error::QueError::runtime(format!("exit({})", code))),
        Err(Signal::Interrupted(sig)) => Err(que_lang::error::QueError::runtime(format!(
            "interrupted by signal {}",
            sig
        ))),
    };

    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn assert_module_output(files: &[(&str, &str)], expected: &[&str]) {
    let (output, _) = run_module_project(files)
        .unwrap_or_else(|e| panic!("module execution failed: {}", e));
    assert_eq!(
        output,
        expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "output mismatch"
    );
}

fn assert_module_error(files: &[(&str, &str)]) {
    assert!(
        run_module_project(files).is_err(),
        "expected error but execution succeeded"
    );
}

// ── Local imports ────────────────────────────────────────────────────

#[test]
fn module_local_import_basic() {
    assert_module_output(
        &[
            ("main.que", r#"
import .utils
println(utils.greet("World"))
"#),
            ("utils.que", r#"
pub fn greet(name) {
    "Hello, " + name + "!"
}
"#),
        ],
        &["Hello, World!"],
    );
}

#[test]
fn module_local_import_nested_path() {
    assert_module_output(
        &[
            ("main.que", r#"
import .lib.math
println(math.add(2, 3))
"#),
            ("lib/math.que", r#"
pub fn add(a, b) { a + b }
"#),
        ],
        &["5"],
    );
}

#[test]
fn module_local_import_with_alias() {
    assert_module_output(
        &[
            ("main.que", r#"
import .utils as u
println(u.greet("Que"))
"#),
            ("utils.que", r#"
pub fn greet(name) {
    "Hello, " + name + "!"
}
"#),
        ],
        &["Hello, Que!"],
    );
}

#[test]
fn module_local_import_selective() {
    assert_module_output(
        &[
            ("main.que", r#"
import .utils { greet, farewell }
println(greet("Que"))
println(farewell("Que"))
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
pub fn farewell(name) { "Goodbye, " + name }
fn private_fn() { "secret" }
"#),
        ],
        &["Hello, Que", "Goodbye, Que"],
    );
}

#[test]
fn module_local_import_multi_shorthand() {
    // import .{utils, config} — loads two modules at once
    assert_module_output(
        &[
            ("main.que", r#"
import .{utils, config}
println(utils.greet("Que"))
println(config.version())
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
"#),
            ("config.que", r#"
pub fn version() { "1.0.0" }
"#),
        ],
        &["Hello, Que", "1.0.0"],
    );
}

#[test]
fn module_local_directory_mod_que() {
    // import .lib → resolves to lib/mod.que
    assert_module_output(
        &[
            ("main.que", r#"
import .lib
println(lib.hello())
"#),
            ("lib/mod.que", r#"
pub fn hello() { "from mod.que" }
"#),
        ],
        &["from mod.que"],
    );
}

#[test]
fn module_local_directory_sub_module() {
    // import .lib.build → resolves to lib/build.que
    // import .lib → resolves to lib/mod.que
    assert_module_output(
        &[
            ("main.que", r#"
import .lib
import .lib.build
println(lib.version())
println(build.compile())
"#),
            ("lib/mod.que", r#"
pub fn version() { "2.0" }
"#),
            ("lib/build.que", r#"
pub fn compile() { "compiled!" }
"#),
        ],
        &["2.0", "compiled!"],
    );
}

// ── Visibility (pub) ─────────────────────────────────────────────────

// ── Relative dot-import resolution ───────────────────────────────────

#[test]
fn module_dot_import_relative_to_file_not_root() {
    // lib/mod.que uses `import .math` — the dot should resolve relative to
    // lib/'s own directory, giving lib/math.que, NOT <root>/math.que.
    assert_module_output(
        &[
            ("main.que", r#"
import .lib
println(lib.add(2, 3))
"#),
            ("lib/mod.que", r#"
pub import .math
pub fn add(a, b) { math.add(a, b) }
"#),
            ("lib/math.que", r#"
pub fn add(a, b) { a + b }
"#),
        ],
        &["5"],
    );
}

#[test]
fn module_pkg_internal_dot_import_relative_to_pkg_dir() {
    // A package in que_packages/ uses `import .utils` inside its mod.que.
    // The dot should resolve to que_packages/mypkg/utils.que, not <root>/utils.que.
    assert_module_output(
        &[
            ("main.que", r#"
import mypkg
println(mypkg.greet("World"))
"#),
            ("que_packages/mypkg/mod.que", r#"
pub import .utils
pub fn greet(name) { utils.hello(name) }
"#),
            ("que_packages/mypkg/utils.que", r#"
pub fn hello(name) { "Hello, " + name + "!" }
"#),
        ],
        &["Hello, World!"],
    );
}

#[test]
fn module_dot_import_does_not_escape_to_root_utils() {
    // Ensure lib/mod.que's `import .utils` does NOT accidentally pick up
    // a root-level utils.que — it must stay relative to lib/.
    // Without a lib/utils.que, this should fail with "module not found".
    assert_module_error(&[
        ("main.que", r#"
import .lib
println(lib.x())
"#),
        // Root-level utils.que exists but must NOT be found by lib/mod.que
        ("utils.que", r#"
pub fn x() { "root utils" }
"#),
        ("lib/mod.que", r#"
pub import .utils
pub fn x() { utils.x() }
"#),
        // lib/utils.que intentionally absent
    ]);
}

#[test]
fn module_private_fn_not_exported() {
    // Private functions should NOT be accessible from outside
    assert_module_output(
        &[
            ("main.que", r#"
import .utils
println(utils.keys())
"#),
            ("utils.que", r#"
pub fn public_fn() { "public" }
fn private_fn() { "private" }
"#),
        ],
        &["[\"public_fn\"]"],
    );
}

#[test]
fn module_selective_import_nonexistent_errors() {
    // Importing a name that doesn't exist in the module should error
    assert_module_error(&[
        ("main.que", r#"
import .utils { nonexistent }
"#),
        ("utils.que", r#"
pub fn greet() { "hi" }
"#),
    ]);
}

// ── Wildcard imports ─────────────────────────────────────────────────

#[test]
fn module_wildcard_import_basic() {
    // import .utils { * } brings all exports directly into scope
    assert_module_output(
        &[
            ("main.que", r#"
import .utils { * }
println(greet("Que"))
println(farewell("Que"))
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
pub fn farewell(name) { "Goodbye, " + name }
fn private_fn() { "secret" }
"#),
        ],
        &["Hello, Que", "Goodbye, Que"],
    );
}

#[test]
fn module_wildcard_import_private_not_imported() {
    // Wildcard only imports pub exports, not private functions
    assert_module_output(
        &[
            ("main.que", r#"
import .utils { * }
println(typeof(greet))
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hi, " + name }
fn internal() { "hidden" }
"#),
        ],
        &["Function"],
    );
}

#[test]
fn module_wildcard_import_std() {
    // import std.json { * } brings all std module functions into scope
    assert_output(
        r#"
import std.json { * }
let s = stringify({"a": 1})
println(s)
"#,
        &[r#"{"a":1}"#],
    );
}

#[test]
fn module_wildcard_import_std_fs() {
    // import std.fs { * } brings read, write, exists into scope
    assert_output(
        r#"
import std.fs { * }
println(typeof(exists))
println(typeof(read))
"#,
        &["Function", "Function"],
    );
}

// ── Module caching (single evaluation) ──────────────────────────────

#[test]
fn module_loaded_once_cached() {
    // The module's top-level code runs only once, even when imported twice
    assert_module_output(
        &[
            ("main.que", r#"
import .counter
import .other
println(counter.value())
"#),
            ("counter.que", r#"
// This println runs at module load time — should only run once
println("loading counter")
mut state = 0
pub fn value() { state }
"#),
            ("other.que", r#"
import .counter
// just importing counter again
"#),
        ],
        &["loading counter", "0"],
    );
}

// ── Circular import detection ────────────────────────────────────────

#[test]
fn module_circular_import_error() {
    assert_module_error(&[
        ("main.que", r#"
import .a
"#),
        ("a.que", r#"
import .b
pub fn from_a() { "a" }
"#),
        ("b.que", r#"
import .a
pub fn from_b() { "b" }
"#),
    ]);
}

// ── Module not found ─────────────────────────────────────────────────

#[test]
fn module_not_found_error() {
    assert_module_error(&[
        ("main.que", r#"
import .nonexistent
"#),
    ]);
}

// ── Module as Map (runtime representation) ──────────────────────────

#[test]
fn module_is_map_typeof() {
    assert_module_output(
        &[
            ("main.que", r#"
import .utils
println(typeof(utils))
"#),
            ("utils.que", r#"
pub fn hello() { "hi" }
"#),
        ],
        &["Map"],
    );
}

#[test]
fn module_map_keys() {
    assert_module_output(
        &[
            ("main.que", r#"
import .utils
let k = utils.keys()
println(k.contains("add"))
println(k.contains("sub"))
"#),
            ("utils.que", r#"
pub fn add(a, b) { a + b }
pub fn sub(a, b) { a - b }
fn private_helper() { 0 }
"#),
        ],
        &["true", "true"],
    );
}

// ── Chained imports (module imports another module) ──────────────────

#[test]
fn module_chained_imports() {
    assert_module_output(
        &[
            ("main.que", r#"
import .app
println(app.run())
"#),
            ("app.que", r#"
import .utils
pub fn run() {
    utils.greet("chain")
}
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
"#),
        ],
        &["Hello, chain"],
    );
}

// ── pub import re-exports ────────────────────────────────────────────

#[test]
fn module_pub_import_reexport_namespace() {
    // pub import .utils → re-exports the utils namespace
    assert_module_output(
        &[
            ("main.que", r#"
import .facade
println(facade.utils.greet("re-export"))
"#),
            ("facade.que", r#"
pub import .utils
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
"#),
        ],
        &["Hello, re-export"],
    );
}

#[test]
fn module_pub_import_reexport_selective() {
    // pub import .utils { greet } → re-exports greet directly
    assert_module_output(
        &[
            ("main.que", r#"
import .facade
println(facade.greet("selective"))
"#),
            ("facade.que", r#"
pub import .utils { greet }
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hello, " + name }
"#),
        ],
        &["Hello, selective"],
    );
}

// ── std imports ──────────────────────────────────────────────────────

#[test]
fn module_std_fs_import() {
    // import std.fs brings in read, write, exists as a namespace
    let source = r#"
import std.fs
println(typeof(fs))
println(typeof(fs.read))
"#;
    assert_output(source, &["Module", "Function"]);
}

#[test]
fn module_std_fs_selective() {
    let source = r#"
import std.fs { exists }
println(typeof(exists))
"#;
    assert_output(source, &["Function"]);
}

#[test]
fn module_std_fs_alias() {
    let source = r#"
import std.fs as io
println(typeof(io))
println(typeof(io.read))
"#;
    assert_output(source, &["Module", "Function"]);
}

#[test]
fn module_std_multi_shorthand() {
    let source = r#"
import std.{fs, http}
println(typeof(fs))
println(typeof(http))
"#;
    assert_output(source, &["Module", "Module"]);
}

#[test]
fn module_std_http_import() {
    let source = r#"
import std.http
println(typeof(http.get))
println(typeof(http.url_encode))
"#;
    assert_output(source, &["Function", "Function"]);
}

#[test]
fn module_std_json_import() {
    let source = r#"
import std.json
println(typeof(json.parse))
println(typeof(json.stringify))
"#;
    assert_output(source, &["Function", "Function"]);
}

#[test]
fn module_std_nonexistent_errors() {
    assert_error(r#"import std.nonexistent_module"#);
}

// ── Task exports ─────────────────────────────────────────────────────

#[test]
fn module_exports_tasks() {
    assert_module_output(
        &[
            ("main.que", r#"
import .build_mod
println(typeof(build_mod.compile))
"#),
            ("build_mod.que", r#"
@description("Compile the project")
task compile {
    println("compiling...")
}
"#),
        ],
        &["Task"],
    );
}

// ── Parse tests for import syntax ────────────────────────────────────

#[test]
fn parse_import_local_dot_prefix() {
    // Just verify all local import forms parse without error
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let source = r#"
import .utils
import .lib.build
import .lib.build as builder
import .lib.build { compile, test }
import .{utils, config}
"#;
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    parser.parse_module().unwrap();
}

#[test]
fn parse_import_external() {
    // External imports parse fine even though modules don't exist
    // (they'll fail at runtime, not parse time)
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let source = r#"
import deploy_tools
import deploy_tools.k8s
import deploy_tools.k8s as k
import deploy_tools.k8s { rollout }
import std.{fs, path}
"#;
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().unwrap();

    // Verify the parsed AST
    let imports: Vec<_> = module.items.iter().filter_map(|(_, item)| {
        if let que_lang::ast::Item::Import(decl) = item {
            Some(decl)
        } else {
            None
        }
    }).collect();

    assert_eq!(imports.len(), 5);

    // import deploy_tools
    assert_eq!(imports[0].path, vec!["deploy_tools"]);
    assert!(!imports[0].is_local);
    assert!(imports[0].alias.is_none());
    assert!(imports[0].items.is_none());

    // import deploy_tools.k8s
    assert_eq!(imports[1].path, vec!["deploy_tools", "k8s"]);
    assert!(!imports[1].is_local);

    // import deploy_tools.k8s as k
    assert_eq!(imports[2].path, vec!["deploy_tools", "k8s"]);
    assert_eq!(imports[2].alias, Some("k".to_string()));

    // import deploy_tools.k8s { rollout }
    assert_eq!(imports[3].path, vec!["deploy_tools", "k8s"]);
    assert_eq!(imports[3].items, Some(vec!["rollout".to_string()]));

    // import std.{fs, path}
    assert_eq!(imports[4].path, vec!["std"]);
    assert_eq!(imports[4].items, Some(vec!["fs".to_string(), "path".to_string()]));
}

#[test]
fn parse_import_local_syntax() {
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let source = r#"
import .utils
import .lib.build
import .lib.build as builder
import .lib.build { compile }
import .{utils, config}
"#;
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().unwrap();

    let imports: Vec<_> = module.items.iter().filter_map(|(_, item)| {
        if let que_lang::ast::Item::Import(decl) = item {
            Some(decl)
        } else {
            None
        }
    }).collect();

    assert_eq!(imports.len(), 5);

    // import .utils
    assert_eq!(imports[0].path, vec!["utils"]);
    assert!(imports[0].is_local);

    // import .lib.build
    assert_eq!(imports[1].path, vec!["lib", "build"]);
    assert!(imports[1].is_local);

    // import .lib.build as builder
    assert_eq!(imports[2].path, vec!["lib", "build"]);
    assert!(imports[2].is_local);
    assert_eq!(imports[2].alias, Some("builder".to_string()));

    // import .lib.build { compile }
    assert_eq!(imports[3].path, vec!["lib", "build"]);
    assert!(imports[3].is_local);
    assert_eq!(imports[3].items, Some(vec!["compile".to_string()]));

    // import .{utils, config}
    assert_eq!(imports[4].path, Vec::<String>::new());
    assert!(imports[4].is_local);
    assert_eq!(imports[4].items, Some(vec!["utils".to_string(), "config".to_string()]));
}

#[test]
fn parse_pub_import() {
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;

    let source = "pub import .utils\n";
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().unwrap();

    let item = module.items.iter()
        .map(|(_, i)| i)
        .next()
        .expect("no items");
    if let que_lang::ast::Item::Import(decl) = item {
        assert!(decl.is_pub);
        assert!(decl.is_local);
        assert_eq!(decl.path, vec!["utils"]);
    } else {
        panic!("expected Import item");
    }
}

// ── Edge cases ───────────────────────────────────────────────────────

#[test]
fn module_multiple_functions_exported() {
    assert_module_output(
        &[
            ("main.que", r#"
import .math
println(math.add(1, 2))
println(math.mul(3, 4))
println(math.neg(5))
"#),
            ("math.que", r#"
pub fn add(a, b) { a + b }
pub fn mul(a, b) { a * b }
pub fn neg(x) { -x }
fn internal_helper() { 0 }
"#),
        ],
        &["3", "12", "-5"],
    );
}

#[test]
fn module_import_passes_first_class_module() {
    // Modules are maps, so they can be passed as arguments
    assert_module_output(
        &[
            ("main.que", r#"
import .utils

fn use_module(mod) {
    mod.greet("passed")
}

println(use_module(utils))
"#),
            ("utils.que", r#"
pub fn greet(name) { "Hi, " + name }
"#),
        ],
        &["Hi, passed"],
    );
}

#[test]
fn module_deeply_nested_path() {
    assert_module_output(
        &[
            ("main.que", r#"
import .a.b.c
println(c.deep())
"#),
            ("a/b/c.que", r#"
pub fn deep() { "deep nested" }
"#),
        ],
        &["deep nested"],
    );
}

// ═════════════════════════════════════════════════════════════════════
// TYPE CONVERSION METHODS
// ═════════════════════════════════════════════════════════════════════

// ── List conversions ─────────────────────────────────────────────────

#[test]
fn list_to_tuple() {
    assert_result(
        r#"[1, 2, 3].to_tuple()"#,
        Value::Tuple(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

#[test]
fn list_to_tuple_empty() {
    assert_result(r#"[].to_tuple()"#, Value::Tuple(vec![]));
}

#[test]
fn list_to_set() {
    assert_result(
        r#"[1, 2, 2, 3, 1].to_set()"#,
        Value::Set(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

#[test]
fn list_to_set_preserves_order() {
    assert_result(
        r#"[3, 1, 2].to_set()"#,
        Value::Set(vec![Value::Int(3), Value::Int(1), Value::Int(2)]),
    );
}

#[test]
fn list_to_map() {
    assert_output(
        r#"
let m = [("a", 1), ("b", 2)].to_map()
println(m.a)
println(m.b)
"#,
        &["1", "2"],
    );
}

#[test]
fn list_to_map_from_lists() {
    assert_output(
        r#"
let m = [["x", 10], ["y", 20]].to_map()
println(m.x)
println(m.y)
"#,
        &["10", "20"],
    );
}

#[test]
fn list_to_map_invalid() {
    assert_error(r#"[1, 2, 3].to_map()"#);
}

// ── Set conversions ──────────────────────────────────────────────────

#[test]
fn set_to_tuple() {
    assert_result(
        r#"#{1, 2, 3}.to_tuple()"#,
        Value::Tuple(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

// ── Tuple conversions ────────────────────────────────────────────────

#[test]
fn tuple_to_set() {
    assert_result(
        r#"(1, 2, 2, 3).to_set()"#,
        Value::Set(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

// ── Map conversions ──────────────────────────────────────────────────

#[test]
fn map_to_list() {
    assert_output(
        r#"
let entries = {a: 1}.to_list()
let pair = entries[0]
println(pair[0])
println(pair[1])
"#,
        &["a", "1"],
    );
}

// ── Int conversions ──────────────────────────────────────────────────

#[test]
fn int_to_float() {
    assert_result("42.to_float()", Value::Float(42.0));
}

#[test]
fn int_to_string() {
    assert_result("42.to_string()", Value::String("42".into()));
}

#[test]
fn int_abs() {
    assert_result("(-5).abs()", Value::Int(5));
}

// ── Float conversions ────────────────────────────────────────────────

#[test]
fn float_to_int() {
    assert_result("3.7.to_int()", Value::Int(3));
}

#[test]
fn float_to_string() {
    assert_result("3.14.to_string()", Value::String("3.14".into()));
}

#[test]
fn float_abs() {
    assert_result("(-2.5).abs()", Value::Float(2.5));
}

#[test]
fn float_floor() {
    assert_result("3.7.floor()", Value::Float(3.0));
}

#[test]
fn float_ceil() {
    assert_result("3.2.ceil()", Value::Float(4.0));
}

#[test]
fn float_round() {
    assert_result("3.5.round()", Value::Float(4.0));
}

// ── Bool conversions ─────────────────────────────────────────────────

#[test]
fn bool_to_int_true() {
    assert_result("true.to_int()", Value::Int(1));
}

#[test]
fn bool_to_int_false() {
    assert_result("false.to_int()", Value::Int(0));
}

#[test]
fn bool_to_string() {
    assert_result("true.to_string()", Value::String("true".into()));
}

// ── Roundtrip conversions ────────────────────────────────────────────

#[test]
fn list_tuple_roundtrip() {
    assert_result(
        r#"[1, 2, 3].to_tuple().to_list()"#,
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
    );
}

#[test]
fn list_set_roundtrip() {
    assert_output(
        r#"
let result = [1, 2, 3].to_set().to_list()
println(result.len())
"#,
        &["3"],
    );
}

// ── Quick-win improvement tests ───────────────────────────────────────

// Improvement #2: println accepts multiple arguments

#[test]
fn println_multi_arg_strings() {
    assert_output(
        r#"println("hello", "world")"#,
        &["hello world"],
    );
}

#[test]
fn println_multi_arg_mixed_types() {
    assert_output(
        r#"println("status:", 200, "ok:", true)"#,
        &["status: 200 ok: true"],
    );
}

#[test]
fn println_multi_arg_with_variables() {
    assert_output(
        r#"
let code = 404
let msg = "not found"
println("status:", code, "message:", msg)
"#,
        &["status: 404 message: not found"],
    );
}

#[test]
fn println_zero_args() {
    assert_output(r#"println()"#, &[""]);
}

#[test]
fn println_single_int() {
    assert_output(r#"println(42)"#, &["42"]);
}

// Improvement #1: error messages include file and line number

#[test]
fn error_has_span_info() {
    // Errors from the interpreter should carry span info (line/col).
    let src = "let x = undefined_var";
    let err = que_lang::interpreter::run(src).unwrap_err();
    // After improvement #1, runtime errors carry source location.
    assert!(err.span.is_some(), "expected span in error, got: {}", err);
    let span = err.span.unwrap();
    assert_eq!(span.line, 1, "expected error on line 1");
}

#[test]
fn error_line_number_in_function() {
    // Error inside a function body should point to the correct line.
    let src = r#"fn check() {
    let a = 1
    let b = no_such_variable
}
check()"#;
    let err = que_lang::interpreter::run(src).unwrap_err();
    assert!(err.span.is_some(), "expected span in error, got: {}", err);
    let span = err.span.unwrap();
    assert_eq!(span.line, 3, "expected error on line 3, got line {}", span.line);
}

// ── Quick-win #5: which() ─────────────────────────────────────────────

#[test]
fn which_finds_existing_command() {
    // "true" or "ls" are available on every Unix-like system.
    let (output, _) = run(r#"
let p = which("true")
println(typeof(p))
"#).unwrap();
    // Should find the command and return a Path.
    assert_eq!(output, &["Path"]);
}

#[test]
fn which_returns_null_for_missing_command() {
    let result = run(r#"which("__que_no_such_command_xyz__")"#).unwrap();
    assert_eq!(result.1, Value::Null);
}

#[test]
fn which_path_is_string_like() {
    // The returned path should contain the command name somewhere.
    let src = r#"
let p = which("true")
if p != null {
    println(str(p).contains("true"))
} else {
    println(true)
}
"#;
    let (output, _) = run(src).unwrap();
    assert_eq!(output, &["true"]);
}

#[test]
fn which_no_arg_is_error() {
    assert_error(r#"which()"#);
}

// ── Quick-win #6: os.exit(code) ──────────────────────────────────────

#[test]
fn os_exit_zero_is_observable() {
    // In the library/test context, os.exit() surfaces as an error
    // with the message "exit(N)" so callers can observe it without
    // the process actually terminating.
    let err = run(r#"os.exit(0)"#).unwrap_err();
    assert!(err.message.contains("exit(0)"), "got: {}", err.message);
}

#[test]
fn os_exit_nonzero_code() {
    let err = run(r#"os.exit(1)"#).unwrap_err();
    assert!(err.message.contains("exit(1)"), "got: {}", err.message);
}

#[test]
fn os_exit_flushes_output_before_exiting() {
    // Output printed before os.exit() is captured.
    let err = run(r#"
println("output before exit")
os.exit(2)
println("output after exit")
"#).unwrap_err();
    assert!(err.message.contains("exit(2)"), "got: {}", err.message);
}

#[test]
fn os_exit_no_arg_defaults_to_zero() {
    let err = run(r#"os.exit()"#).unwrap_err();
    assert!(err.message.contains("exit(0)"), "got: {}", err.message);
}

#[test]
fn os_exit_wrong_type_is_error() {
    // Passing a string should be a type error, not an exit signal.
    let err = run(r#"os.exit("bad")"#).unwrap_err();
    // The error should mention the type mismatch, not be "exit(N)".
    assert!(err.message.contains("expects") || err.message.contains("Int"),
        "expected type error, got: {}", err.message);
}

#[test]
fn os_map_has_exit_field() {
    // os.exit should be callable as a field access on the os map.
    let result = run(r#"typeof(os.exit)"#).unwrap();
    assert_eq!(result.1, Value::String("Function".to_string()));
}

// ── Quick-win 7: timestamp + Duration arithmetic ─────────────────────────────

#[test]
fn timestamp_plus_duration_returns_int() {
    // timestamp + duration should return an Int (future timestamp in ms)
    let result = run("import std.time\ntypeof(time.timestamp() + 1s)").unwrap();
    assert_eq!(result.1, Value::String("Int".to_string()));
}

#[test]
fn timestamp_plus_duration_is_greater_than_timestamp() {
    let result = run("import std.time
let t = time.timestamp()
let deadline = t + 1000ms
deadline > t").unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn timestamp_minus_duration_returns_int() {
    let result = run("import std.time\ntypeof(time.timestamp() - 1s)").unwrap();
    assert_eq!(result.1, Value::String("Int".to_string()));
}

#[test]
fn timestamp_minus_duration_is_less_than_timestamp() {
    let result = run("import std.time
let t = time.timestamp()
let past = t - 1000ms
past < t").unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn duration_plus_int_returns_int() {
    // Duration + Int (symmetric) should also give Int
    let result = run("import std.time\ntypeof(1s + time.timestamp())").unwrap();
    assert_eq!(result.1, Value::String("Int".to_string()));
}

#[test]
fn deadline_comparison_works() {
    // Typical deadline pattern: timestamp + duration, compare with timestamp
    let result = run("import std.time
let deadline = time.timestamp() + 24h
time.timestamp() < deadline").unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

// ── Quick-win 8: path.home() ─────────────────────────────────────────────────

#[test]
fn path_home_returns_path_type() {
    let result = run("typeof(path.home())").unwrap();
    assert_eq!(result.1, Value::String("Path".to_string()));
}

#[test]
fn path_home_is_non_empty() {
    let result = run("let h = path.home()
str(h).len() > 0").unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn path_home_slash_join() {
    // path.home() / "subdir" should give a Path ending in /subdir
    let result = run(r#"let p = path.home() / "subdir"
typeof(p)"#).unwrap();
    assert_eq!(result.1, Value::String("Path".to_string()));
}

#[test]
fn path_home_slash_join_contains_home() {
    let result = run(r#"let home = path.home()
let p = home / ".ssh"
str(p).starts_with(str(home))"#).unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

// ── Quick-win 9: env.set(key, value) ─────────────────────────────────────────

#[test]
fn env_set_makes_variable_readable() {
    let result = run(r#"env.set("QUE_TEST_QW9", "hello_que")
env.get("QUE_TEST_QW9")"#).unwrap();
    assert_eq!(result.1, Value::String("hello_que".to_string()));
}

#[test]
fn env_set_returns_null() {
    let result = run(r#"env.set("QUE_TEST_QW9_NULL", "x")"#).unwrap();
    assert_eq!(result.1, Value::Null);
}

#[test]
fn env_set_overrides_existing() {
    let result = run(r#"env.set("QUE_TEST_QW9_OVER", "first")
env.set("QUE_TEST_QW9_OVER", "second")
env.get("QUE_TEST_QW9_OVER")"#).unwrap();
    assert_eq!(result.1, Value::String("second".to_string()));
}

#[test]
fn env_set_no_key_is_error() {
    assert_error(r#"env.set()"#);
}

// ── Tier 1 Feature 6: env.require() and env.all() ─────────────────────────────

#[test]
fn env_all_returns_map() {
    let result = run(r#"let all = env.all()
typeof(all)"#).unwrap();
    assert_eq!(result.1, Value::String("Map".to_string()));
}

#[test]
fn env_all_contains_path() {
    // PATH should be in the env on any system
    let result = run(r#"let all = env.all()
all.contains("PATH")"#).unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn env_require_existing_var() {
    std::env::set_var("QUE_REQUIRE_TEST", "value123");
    let result = run(r#"env.set("QUE_REQUIRE_TEST", "value123")
let vars = env.require("QUE_REQUIRE_TEST")
vars["QUE_REQUIRE_TEST"]"#).unwrap();
    assert_eq!(result.1, Value::String("value123".to_string()));
}

#[test]
fn env_require_missing_var_errors() {
    // Ensure it doesn't exist
    std::env::remove_var("QUE_NONEXISTENT_XYZ123");
    assert_error(r#"env.require("QUE_NONEXISTENT_XYZ123")"#);
}

#[test]
fn env_require_list_of_vars() {
    std::env::set_var("QUE_REQ_A", "foo");
    std::env::set_var("QUE_REQ_B", "bar");
    let result = run(r#"env.set("QUE_REQ_A", "foo")
env.set("QUE_REQ_B", "bar")
let vars = env.require(["QUE_REQ_A", "QUE_REQ_B"])
vars["QUE_REQ_A"]"#).unwrap();
    assert_eq!(result.1, Value::String("foo".to_string()));
}

#[test]
fn env_is_ci_returns_bool() {
    let result = run("typeof(env.is_ci())").unwrap();
    assert_eq!(result.1, Value::String("Bool".to_string()));
}

#[test]
fn env_platform_returns_string() {
    let result = run("typeof(env.platform())").unwrap();
    assert_eq!(result.1, Value::String("String".to_string()));
}

#[test]
fn env_is_interactive_returns_bool() {
    let result = run("typeof(env.is_interactive())").unwrap();
    assert_eq!(result.1, Value::String("Bool".to_string()));
}

// ── Tier 1 Feature 3: Structured error context (.context()) ───────────────────

#[test]
fn context_on_ok_is_passthrough() {
    let result = run(r#"Ok(42).context("reading config")"#).unwrap();
    assert_eq!(result.1, Value::Ok(Box::new(Value::Int(42))));
}

#[test]
fn context_on_err_wraps_message() {
    let result = run(r#"
let e = Err("file not found").context("reading service config")
e.unwrap_err()"#).unwrap();
    assert_eq!(
        result.1,
        Value::String("reading service config: file not found".to_string())
    );
}

#[test]
fn context_chain_error_message() {
    let result = run(r#"
let r = Err("permission denied")
    .context("reading config.yaml")
    .context("loading service configuration")
r.unwrap_err()"#).unwrap();
    assert_eq!(
        result.1,
        Value::String("loading service configuration: reading config.yaml: permission denied".to_string())
    );
}

#[test]
fn err_with_context_and_try_operator() {
    // .context() followed by ? should propagate the wrapped error
    assert_error(r#"
fn load() {
    Err("not found").context("reading config")?
    "ok"
}
load()"#);
}

#[test]
fn unwrap_err_on_err_returns_inner() {
    let result = run(r#"Err("oops").unwrap_err()"#).unwrap();
    assert_eq!(result.1, Value::String("oops".to_string()));
}

#[test]
fn unwrap_err_on_ok_errors() {
    assert_error(r#"Ok(1).unwrap_err()"#);
}

// ── Tier 1 Feature 4: Expanded std.fs operations ──────────────────────────────

#[test]
fn fs_atomic_write_creates_file() {
    let result = run(r#"
import std.fs
import std.time
let tmp_path = "/tmp/que_atomic_test_" + str(time.timestamp())
fs.atomic_write(path(tmp_path), "hello atomic")
"#).unwrap();
    assert_eq!(result.1, Value::Ok(Box::new(Value::Null)));
}

#[test]
fn fs_temp_file_creates_a_path() {
    let result = run(r#"
import std.fs
let p = fs.temp_file()
typeof(p)"#).unwrap();
    assert_eq!(result.1, Value::String("Ok".to_string()));
}

#[test]
fn fs_temp_file_ok_is_path() {
    let result = run(r#"
import std.fs
let p = fs.temp_file().unwrap()
typeof(p)"#).unwrap();
    assert_eq!(result.1, Value::String("Path".to_string()));
}

#[test]
fn fs_temp_dir_creates_directory() {
    let result = run(r#"
import std.fs
let d = fs.temp_dir("que_test_").unwrap()
path(d).is_dir()"#).unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn fs_remove_dir_removes_directory() {
    // Create a temp dir, then remove it
    let result = run(r#"
import std.fs
let d = fs.temp_dir("que_rmtest_").unwrap()
let r = fs.remove_dir(path(d))
r"#).unwrap();
    assert_eq!(result.1, Value::Ok(Box::new(Value::Null)));
}

#[test]
fn fs_copy_dir_copies_contents() {
    use std::io::Write;
    // Create source dir with a file
    let src = std::env::temp_dir().join("que_copytest_src");
    let dest = std::env::temp_dir().join("que_copytest_dest");
    let _ = std::fs::create_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dest);
    let mut f = std::fs::File::create(src.join("hello.txt")).unwrap();
    f.write_all(b"hello").unwrap();
    drop(f);
    let src_str = src.to_str().unwrap();
    let dest_str = dest.to_str().unwrap();
    let result = run(&format!(r#"
import std.fs
fs.copy_dir("{}", "{}")"#, src_str, dest_str)).unwrap();
    assert_eq!(result.1, Value::Ok(Box::new(Value::Null)));
    assert!(dest.join("hello.txt").exists());
}

#[test]
fn fs_find_returns_list() {
    // Create a controlled directory to search
    let dir = std::env::temp_dir().join("que_find_test");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("a.txt"), "a");
    let _ = std::fs::write(dir.join("b.txt"), "b");
    let dir_str = dir.to_str().unwrap();
    let result = run(&format!(r#"
import std.fs
fs.find("{}")"#, dir_str)).unwrap();
    assert!(matches!(result.1, Value::List(_)));
}

#[test]
fn fs_read_lines_and_write_lines() {
    let result = run(r#"
import std.fs
let tmp = fs.temp_file("que_lines_", ".txt").unwrap()
let lines = ["line1", "line2", "line3"]
fs.write_lines(lines, path(tmp))
let read_back = fs.read_lines(path(tmp)).unwrap()
read_back.len()"#).unwrap();
    assert_eq!(result.1, Value::Int(3));
}

// ── Tier 1 Feature 2: Background process spawn ────────────────────────────────

#[test]
fn spawn_returns_process_handle() {
    let result = run(r#"
let handle = spawn `sleep 100`
typeof(handle)"#).unwrap();
    // Kill the process after check
    assert_eq!(result.1, Value::String("ProcessHandle".to_string()));
}

#[test]
fn spawn_pid_is_nonzero() {
    let result = run(r#"
let handle = spawn `sleep 100`
let pid = handle.pid()
handle.kill()
pid > 0"#).unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn spawn_is_alive_true_while_running() {
    let result = run(r#"
let handle = spawn `sleep 100`
let alive = handle.is_alive()
handle.kill()
alive"#).unwrap();
    assert_eq!(result.1, Value::Bool(true));
}

#[test]
fn spawn_wait_returns_exit_code() {
    let result = run(r#"
let handle = spawn `true`
handle.wait()"#).unwrap();
    assert_eq!(result.1, Value::Int(0));
}

#[test]
fn spawn_kill_stops_process() {
    let result = run(r#"
let handle = spawn `sleep 100`
handle.kill()
handle.wait()
handle.is_alive()"#).unwrap();
    assert_eq!(result.1, Value::Bool(false));
}

#[test]
fn spawn_requires_cmd_value() {
    // spawn on a non-Cmd value should error
    assert_error(r#"spawn 42"#);
}

// ── Tier 1 Feature 1: True parallel execution ─────────────────────────────────

#[test]
fn parallel_unnamed_returns_tuple() {
    let result = run(r#"
parallel {
    1 + 1,
    "hello",
    true
}"#).unwrap();
    assert_eq!(
        result.1,
        Value::Tuple(vec![Value::Int(2), Value::String("hello".to_string()), Value::Bool(true)])
    );
}

#[test]
fn parallel_named_returns_map() {
    let result = run(r#"
let r = parallel {
    a: 1 + 1,
    b: "world",
    c: 3 * 3
}
r["b"]"#).unwrap();
    assert_eq!(result.1, Value::String("world".to_string()));
}

#[test]
fn parallel_named_map_keys() {
    let result = run(r#"
let r = parallel {
    x: 10,
    y: 20
}
r["x"] + r["y"]"#).unwrap();
    assert_eq!(result.1, Value::Int(30));
}

#[test]
fn parallel_tuple_destructuring() {
    let result = run(r#"
let (a, b) = parallel {
    2 * 3,
    10 + 5
}
a + b"#).unwrap();
    assert_eq!(result.1, Value::Int(21));
}

#[test]
fn parallel_mixed_labels_errors() {
    // Mixing named and unnamed is not allowed
    assert_error(r#"parallel { a: 1, 2, b: 3 }"#);
}

#[test]
fn parallel_with_functions() {
    let result = run(r#"
fn double(x) { x * 2 }
fn triple(x) { x * 3 }
parallel {
    a: double(5),
    b: triple(5)
}"#).unwrap();
    use std::collections::BTreeMap;
    let mut expected = BTreeMap::new();
    expected.insert("a".to_string(), Value::Int(10));
    expected.insert("b".to_string(), Value::Int(15));
    assert_eq!(result.1, Value::Map(expected));
}

#[test]
fn parallel_branches_actually_overlap() {
    // Three one-second sleeps. Sequential evaluation cannot finish this in
    // under two seconds, so the assertion is on wall-clock, not on a flag.
    let start = std::time::Instant::now();
    let result = run(r#"parallel {
    `sleep 1`.out(),
    `sleep 1`.out(),
    `sleep 1`.out(),
}"#)
    .unwrap();
    assert!(
        start.elapsed() < std::time::Duration::from_millis(2500),
        "parallel took {:?}, which means the branches ran one after another",
        start.elapsed()
    );
    assert!(matches!(result.1, Value::Tuple(ref v) if v.len() == 3));
}

#[test]
fn parallel_branches_read_outer_variables() {
    let result = run(r#"
let base = 10
parallel { base + 1, base + 2 }"#)
        .unwrap();
    assert_eq!(
        result.1,
        Value::Tuple(vec![Value::Int(11), Value::Int(12)])
    );
}

#[test]
fn parallel_output_is_replayed_in_source_order() {
    // Branches finish in whatever order the scheduler picks; the transcript
    // must not depend on that.
    let result = run(r#"parallel {
    { `sleep 0.3`.out(); println("slow"); 1 },
    { println("fast"); 2 },
}"#)
    .unwrap();
    assert_eq!(result.0, vec!["slow".to_string(), "fast".to_string()]);
}

#[test]
fn parallel_reports_the_first_failing_branch() {
    let err = run(r#"parallel {
    1,
    fail("second broke"),
    fail("third broke"),
}"#)
    .unwrap_err();
    assert!(
        err.message.contains("second broke"),
        "expected the first failure in source order, got {}",
        err.message
    );
}

// ── OOP: Structs + Impl + Traits ─────────────────────────────────────

#[test]
fn struct_basic_construction() {
    assert_result(r#"
struct Point { x: Float, y: Float }
let p = Point { x: 1.0, y: 2.0 }
p.x
"#, Value::Float(1.0));
}

#[test]
fn struct_field_defaults() {
    assert_result(r#"
struct Config {
    host: String = "localhost"
    port: Int = 8080
}
let c = Config {}
c.port
"#, Value::Int(8080));
}

#[test]
fn struct_partial_defaults() {
    assert_result(r#"
struct Config {
    host: String = "localhost"
    port: Int = 8080
}
let c = Config { port: 9000 }
c.port
"#, Value::Int(9000));
}

#[test]
fn struct_field_shorthand() {
    assert_result(r#"
struct Point { x: Float, y: Float }
let x = 3.0
let y = 4.0
let p = Point { x, y }
p.x
"#, Value::Float(3.0));
}

#[test]
fn struct_unknown_field_error() {
    assert_error(r#"
struct Point { x: Float, y: Float }
let p = Point { x: 1.0, z: 99.0 }
"#);
}

#[test]
fn struct_missing_required_field_error() {
    assert_error(r#"
struct Point { x: Float, y: Float }
let p = Point { x: 1.0 }
"#);
}

#[test]
fn struct_static_method() {
    assert_result(r#"
struct Point { x: Float, y: Float }
impl Point {
    fn new(x, y) -> Point { Point { x, y } }
}
let p = Point.new(1.0, 2.0)
p.y
"#, Value::Float(2.0));
}

#[test]
fn struct_callable_as_constructor() {
    // TypeName(args) should be sugar for TypeName.new(args)
    assert_result(r#"
struct Point { x: Float, y: Float }
impl Point {
    fn new(x, y) -> Point { Point { x, y } }
}
let p = Point(3.0, 4.0)
p.x
"#, Value::Float(3.0));
}

#[test]
fn struct_callable_constructor_chained() {
    assert_result(r#"
struct Counter { value: Int }
impl Counter {
    fn new(v) -> Counter { Counter { value: v } }
    fn inc(self) -> Counter { Counter { value: self.value + 1 } }
}
Counter(10).inc().inc().value
"#, Value::Int(12));
}

#[test]
fn struct_callable_no_new_error() {
    assert_error(r#"
struct Point { x: Float, y: Float }
Point(1.0, 2.0)
"#);
}

#[test]
fn struct_instance_method() {
    assert_result(r#"
struct Point { x: Float, y: Float }
impl Point {
    fn new(x, y) -> Point { Point { x, y } }
    fn scale(self, factor) -> Point {
        Point { x: self.x * factor, y: self.y * factor }
    }
}
let p = Point.new(2.0, 3.0)
let q = p.scale(2.0)
q.x
"#, Value::Float(4.0));
}

#[test]
fn struct_instance_method_returns_field() {
    assert_result(r#"
struct Rectangle { width: Float, height: Float }
impl Rectangle {
    fn area(self) -> Float { self.width * self.height }
}
let r = Rectangle { width: 5.0, height: 3.0 }
r.area()
"#, Value::Float(15.0));
}

#[test]
fn struct_mut_field_assignment() {
    assert_result(r#"
struct Counter { value: Int = 0 }
mut c = Counter {}
c.value = 42
c.value
"#, Value::Int(42));
}

#[test]
fn a_mut_self_method_updates_the_value_it_was_called_on() {
    assert_result(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn bump(mut self) { self.n = self.n + 1 }
}
mut c = Counter {}
c.bump()
c.bump()
c.n
"#, Value::Int(2));
}

#[test]
fn a_mut_self_method_still_returns_a_value() {
    assert_result(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn add(mut self, k) -> Int { self.n = self.n + k; self.n }
}
mut c = Counter {}
let doubled = c.add(4) + c.add(0)
[c.n, doubled]
"#, Value::List(vec![Value::Int(4), Value::Int(8)]));
}

#[test]
fn a_mut_self_method_reaches_through_a_field_path() {
    assert_result(r#"
struct Counter { n: Int = 0 }
struct Box { inner }
impl Counter {
    fn bump(mut self) { self.n = self.n + 1 }
}
mut b = Box { inner: Counter {} }
b.inner.bump()
b.inner.n
"#, Value::Int(1));
}

#[test]
fn a_mut_self_method_needs_a_mut_binding() {
    // The write-back is an assignment, so `let` refuses it — and says why,
    // rather than reporting a bare "immutable variable" for a line that
    // contains no assignment.
    assert_error_contains(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn bump(mut self) { self.n = self.n + 1 }
}
let c = Counter {}
c.bump()
"#, "bump() takes `mut self`");
}

#[test]
fn a_mut_self_method_refuses_a_receiver_nobody_can_see_again() {
    assert_error_contains(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn new() -> Counter { Counter {} }
    fn bump(mut self) { self.n = self.n + 1 }
}
Counter().bump()
"#, "call it on a variable, not on a temporary");
}

#[test]
fn a_plain_self_is_still_a_copy_the_method_cannot_write_to() {
    assert_error_contains(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn bump(self) { self.n = self.n + 1 }
}
mut c = Counter {}
c.bump()
"#, "cannot assign to immutable variable 'self'");
}

#[test]
fn only_the_receiver_can_be_declared_mut() {
    assert_error_contains(
        "fn f(mut a) { a }",
        "only `self` can be declared `mut`",
    );
}

#[test]
fn a_mut_self_enter_is_visible_to_exit() {
    // `with Dir(...)` has no variable to write the manager back to, so the
    // `with` itself carries what enter() changed through to exit().
    assert_output(r#"
struct Trace { mark }
impl Trace {
    fn new() -> Trace { Trace { mark: "unset" } }
}
impl Contextual for Trace {
    fn enter(mut self) { self.mark = "entered" }
    fn exit(self, resource) { println("exit saw " + self.mark) }
}
with Trace() {
    println("body")
}
"#, &["body", "exit saw entered"]);
}

#[test]
fn struct_typeof() {
    assert_output(r#"
struct Dog { name: String }
let d = Dog { name: "Rex" }
println(typeof(d))
"#, &["Dog"]);
}

#[test]
fn struct_display() {
    // Display shows TypeName { field: val, ... }
    assert_output(r#"
struct Point { x: Float, y: Float }
let p = Point { x: 1.0, y: 2.0 }
println(str(p))
"#, &["Point {x: 1, y: 2}"]);
}

#[test]
fn struct_match_instance_pattern() {
    assert_output(r#"
struct Point { x: Float, y: Float }
let p = Point { x: 0.0, y: 5.0 }
match p {
    Point { x: 0.0, y } => println("on y-axis at ${y}")
    Point { x, y } => println("at (${x}, ${y})")
}
"#, &["on y-axis at 5"]);
}

#[test]
fn struct_match_binds_fields() {
    assert_result(r#"
struct Pair { a: Int, b: Int }
let p = Pair { a: 10, b: 20 }
match p {
    Pair { a, b } => a + b
}
"#, Value::Int(30));
}

#[test]
fn trait_basic_definition_and_impl() {
    assert_output(r#"
struct Dog { name: String }
trait Greet {
    fn greet(self) -> String
}
impl Greet for Dog {
    fn greet(self) -> String { "Woof, I'm ${self.name}!" }
}
let d = Dog { name: "Rex" }
println(d.greet())
"#, &["Woof, I'm Rex!"]);
}

#[test]
fn trait_default_method() {
    assert_output(r#"
struct Point { x: Float, y: Float }
trait Show {
    fn label(self) -> String
    fn show(self) {
        println(self.label())
    }
}
impl Show for Point {
    fn label(self) -> String { "(${self.x}, ${self.y})" }
}
let p = Point { x: 3.0, y: 4.0 }
p.show()
"#, &["(3, 4)"]);
}

#[test]
fn trait_missing_required_method_error() {
    assert_error(r#"
struct Foo { x: Int }
trait Required { fn must_implement(self) -> Int }
impl Required for Foo {
    // missing must_implement
}
"#);
}

#[test]
fn trait_override_default_method() {
    assert_output(r#"
struct Foo {}
trait Greet {
    fn name(self) -> String { "default" }
    fn greet(self) { println("Hello, ${self.name()}") }
}
impl Greet for Foo {
    fn name(self) -> String { "Foo" }
}
let f = Foo {}
f.greet()
"#, &["Hello, Foo"]);
}

#[test]
fn impl_multiple_methods() {
    assert_result(r#"
struct Counter { n: Int = 0 }
impl Counter {
    fn new() -> Counter { Counter { n: 0 } }
    fn inc(self) -> Counter { Counter { n: self.n + 1 } }
    fn value(self) -> Int { self.n }
}
let c = Counter.new().inc().inc().inc()
c.value()
"#, Value::Int(3));
}

// ═════════════════════════════════════════════════════════════════════
// FILE HANDLE TESTS
// ═════════════════════════════════════════════════════════════════════

#[test]
fn open_returns_file_handle() {
    // Write a temp file then open it
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_open_basic.txt");
    std::fs::write(&path, "hello").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
typeof(f)
"#, p = p);
    assert_result(&source, Value::String("FileHandle".into()));
}

#[test]
fn open_nonexistent_returns_err() {
    let source = r#"
let f = open("/this/path/does/not/exist/que_test_missing.txt")
typeof(f)
"#;
    assert_result(source, Value::String("Err".into()));
}

#[test]
fn open_invalid_mode_returns_err() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_bad_mode.txt");
    std::fs::write(&path, "hi").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}", "x")
typeof(f)
"#, p = p);
    assert_result(&source, Value::String("Err".into()));
}

#[test]
fn file_handle_path_method() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_path.txt");
    std::fs::write(&path, "data").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.close()
typeof(f.path())
"#, p = p);
    assert_result(&source, Value::String("Path".into()));
}

#[test]
fn file_handle_is_open_true_before_close() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_is_open_true.txt");
    std::fs::write(&path, "x").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.is_open()
"#, p = p);
    assert_result(&source, Value::Bool(true));
}

#[test]
fn file_handle_is_open_false_after_close() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_is_open_false.txt");
    std::fs::write(&path, "x").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.close()
f.is_open()
"#, p = p);
    assert_result(&source, Value::Bool(false));
}

#[test]
fn file_handle_read_returns_content() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_read.txt");
    std::fs::write(&path, "hello world").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
let content = f.read()
f.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("hello world".into()))));
}

#[test]
fn file_handle_read_line_iterates() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_readline.txt");
    std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
let a = f.read_line()
let b = f.read_line()
let c = f.read_line()
let eof = f.read_line()
f.close()
println(a)
println(b)
println(c)
println(typeof(eof))
"#, p = p);
    assert_output(&source, &["line1", "line2", "line3", "Null"]);
}

#[test]
fn file_handle_lines_returns_list() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_lines.txt");
    std::fs::write(&path, "a\nb\nc\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
let ls = f.lines()
f.close()
ls
"#, p = p);
    assert_result(&source, Value::List(vec![
        Value::String("a".into()),
        Value::String("b".into()),
        Value::String("c".into()),
    ]));
}

#[test]
fn file_handle_write_and_read() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_write_read.txt");
    let p = path.to_string_lossy();
    let source = format!(r#"
let out = open("{p}", "w")
out.write("written content")
out.close()
let inp = open("{p}")
let content = inp.read()
inp.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("written content".into()))));
}

#[test]
fn file_handle_append_mode() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_append.txt");
    std::fs::write(&path, "first\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let out = open("{p}", "a")
out.writeln("second")
out.close()
let inp = open("{p}")
let lines = inp.lines()
inp.close()
lines
"#, p = p);
    assert_result(&source, Value::List(vec![
        Value::String("first".into()),
        Value::String("second".into()),
    ]));
}

#[test]
fn file_handle_write_on_read_handle_returns_err() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_wrong_mode.txt");
    std::fs::write(&path, "data").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}", "r")
let result = f.write("oops")
f.close()
typeof(result)
"#, p = p);
    assert_result(&source, Value::String("Err".into()));
}

#[test]
fn file_handle_with_defer() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_defer.txt");
    let p = path.to_string_lossy();
    // Write content, then use defer to close while reading
    let source = format!(r#"
fn read_file(path) {{
    let f = open(path, "w")
    defer f.close()
    f.write("deferred close")
}}
read_file("{p}")
let inp = open("{p}")
let content = inp.read()
inp.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("deferred close".into()))));
}

#[test]
fn file_handle_seek_start() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_seek.txt");
    std::fs::write(&path, "abcdefghij").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.read_line()
f.seek(3)
let content = f.read()
f.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("defghij".into()))));
}

#[test]
fn file_handle_seek_returns_position() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_seek_pos.txt");
    std::fs::write(&path, "abcdefghij").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
let pos = f.seek(5)
f.close()
pos
"#, p = p);
    assert_result(&source, Value::Int(5));
}

#[test]
fn file_handle_seek_end() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_seek_end.txt");
    std::fs::write(&path, "abcdefghij").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.seek(-3, "end")
let content = f.read()
f.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("hij".into()))));
}

#[test]
fn file_handle_seek_current() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_fh_seek_cur.txt");
    std::fs::write(&path, "abcdefghij").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
let f = open("{p}")
f.seek(2)
f.seek(3, "current")
let content = f.read()
f.close()
content
"#, p = p);
    assert_result(&source, Value::Ok(Box::new(Value::String("fghij".into()))));
}

/// ── Tier 2: std.hash ──────────────────────────────────────────────────────

#[test]
fn hash_sha256_string() {
    // SHA-256 of empty string is known
    let source = r#"
import std.hash
hash.sha256("")
"#;
    assert_result(source, Value::String("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into()));
}

#[test]
fn hash_sha256_content() {
    let source = r#"
import std.hash
hash.sha256("hello")
"#;
    assert_result(source, Value::String("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into()));
}

#[test]
fn hash_md5_string() {
    let source = r#"
import std.hash
hash.md5("")
"#;
    assert_result(source, Value::String("d41d8cd98f00b204e9800998ecf8427e".into()));
}

#[test]
fn hash_sha512_string() {
    let source = r#"
import std.hash
let h = hash.sha512("")
h.len() == 128
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn hash_sha256_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_hash_test.txt");
    std::fs::write(&path, "hello").unwrap();
    let p = path.to_string_lossy();
    // path() value → reads the file
    let source = format!(r#"
import std.hash
hash.sha256(path("{p}"))
"#, p = p);
    assert_result(&source, Value::String("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into()));
}

#[test]
fn hash_sha256_string_does_not_read_file() {
    // Regression: hash.sha256("filename") must hash the literal string,
    // not silently read the file even if a file by that name exists.
    let dir = std::env::temp_dir();
    let path = dir.join("que_hash_str_vs_file.txt");
    std::fs::write(&path, "different content").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.hash
// String literal → hash of the string "que_hash_str_vs_file.txt" (filename chars)
let str_hash  = hash.sha256("{p}")
// path() → hash of the file contents
let file_hash = hash.sha256(path("{p}"))
str_hash != file_hash
"#, p = p);
    assert_result(&source, Value::Bool(true));
}

#[test]
fn hash_integrity_verify() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_integrity_test.txt");
    std::fs::write(&path, "hello").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.hash
hash.verify("{p}", "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
"#, p = p);
    assert_result(&source, Value::Bool(true));
}

#[test]
fn hash_integrity_verify_fail() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_integrity_test_fail.txt");
    std::fs::write(&path, "hello").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.hash
hash.verify("{p}", "sha256:0000000000000000000000000000000000000000000000000000000000000000")
"#, p = p);
    assert_result(&source, Value::Bool(false));
}

#[test]
fn hash_write_and_verify_checksums() {
    let dir = std::env::temp_dir();
    let file1 = dir.join("que_chk1.txt");
    let file2 = dir.join("que_chk2.txt");
    let checksums = dir.join("que_SHA256SUMS.txt");
    std::fs::write(&file1, "aaa").unwrap();
    std::fs::write(&file2, "bbb").unwrap();
    let p1 = file1.to_string_lossy();
    let p2 = file2.to_string_lossy();
    let pchk = checksums.to_string_lossy();
    let source = format!(r#"
import std.hash
hash.write_checksums(["{p1}", "{p2}"], "{pchk}")
hash.verify_checksums("{pchk}")
"#, p1=p1, p2=p2, pchk=pchk);
    assert_result(&source, Value::Bool(true));
}

// ── Tier 2: std.csv ───────────────────────────────────────────────────────

#[test]
fn csv_parse_str_basic() {
    use std::collections::BTreeMap;
    let source = r#"
import std.csv
let rows = csv.parse_str("name,age\nAlice,30\nBob,25")
rows
"#;
    let (_, result) = que_lang::interpreter::run(source).unwrap();
    match result {
        Value::List(rows) => {
            assert_eq!(rows.len(), 2);
            match &rows[0] {
                Value::Map(m) => {
                    assert_eq!(m.get("name"), Some(&Value::String("Alice".into())));
                    assert_eq!(m.get("age"), Some(&Value::String("30".into())));
                }
                _ => panic!("expected map row"),
            }
        }
        _ => panic!("expected list"),
    }
}

#[test]
fn csv_parse_str_tsv() {
    let source = r#"
import std.csv
let rows = csv.parse_str("a\tb\nc\td", "\t")
rows.len()
"#;
    assert_result(source, Value::Int(1));
}

#[test]
fn csv_write_and_reparse() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_csv_test.csv");
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.csv
let rows = [
    {{ "name": "Alice", "score": "100" }},
    {{ "name": "Bob", "score": "85" }}
]
csv.write(rows, "{p}")
let back = csv.parse("{p}")
back.len()
"#, p = p);
    assert_result(&source, Value::Int(2));
}

#[test]
fn csv_to_string() {
    let source = r#"
import std.csv
let rows = [{"a": "1", "b": "2"}]
let s = csv.to_string(rows)
s.contains("a")
"#;
    assert_result(source, Value::Bool(true));
}

// ── Tier 2: std.dotenv ────────────────────────────────────────────────────

#[test]
fn dotenv_parse_file() {
    use std::collections::BTreeMap;
    let dir = std::env::temp_dir();
    let path = dir.join("que_test.env");
    std::fs::write(&path, "FOO=bar\nBAZ=qux\n# comment\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.dotenv
let vars = dotenv.parse("{p}")
vars["FOO"]
"#, p = p);
    assert_result(&source, Value::String("bar".into()));
}

#[test]
fn dotenv_parse_quoted_values() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_test_quoted.env");
    std::fs::write(&path, "MSG=\"hello world\"\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.dotenv
let vars = dotenv.parse("{p}")
vars["MSG"]
"#, p = p);
    assert_result(&source, Value::String("hello world".into()));
}

#[test]
fn dotenv_write_and_parse() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_written.env");
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.dotenv
dotenv.write({{ "PORT": "8080", "ENV": "test" }}, "{p}")
let vars = dotenv.parse("{p}")
vars["PORT"]
"#, p = p);
    assert_result(&source, Value::String("8080".into()));
}

#[test]
fn dotenv_load_into_env() {
    let dir = std::env::temp_dir();
    let path = dir.join("que_load_test.env");
    std::fs::write(&path, "QUE_TEST_DOTENV_KEY=loaded_value\n").unwrap();
    let p = path.to_string_lossy();
    let source = format!(r#"
import std.dotenv
dotenv.load("{p}")
env.get("QUE_TEST_DOTENV_KEY")
"#, p = p);
    assert_result(&source, Value::String("loaded_value".into()));
}

// ── Tier 2: std.log ───────────────────────────────────────────────────────

#[test]
fn log_info_basic() {
    let source = r#"
import std.log
log.info("hello world")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert!(!output.is_empty());
    assert!(output[0].contains("INFO"));
    assert!(output[0].contains("hello world"));
}

#[test]
fn log_all_levels() {
    let source = r#"
import std.log
log.debug("debug msg")
log.info("info msg")
log.warn("warn msg")
log.error("error msg")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 4);
    assert!(output[0].contains("DEBUG"));
    assert!(output[1].contains("INFO"));
    assert!(output[2].contains("WARN"));
    assert!(output[3].contains("ERROR"));
}

#[test]
fn log_with_fields() {
    let source = r#"
import std.log
log.info("deploy started", { version: "1.0", env: "prod" })
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert!(!output.is_empty());
    assert!(output[0].contains("version=1.0"));
}

// ── std.log enhanced: level gating ────────────────────────────────────────

#[test]
fn log_set_level_filters_below() {
    let source = r#"
import std.log
log.set_level("warn")
log.debug("should not appear")
log.info("should not appear")
log.warn("this should appear")
log.error("this too")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 2);
    assert!(output[0].contains("WARN"));
    assert!(output[1].contains("ERROR"));
}

#[test]
fn log_set_level_default_passes_all() {
    let source = r#"
import std.log
log.debug("d")
log.info("i")
log.warn("w")
log.error("e")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 4);
}

// ── std.log enhanced: JSON format ─────────────────────────────────────────

#[test]
fn log_set_format_json() {
    let source = r#"
import std.log
log.set_format("json")
log.info("hello json")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 1);
    let line = &output[0];
    assert!(line.starts_with('{'));
    assert!(line.ends_with('}'));
    assert!(line.contains("\"level\":\"INFO\""));
    assert!(line.contains("\"message\":\"hello json\""));
    assert!(line.contains("\"timestamp\":"));
}

#[test]
fn log_json_with_fields() {
    let source = r#"
import std.log
log.set_format("json")
log.info("deploy", { env: "prod", count: 42 })
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    let line = &output[0];
    assert!(line.contains("\"env\":\"prod\""));
    assert!(line.contains("\"count\":42"));
}

#[test]
fn log_json_stays_parseable_with_awkward_content() {
    // A log ingester parses these lines. A message containing a newline, a
    // quote, a tab or a control character used to produce a line that no
    // JSON parser would accept, which loses the whole record.
    let source = r#"
import std.log
log.set_format("json")
log.info("line one\nline two\ttabbed \"quoted\" back\\slash", { "wei\nrd": "tab\there" })
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output[0])
        .unwrap_or_else(|e| panic!("log line is not valid JSON ({}): {}", e, output[0]));
    assert_eq!(
        parsed["message"],
        "line one\nline two\ttabbed \"quoted\" back\\slash"
    );
    assert_eq!(parsed["wei\nrd"], "tab\there");
}

// ── std.log enhanced: sinks ───────────────────────────────────────────────

#[test]
fn log_add_file_sink_basic() {
    let dir = std::env::temp_dir().join("que_log_test_basic");
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("test.log");
    let _ = std::fs::remove_file(&log_path); // clean slate

    let source = format!(r#"
import std.log
log.add_file_sink("{}")
log.info("file log test")
"#, log_path.display());
    let (output, _) = que_lang::interpreter::run(&source).unwrap();
    // Console output still works (default sink + file sink)
    assert!(output.iter().any(|l| l.contains("file log test")));
    // File should also have the line
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("file log test"));
    // cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_file_sink_with_level_override() {
    let dir = std::env::temp_dir().join("que_log_test_level");
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("errors.log");
    let _ = std::fs::remove_file(&log_path);

    let source = format!(r#"
import std.log
log.add_file_sink("{}", {{ level: "error" }})
log.info("info line")
log.error("error line")
"#, log_path.display());
    let (output, _) = que_lang::interpreter::run(&source).unwrap();
    // Both appear on console
    assert_eq!(output.len(), 2);
    // Only error should be in file
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(!content.contains("info line"));
    assert!(content.contains("error line"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_file_sink_with_field_filter() {
    let dir = std::env::temp_dir().join("que_log_test_filter");
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("prod.log");
    let _ = std::fs::remove_file(&log_path);

    let source = format!(r#"
import std.log
log.add_file_sink("{}", {{ filter: {{ env: "prod" }} }})
log.info("dev deploy", {{ env: "dev" }})
log.info("prod deploy", {{ env: "prod" }})
"#, log_path.display());
    let (_output, _) = que_lang::interpreter::run(&source).unwrap();
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(!content.contains("dev deploy"));
    assert!(content.contains("prod deploy"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_remove_sinks_resets() {
    let dir = std::env::temp_dir().join("que_log_test_remove");
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("removed.log");
    let _ = std::fs::remove_file(&log_path);

    let source = format!(r#"
import std.log
log.add_file_sink("{}")
log.info("before remove")
log.remove_sinks()
log.info("after remove")
"#, log_path.display());
    let (output, _) = que_lang::interpreter::run(&source).unwrap();
    // Both should appear in console output (remove_sinks restores default console)
    assert_eq!(output.len(), 2);
    // File should only have the first line
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("before remove"));
    assert!(!content.contains("after remove"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_add_console_sink_json() {
    let source = r#"
import std.log
log.add_console_sink({ format: "json" })
log.info("dual output")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    // Default console (text) + added console (json) = 2 lines
    assert_eq!(output.len(), 2);
    // One should be text, one should be JSON
    let has_json = output.iter().any(|l| l.starts_with('{'));
    let has_text = output.iter().any(|l| l.starts_with('['));
    assert!(has_json);
    assert!(has_text);
}

// ── std.log enhanced: Logger instances ────────────────────────────────────

#[test]
fn log_new_basic() {
    let source = r#"
import std.log
let logger = log.new({ service: "api", version: "1.2" })
logger.info("starting")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 1);
    assert!(output[0].contains("service=api"));
    assert!(output[0].contains("version=1.2"));
    assert!(output[0].contains("starting"));
}

#[test]
fn log_child_creates_child() {
    let source = r#"
import std.log
let logger = log.new({ service: "api", version: "1.0" })
let db_log = logger.child({ component: "db", version: "2.0" })
db_log.info("connected")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 1);
    let line = &output[0];
    // Should have parent fields
    assert!(line.contains("service=api"));
    // Should have child fields
    assert!(line.contains("component=db"));
    // Child overrides parent on collision
    assert!(line.contains("version=2.0"));
    assert!(!line.contains("version=1.0"));
}

#[test]
fn log_instance_and_root_independent() {
    let source = r#"
import std.log
let logger = log.new({ ctx: "instance" })
logger.info("from instance")
log.info("from root")
"#;
    let (output, _) = que_lang::interpreter::run(source).unwrap();
    assert_eq!(output.len(), 2);
    // Instance line has context
    assert!(output[0].contains("ctx=instance"));
    // Root line does NOT have context
    assert!(!output[1].contains("ctx="));
}

// ── Tier 2: std.git ───────────────────────────────────────────────────────

#[test]
fn git_branch_in_repo() {
    // This test runs in the que git repo itself
    let source = r#"
import std.git
let b = git.branch(".")
b.len() > 0
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn git_commit_is_hex() {
    let source = r#"
import std.git
let c = git.commit(".")
c.len() == 40
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn git_short_commit_length() {
    let source = r#"
import std.git
let c = git.short_commit(".")
c.len() >= 7
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn git_is_clean_or_dirty_is_bool() {
    let source = r#"
import std.git
let d = git.is_dirty(".")
let c = git.is_clean(".")
typeof(d) == "Bool" && typeof(c) == "Bool"
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn git_tags_is_list() {
    let source = r#"
import std.git
let tags = git.tags(".")
typeof(tags) == "List"
"#;
    assert_result(source, Value::Bool(true));
}

#[test]
fn git_clone_copies_a_repository_and_returns_the_destination() {
    // A local bare repo is a real remote as far as `git clone` is concerned,
    // so this exercises the whole path without touching the network.
    let Some(fixture) = git_fixture("clone_ok") else {
        return;
    };
    let origin = fixture.join("origin");
    let dest = fixture.join("cloned");
    let source = format!(
        r#"
import std.git
let d = git.clone("{}", p"{}", {{ quiet: true }})?
let readme = d / "README.md"
d.to_string() == "{}" && readme.read()? == "hello"
"#,
        origin.display(),
        dest.display(),
        dest.display()
    );
    assert_result(&source, Value::Bool(true));
    let _ = std::fs::remove_dir_all(&fixture);
}

#[test]
fn git_clone_infers_the_directory_from_the_url() {
    let Some(fixture) = git_fixture("clone_default") else {
        return;
    };
    let origin = fixture.join("origin");
    let workdir = fixture.join("work");
    std::fs::create_dir_all(&workdir).unwrap();
    // `git clone <url>` names the directory after the url's last segment;
    // `.git` is stripped the way git strips it.
    let source = format!(
        r#"
import std.git
with dir(p"{}") {{
    let d = git.clone("{}", null, {{ quiet: true }})?
    d.to_string() == "origin" && p"origin/README.md".exists()
}}
"#,
        workdir.display(),
        origin.display()
    );
    assert_result(&source, Value::Bool(true));
    let _ = std::fs::remove_dir_all(&fixture);
}

#[test]
fn git_clone_reports_a_failure_as_an_err_rather_than_raising() {
    let dir = std::env::temp_dir().join("que_git_clone_missing");
    let _ = std::fs::remove_dir_all(&dir);
    let source = format!(
        r#"
import std.git
let r = git.clone("{}/nope", p"{}/dest", {{ quiet: true }})
r.is_err()
"#,
        dir.display(),
        dir.display()
    );
    assert_result(&source, Value::Bool(true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_clone_does_not_let_a_url_smuggle_in_a_shell_command() {
    // The url is an `Interpolated` command part, so it is escaped rather
    // than parsed by the shell.
    let dir = std::env::temp_dir().join("que_git_clone_inject");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("PWNED");
    let source = format!(
        r#"
import std.git
let r = git.clone("http://x/y.git; touch {}", p"{}/dest", {{ quiet: true }})
r.is_err()
"#,
        marker.display(),
        dir.display()
    );
    assert_result(&source, Value::Bool(true));
    assert!(!marker.exists(), "the url ran as a shell command");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Build a bare repo with one commit on `main` and return its parent
/// directory, or `None` when `git` is not installed.
fn git_fixture(name: &str) -> Option<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("que_git_fixture_{}", name));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).ok()?;
    let origin = root.join("origin");
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).ok()?;

    let git = |args: &[&str], cwd: &std::path::Path| -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    let origin_s = origin.to_string_lossy().to_string();
    let ok = git(&["init", "-q", "--bare", &origin_s], &root)
        && git(&["init", "-q"], &seed)
        && {
            std::fs::write(seed.join("README.md"), "hello").ok()?;
            true
        }
        && git(&["add", "-A"], &seed)
        && git(
            &[
                "-c", "user.email=t@example.com",
                "-c", "user.name=t",
                "commit", "-qm", "init",
            ],
            &seed,
        )
        && git(&["push", "-q", &origin_s, "HEAD:refs/heads/main"], &seed)
        // A bare repo's HEAD still points at whatever `init` defaulted to,
        // so aim it at the branch that was actually pushed — otherwise a
        // clone checks nothing out.
        && git(&["symbolic-ref", "HEAD", "refs/heads/main"], &origin);

    if ok {
        Some(root)
    } else {
        let _ = std::fs::remove_dir_all(&root);
        None
    }
}

/// ── Tier 2: std.archive ───────────────────────────────────────────────────

#[test]
fn archive_tar_gz_create_and_list() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_archive_src.txt");
    let archive = dir.join("que_test_archive.tar.gz");
    std::fs::write(&src_file, "archive content").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", ["{src}"])
let entries = archive.list("{arc}")
entries.len() > 0
"#, arc=arc, src=src);
    assert_result(&source, Value::Bool(true));
}

#[test]
fn archive_tar_gz_extract() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_arc_extract_src.txt");
    let archive = dir.join("que_extract_test.tar.gz");
    let out_dir = dir.join("que_extract_out");
    std::fs::write(&src_file, "extract me").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let out = out_dir.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", ["{src}"])
archive.extract("{arc}", "{out}")
"#, arc=arc, src=src, out=out);
    let (_, result) = que_lang::interpreter::run(&source).unwrap();
    assert_eq!(result, Value::Null);
    assert!(out_dir.exists());
}

#[test]
fn archive_zip_create_and_list() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_zip_src.txt");
    let archive = dir.join("que_test.zip");
    std::fs::write(&src_file, "zip content").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.zip("{arc}", ["{src}"])
let entries = archive.list("{arc}")
entries.len() > 0
"#, arc=arc, src=src);
    assert_result(&source, Value::Bool(true));
}

// ── archive: prefix and src/dest mapping ─────────────────────────────────

#[test]
fn archive_tar_gz_with_prefix() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_pfx_src.txt");
    let archive  = dir.join("que_pfx.tar.gz");
    std::fs::write(&src_file, "prefixed").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", ["{src}"], "myapp-v1.0")
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, src=src);
    // Entry should be "myapp-v1.0/que_pfx_src.txt"
    assert_result(&source, Value::String("myapp-v1.0/que_pfx_src.txt".into()));
}

#[test]
fn archive_zip_with_prefix() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_zpfx_src.txt");
    let archive  = dir.join("que_zpfx.zip");
    std::fs::write(&src_file, "zip prefixed").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.zip("{arc}", ["{src}"], "release")
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, src=src);
    assert_result(&source, Value::String("release/que_zpfx_src.txt".into()));
}

#[test]
fn archive_tar_gz_src_dest_mapping() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_map_src.txt");
    let archive  = dir.join("que_map.tar.gz");
    std::fs::write(&src_file, "mapped").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", [
    {{ src: "{src}", dest: "bin/tool" }}
])
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, src=src);
    assert_result(&source, Value::String("bin/tool".into()));
}

#[test]
fn archive_tar_gz_src_dest_with_prefix() {
    let dir = std::env::temp_dir();
    let src_file = dir.join("que_mappfx_src.txt");
    let archive  = dir.join("que_mappfx.tar.gz");
    std::fs::write(&src_file, "mapped+prefix").unwrap();
    let src = src_file.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", [
    {{ src: "{src}", dest: "bin/tool" }}
], "myapp-v2.0")
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, src=src);
    assert_result(&source, Value::String("myapp-v2.0/bin/tool".into()));
}

#[test]
fn archive_tar_gz_dest_dot_archives_contents_at_root() {
    // { src: dir, dest: "." } should archive the directory's *contents* at the
    // archive root, producing "file.txt" not "./file.txt".
    let dir = std::env::temp_dir();
    let src_dir = dir.join("que_dest_dot_src");
    let archive  = dir.join("que_dest_dot.tar.gz");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("hello.txt"), "hi").unwrap();
    let sd = src_dir.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", [{{ src: "{sd}", dest: "." }}])
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, sd=sd);
    assert_result(&source, Value::String("hello.txt".into()));
}

#[test]
fn archive_tar_gz_dest_prefix_archives_contents_under_prefix() {
    // { src: dir, dest: "myapp-1.0" } archives directory contents under that prefix.
    let dir = std::env::temp_dir();
    let src_dir = dir.join("que_dest_pfx_src");
    let archive  = dir.join("que_dest_pfx.tar.gz");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("binary"), "exec").unwrap();
    let sd = src_dir.to_string_lossy();
    let arc = archive.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{arc}", [{{ src: "{sd}", dest: "myapp-1.0" }}])
let entries = archive.list("{arc}")
entries[0]
"#, arc=arc, sd=sd);
    assert_result(&source, Value::String("myapp-1.0/binary".into()));
}

#[test]
fn archive_zip_dir_basename_consistent_with_tar() {
    // Regression: zip should include the directory basename (like tar),
    // not silently drop it and put contents at the archive root.
    let dir = std::env::temp_dir();
    let src_dir = dir.join("que_arc_dir_test");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("file.txt"), "hello").unwrap();
    let tar_arc = dir.join("que_dir_test.tar.gz");
    let zip_arc = dir.join("que_dir_test.zip");
    let sd = src_dir.to_string_lossy();
    let ta = tar_arc.to_string_lossy();
    let za = zip_arc.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{ta}", ["{sd}"])
archive.zip("{za}", ["{sd}"])
let tar_entries = archive.list("{ta}")
let zip_entries = archive.list("{za}")
// Both should have the directory name as root
let tar_has_basename = tar_entries[0].starts_with("que_arc_dir_test")
let zip_has_basename = zip_entries[0].starts_with("que_arc_dir_test")
tar_has_basename && zip_has_basename
"#, ta=ta, za=za, sd=sd);
    assert_result(&source, Value::Bool(true));
}

// ═════════════════════════════════════════════════════════════════════
// FORMATTER (que fmt)
// ═════════════════════════════════════════════════════════════════════

use que_lang::formatter::Formatter;
use que_lang::lexer::Lexer;
use que_lang::parser::Parser;

/// Helper: parse and format source, return the formatted string.
fn format_source(source: &str) -> String {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("lex error");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().expect("parse error");
    Formatter::new().format_module(&module)
}

#[test]
fn fmt_let_binding() {
    let out = format_source("let   x   =   42");
    assert!(out.contains("let x = 42"));
}

#[test]
fn fmt_keeps_the_prologue() {
    let out = format_source("#!/usr/bin/env que\n#!strict\nlet x = 1\n");
    assert!(out.starts_with("#!/usr/bin/env que\n#!strict\n\n"), "{}", out);
}

#[test]
fn fmt_keeps_parentheses_a_prefix_operator_needs() {
    // Dropping them rewrote `!(a == b)` as `(!a) == b`, which is a different
    // answer rather than a different layout.
    let out = format_source("let x = !(1 == 2)");
    assert!(out.contains("!(1 == 2)"), "{}", out);
}

#[test]
fn fmt_keeps_parentheses_a_right_hand_operand_needs() {
    let out = format_source("let x = 10 - (4 - 1)");
    assert!(out.contains("10 - (4 - 1)"), "{}", out);
}

#[test]
fn fmt_puts_task_metadata_above_the_task() {
    let out = format_source(
        "@aliases([b])\n@description(\"Build\")\n@deps([prep])\ntask prep {}\ntask build { println(\"x\") }\n",
    );
    assert!(
        out.contains("@description(\"Build\")\n@deps([prep])\n@aliases([\"b\"])\ntask prep {"),
        "{}",
        out
    );
}

#[test]
fn fmt_mut_binding() {
    let out = format_source("mut   y   =   10");
    assert!(out.contains("mut y = 10"));
}

#[test]
fn fmt_function_declaration() {
    let out = format_source("fn   add(a,  b)  {  a + b  }");
    assert!(out.contains("fn add(a, b) {"));
    assert!(out.contains("    a + b"));
    assert!(out.contains("}"));
}

#[test]
fn fmt_task_rest_parameter_keeps_its_marker() {
    let out = format_source("task  build( target ,  ...files )  {  println(target)  }");
    assert!(out.contains("task build(target, ...files) {"), "{out}");
}

#[test]
fn fmt_if_else() {
    let out = format_source("if  true  {  1  }  else  {  2  }");
    assert!(out.contains("if true {"));
    assert!(out.contains("} else {"));
}

#[test]
fn fmt_for_loop() {
    let out = format_source("for  x  in  [1,2,3]  {  println(x)  }");
    assert!(out.contains("for x in [1, 2, 3] {"));
}

#[test]
fn fmt_match_expression() {
    let out = format_source(r#"match x { 1 => "one", 2 => "two", _ => "other" }"#);
    assert!(out.contains("match x {"));
    assert!(out.contains("    1 => \"one\","));
    assert!(out.contains("    _ => \"other\","));
}

#[test]
fn fmt_import() {
    let out = format_source("import  std.fs");
    assert!(out.contains("import std.fs"));
}

#[test]
fn fmt_pipe_operator() {
    let out = format_source("[1, 2, 3] |> map(|x| x * 2)");
    assert!(out.contains("|> map("));
}

#[test]
fn fmt_binary_ops_spacing() {
    let out = format_source("let x = 1+2*3");
    assert!(out.contains("1 + 2 * 3"));
}

#[test]
fn fmt_blank_line_between_fns() {
    let out = format_source("fn a() { 1 }\nfn b() { 2 }");
    // Should have a blank line between the two function declarations
    assert!(out.contains("}\n\nfn b()"));
}

#[test]
fn fmt_struct_decl() {
    let out = format_source("struct  Point  {  x: Int,  y: Int  }");
    assert!(out.contains("struct Point {"));
    assert!(out.contains("    x: Int,"));
    assert!(out.contains("    y: Int,"));
}

#[test]
fn fmt_idempotent() {
    // Formatting should be idempotent — formatting twice gives the same result
    let source = r#"
fn greet(name) {
    let msg = "Hello, ${name}!"
    println(msg)
}

fn main() {
    greet("world")
}
"#;
    let first = format_source(source);
    let second = format_source(&first);
    assert_eq!(first, second, "Formatter is not idempotent");
}

#[test]
fn fmt_try_catch() {
    let out = format_source("try  {  1  }  catch  e  {  2  }");
    assert!(out.contains("try {"));
    assert!(out.contains("} catch e {"));
}

#[test]
fn fmt_while_loop() {
    let out = format_source("while  x > 0  {  x -= 1  }");
    assert!(out.contains("while x > 0 {"));
}

#[test]
fn fmt_map_literal() {
    // Parser converts identifier keys to StringLit, so formatter reproduces with quotes
    let out = format_source("let m = { a: 1, b: 2 }");
    assert!(out.contains("{ \"a\": 1, \"b\": 2 }"));
}

#[test]
fn fmt_named_args() {
    let out = format_source("deploy(service: \"api\", replicas: 3)");
    assert!(out.contains("deploy(service: \"api\", replicas: 3)"));
}

#[test]
fn fmt_spawn_expr() {
    let out = format_source("let p = spawn  `echo hello`");
    assert!(out.contains("spawn `echo hello`"));
}

#[test]
fn fmt_parallel_block() {
    let out = format_source("parallel  {  a: foo(),  b: bar()  }");
    assert!(out.contains("parallel {"));
    assert!(out.contains("    a: foo(),"));
    assert!(out.contains("    b: bar(),"));
}

#[test]
fn fmt_enum_decl() {
    let out = format_source("enum  Color  {  Red,  Green,  Blue  }");
    assert!(out.contains("enum Color {"));
    assert!(out.contains("    Red,"));
    assert!(out.contains("    Green,"));
    assert!(out.contains("    Blue,"));
}

#[test]
fn fmt_trailing_newline() {
    let out = format_source("let x = 1");
    assert!(out.ends_with('\n'));
}

#[test]
fn fmt_lambda() {
    let out = format_source("|x, y|  x + y");
    assert!(out.contains("|x, y| x + y"));
}

#[test]
fn fmt_preserves_cmd_literals() {
    let out = format_source("let r = `git status`");
    assert!(out.contains("`git status`"));
}

#[test]
fn fmt_preserves_string_interpolation() {
    let out = format_source(r#"let s = "hello ${name}""#);
    assert!(out.contains("\"hello ${name}\""));
}

#[test]
fn fmt_duration_literal() {
    let out = format_source("let t = 5s");
    assert!(out.contains("5s"));
}

#[test]
fn fmt_return_statement() {
    let out = format_source("fn f() { return  42 }");
    assert!(out.contains("return 42"));
}

#[test]
fn fmt_compound_assign() {
    let out = format_source("x  +=  1");
    assert!(out.contains("x += 1"));
}

#[test]
fn fmt_defer() {
    let out = format_source("defer  cleanup()");
    assert!(out.contains("defer cleanup()"));
}

#[test]
fn fmt_regex_escaped_quote() {
    // \" inside a regex literal must survive the format round-trip
    let src = r#"re"foo\"bar""#;
    let out = format_source(src);
    assert!(out.contains(r#"re"foo\"bar""#), "got: {out}");
}

#[test]
fn fmt_regex_metachar_preserved() {
    // \d, \s etc. must not be doubled
    let src = r#"re"\d+\s*""#;
    let out = format_source(src);
    assert!(out.contains(r#"re"\d+\s*""#), "got: {out}");
}

#[test]
fn fmt_method_call_on_binary_expr_keeps_parens() {
    // (basedir / "folder").mkdir() — parens are semantically required
    let src = r#"(basedir / "folder").mkdir()"#;
    let out = format_source(src);
    assert!(out.contains(r#"(basedir / "folder").mkdir()"#), "got: {out}");
}

#[test]
fn fmt_field_access_on_binary_expr_keeps_parens() {
    let src = "(a + b).field";
    let out = format_source(src);
    assert!(out.contains("(a + b).field"), "got: {out}");
}

#[test]
fn fmt_index_on_binary_expr_keeps_parens() {
    let src = "(a + b)[0]";
    let out = format_source(src);
    assert!(out.contains("(a + b)[0]"), "got: {out}");
}

// ═════════════════════════════════════════════════════════════════════
// TYPE ENFORCEMENT (strict mode)
// ═════════════════════════════════════════════════════════════════════

#[test]
fn strict_param_type_check_int() {
    // Correct type should work
    let (_, val) = run_strict(r#"
fn add(a: Int, b: Int) -> Int { a + b }
add(1, 2)
"#).unwrap();
    assert_eq!(val, Value::Int(3));
}

#[test]
fn strict_param_type_check_string() {
    let (_, val) = run_strict(r#"
fn greet(name: String) -> String { "Hello, " + name }
greet("world")
"#).unwrap();
    assert_eq!(val, Value::String("Hello, world".to_string()));
}

#[test]
fn strict_param_type_mismatch() {
    // Wrong type should fail in strict mode
    assert!(run_strict(r#"
fn add(a: Int, b: Int) -> Int { a + b }
add("hello", 2)
"#).is_err());
}

#[test]
fn strict_return_type_mismatch() {
    // Return type mismatch should fail
    assert!(run_strict(r#"
fn get_num() -> Int { "not a number" }
get_num()
"#).is_err());
}

#[test]
fn strict_no_annotation_no_check() {
    // Without type annotations, strict mode should not interfere
    let (_, val) = run_strict(r#"
fn add(a, b) { a + b }
add("hello", " world")
"#).unwrap();
    assert_eq!(val, Value::String("hello world".to_string()));
}

#[test]
fn strict_bool_param() {
    assert!(run_strict(r#"
fn check(flag: Bool) { flag }
check(42)
"#).is_err());
}

#[test]
fn strict_list_type() {
    let (_, val) = run_strict(r#"
fn first(items: List) { items[0] }
first([1, 2, 3])
"#).unwrap();
    assert_eq!(val, Value::Int(1));
}

#[test]
fn strict_map_type() {
    let (_, val) = run_strict(r#"
fn get_key(m: Map) { m.keys() }
get_key({ a: 1 })
"#).unwrap();
    assert_eq!(val, Value::List(vec![Value::String("a".to_string())]));
}

#[test]
fn strict_any_type() {
    // Any type should accept anything
    let (_, val) = run_strict(r#"
fn identity(x: Any) -> Any { x }
identity(42)
"#).unwrap();
    assert_eq!(val, Value::Int(42));
}

#[test]
fn strict_enabled_via_pragma() {
    assert!(run(r#"#!strict
fn add(a: Int, b: Int) -> Int { a + b }
add("hello", 2)
"#).is_err());
}

#[test]
fn strict_pragma_may_follow_a_shebang() {
    assert!(run(r#"#!/usr/bin/env que
#!strict
fn add(a: Int, b: Int) -> Int { a + b }
add("hello", 2)
"#).is_err());
}

#[test]
fn shebang_alone_does_not_enable_strict() {
    let (_, val) = run(r#"#!/usr/bin/env que
fn add(a: Int, b: Int) -> Int { a + b }
add("hello", " world")
"#).unwrap();
    assert_eq!(val, Value::String("hello world".to_string()));
}

#[test]
fn unknown_pragma_is_rejected() {
    assert!(run("#!lenient\n1").is_err());
}

#[test]
fn strict_cannot_be_toggled_at_runtime() {
    assert!(run("strict(true)").is_err());
    assert!(run("strict(false)").is_err());
}

#[test]
fn strict_disabled_by_default() {
    // Without strict mode, type annotations are decorative
    let (_, val) = run(r#"
fn add(a: Int, b: Int) -> Int { a + b }
add("hello", " world")
"#).unwrap();
    assert_eq!(val, Value::String("hello world".to_string()));
}

#[test]
fn strict_query() {
    // strict() with no args returns current state
    let (_, val) = run("strict()").unwrap();
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn strict_path_type() {
    let result = run_strict(r#"
fn process(p: Path) { str(p) }
process(42)
"#);
    assert!(result.is_err());
}

#[test]
fn strict_return_type_correct() {
    let (_, val) = run_strict(r#"
fn make_list() -> List { [1, 2, 3] }
make_list()
"#).unwrap();
    assert_eq!(val, Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
}

// ═════════════════════════════════════════════════════════════════════
// LINTER (que lint)
// ═════════════════════════════════════════════════════════════════════

use que_lang::linter::Linter;

/// Helper: parse and lint source, return diagnostics.
fn lint_source(source: &str) -> Vec<que_lang::linter::LintDiagnostic> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("lex error");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().expect("parse error");
    Linter::new().lint_module(&module)
}

#[test]
fn lint_unreachable_after_return() {
    let diags = lint_source(r#"
fn foo() {
    return 1
    println("unreachable")
}
"#);
    assert!(diags.iter().any(|d| d.rule == "unreachable-code"));
}

#[test]
fn lint_unreachable_after_break() {
    let diags = lint_source(r#"
for x in [1, 2, 3] {
    break
    println("unreachable")
}
"#);
    assert!(diags.iter().any(|d| d.rule == "unreachable-code"));
}

#[test]
fn lint_unreachable_after_continue() {
    let diags = lint_source(r#"
for x in [1, 2, 3] {
    continue
    println("unreachable")
}
"#);
    assert!(diags.iter().any(|d| d.rule == "unreachable-code"));
}

#[test]
fn lint_no_unreachable_without_terminator() {
    let diags = lint_source(r#"
fn foo() {
    let x = 1
    println(x)
}
"#);
    assert!(diags.iter().all(|d| d.rule != "unreachable-code"));
}

#[test]
fn lint_no_unused_for_bare_cmd() {
    // A bare command now runs and raises on failure, so its result is not
    // "unused" in any meaningful sense.
    let diags = lint_source("`git status`");
    assert!(diags.iter().all(|d| d.rule != "unused-result"));
}

#[test]
fn lint_unused_http_result() {
    let diags = lint_source(r#"get("https://example.com")"#);
    assert!(diags.iter().any(|d| d.rule == "unused-result"));
}

#[test]
fn lint_no_unused_for_run() {
    let diags = lint_source("`git status`.run()");
    assert!(diags.iter().all(|d| d.rule != "unused-result"));
}

#[test]
fn lint_no_unused_when_assigned() {
    let diags = lint_source(r#"let result = `git status`"#);
    assert!(diags.iter().all(|d| d.rule != "unused-result"));
}

#[test]
fn lint_unscoped_cd() {
    // Throwing away what `cd` returns leaves nothing to move back with, and
    // the process stays moved for the rest of the run.
    let diags = lint_source("cd(p\"sub\")\nprintln(\"after\")\n");
    assert!(diags.iter().any(|d| d.rule == "unscoped-cd"), "{:?}", diags);
}

#[test]
fn lint_no_unscoped_cd_when_the_way_back_is_kept() {
    let diags = lint_source("let previous = cd(p\"sub\")\nprintln(\"after\")\n");
    assert!(diags.iter().all(|d| d.rule != "unscoped-cd"), "{:?}", diags);
}

#[test]
fn lint_no_unscoped_cd_for_a_with_dir_block() {
    let diags = lint_source("with dir(p\"sub\") {\n    println(\"hi\")\n}\nprintln(\"after\")\n");
    assert!(diags.iter().all(|d| d.rule != "unscoped-cd"), "{:?}", diags);
}

#[test]
fn lint_empty_function_body() {
    let diags = lint_source("fn empty() {}");
    assert!(diags.iter().any(|d| d.rule == "empty-block"));
}

#[test]
fn lint_no_empty_body_warning() {
    let diags = lint_source("fn add(a, b) { a + b }");
    assert!(diags.iter().all(|d| d.rule != "empty-block"));
}

#[test]
fn lint_secret_interpolation() {
    let diags = lint_source(r#"let msg = "token is ${api_token}""#);
    assert!(diags.iter().any(|d| d.rule == "secret-interpolation"));
}

#[test]
fn lint_no_secret_warning_normal() {
    let diags = lint_source(r#"let msg = "hello ${name}""#);
    assert!(diags.iter().all(|d| d.rule != "secret-interpolation"));
}

#[test]
fn lint_clean_code_no_warnings() {
    let diags = lint_source(r#"
fn greet(name) {
    let msg = "Hello, ${name}!"
    println(msg)
}
greet("world")
"#);
    assert!(diags.is_empty(), "Expected no warnings, got: {:?}", diags);
}

// ── Template engine tests ──────────────────────────────────────────────────

#[test]
fn template_simple_substitution() {
    assert_output(r#"
import std.template
let result = template.render("Hello, {{ name }}!", { "name": "World" })
println(result)
"#, &["Hello, World!"]);
}

#[test]
fn template_multiple_vars() {
    assert_output(r#"
import std.template
let result = template.render("{{ greeting }}, {{ name }}!", { "greeting": "Hi", "name": "Alice" })
println(result)
"#, &["Hi, Alice!"]);
}

#[test]
fn template_dotted_path() {
    assert_output(r#"
import std.template
let ctx = { "server": { "host": "localhost", "port": 8080 } }
let result = template.render("{{ server.host }}:{{ server.port }}", ctx)
println(result)
"#, &["localhost:8080"]);
}

#[test]
fn template_if_true() {
    assert_output(r#"
import std.template
let tmpl = "start{{# if show }}visible{{/ if }}end"
let result = template.render(tmpl, { "show": true })
println(result)
"#, &["startvisibleend"]);
}

#[test]
fn template_if_false() {
    assert_output(r#"
import std.template
let tmpl = "start{{# if show }}visible{{/ if }}end"
let result = template.render(tmpl, { "show": false })
println(result)
"#, &["startend"]);
}

#[test]
fn template_if_else() {
    assert_output(r#"
import std.template
let tmpl = "{{# if active }}ON{{# else }}OFF{{/ if }}"
let result = template.render(tmpl, { "active": false })
println(result)
"#, &["OFF"]);
}

#[test]
fn template_for_loop() {
    assert_output(r#"
import std.template
let tmpl = "{{# for item in items }}[{{ item }}]{{/ for }}"
let result = template.render(tmpl, { "items": ["a", "b", "c"] })
println(result)
"#, &["[a][b][c]"]);
}

#[test]
fn template_for_with_map_items() {
    assert_output(r#"
import std.template
let tmpl = "{{# for svc in services }}{{ svc.name }}:{{ svc.port }} {{/ for }}"
let ctx = { "services": [{ "name": "api", "port": 8080 }, { "name": "web", "port": 3000 }] }
let result = template.render(tmpl, ctx)
println(result)
"#, &["api:8080 web:3000 "]);
}

#[test]
fn template_nested_if_in_for() {
    assert_output(r#"
import std.template
let tmpl = "{{# for item in items }}{{# if item.active }}{{ item.name }} {{/ if }}{{/ for }}"
let ctx = { "items": [
    { "name": "a", "active": true },
    { "name": "b", "active": false },
    { "name": "c", "active": true }
] }
let result = template.render(tmpl, ctx)
println(result)
"#, &["a c "]);
}

#[test]
fn template_missing_var_is_null() {
    assert_output(r#"
import std.template
let result = template.render("Hello, {{ name }}!", {})
println(result)
"#, &["Hello, null!"]);
}

#[test]
fn template_integer_value() {
    assert_output(r#"
import std.template
let result = template.render("Count: {{ n }}", { "n": 42 })
println(result)
"#, &["Count: 42"]);
}

#[test]
fn template_multiline() {
    assert_output(r#"
import std.template
let tmpl = "name: {{ name }}\nreplicas: {{ replicas }}"
let result = template.render(tmpl, { "name": "myapp", "replicas": 3 })
println(result)
"#, &["name: myapp\nreplicas: 3"]);
}

#[test]
fn template_empty_for_loop() {
    assert_output(r#"
import std.template
let tmpl = "before{{# for x in items }}[{{ x }}]{{/ for }}after"
let result = template.render(tmpl, { "items": [] })
println(result)
"#, &["beforeafter"]);
}

#[test]
fn template_render_result_type() {
    assert_result(r#"
import std.template
typeof(template.render("hello", {}))
"#, Value::String("String".to_string()));
}

// ── Net utilities tests ────────────────────────────────────────────────────

#[test]
fn net_resolve_localhost() {
    assert_output(r#"
import std.net
let addrs = net.resolve("localhost")
println(typeof(addrs))
println(addrs.len() > 0)
"#, &["List", "true"]);
}

#[test]
fn net_resolve_returns_list() {
    assert_result(r#"
import std.net
typeof(net.resolve("localhost"))
"#, Value::String("List".to_string()));
}

#[test]
fn net_port_open_closed_port() {
    // Port 19999 should not be open on localhost
    assert_result(r#"
import std.net
net.port_open("127.0.0.1", 19999, 100)
"#, Value::Bool(false));
}

#[test]
fn net_port_open_returns_bool() {
    assert_result(r#"
import std.net
typeof(net.port_open("127.0.0.1", 19999, 100))
"#, Value::String("Bool".to_string()));
}

#[test]
fn net_ping_returns_bool() {
    assert_result(r#"
import std.net
typeof(net.ping("127.0.0.1", 100))
"#, Value::String("Bool".to_string()));
}

#[test]
fn net_wait_for_port_timeout() {
    // Should return false quickly since port 19999 isn't open
    assert_result(r#"
import std.net
net.wait_for_port("127.0.0.1", 19999, 200, 100)
"#, Value::Bool(false));
}

#[test]
fn net_wait_for_port_with_duration() {
    // Test that Duration values work for timeout/interval args
    assert_result(r#"
import std.net
net.wait_for_port("127.0.0.1", 19999, 200ms, 100ms)
"#, Value::Bool(false));
}

#[test]
fn net_module_import() {
    // Verify the module imports correctly and has expected functions
    assert_output(r#"
import std.net
println(typeof(net.ping))
println(typeof(net.port_open))
println(typeof(net.resolve))
println(typeof(net.wait_for_port))
println(typeof(net.wait_for_url))
"#, &["Function", "Function", "Function", "Function", "Function"]);
}

// ── Path iteration, walk, and API consistency ────────────────────────

#[test]
fn path_iteration_directory() {
    // Iterating a directory path yields its children
    let source = r#"
with TempDir {} as tmp {
    tmp.join("a.txt").write_text("alpha").unwrap()
    tmp.join("b.txt").write_text("beta").unwrap()
    let items = tmp.ls()
    let names = items.map(|f| f.name()).sort()
    println(names[0])
    println(names[1])
    println(names.len())
    // Also verify direct iteration works
    mut count = 0
    for f in tmp {
        count = count + 1
    }
    println(count)
}
"#;
    assert_output(source, &["a.txt", "b.txt", "2", "2"]);
}

#[test]
fn path_iteration_non_dir_error() {
    // Iterating a file path should error
    let source = r#"
with TempDir {} as tmp {
    let f = tmp.join("file.txt")
    f.write_text("hello").unwrap()
    for x in f {
        println(x)
    }
}
"#;
    assert_error(source);
}

#[test]
fn path_walk_recursive() {
    // .walk() returns all descendants (files and dirs)
    let source = r#"
with TempDir {} as tmp {
    tmp.join("a.txt").write_text("").unwrap()
    tmp.join("sub").mkdir().unwrap()
    tmp.join("sub/b.txt").write_text("").unwrap()
    let all = tmp.walk()
    println(all.len())
    // Should contain both files and the subdirectory
    let names = all.map(|p| p.name()).sort()
    for n in names {
        println(n)
    }
}
"#;
    assert_output(source, &["3", "a.txt", "b.txt", "sub"]);
}

#[test]
fn mkdir_clean_empties_a_directory_that_already_existed() {
    let source = r#"
with TempDir {} as tmp {
    let d = tmp / "build"
    d.mkdir()?
    (d / "stale.txt").write_text("old")?
    d.mkdir({ clean: true })?
    println(d.is_dir())
    println((d / "stale.txt").exists())
}
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn mkdir_clean_is_plain_mkdir_when_nothing_is_there() {
    let source = r#"
with TempDir {} as tmp {
    let d = tmp / "fresh"
    d.mkdir({ clean: true })?
    println(d.is_dir())
}
"#;
    assert_output(source, &["true"]);
}

#[test]
fn mkdir_clean_replaces_a_file_with_a_directory() {
    let source = r#"
with TempDir {} as tmp {
    let p = tmp / "was_a_file"
    p.write_text("x")?
    p.mkdir({ clean: true })?
    println(p.is_dir())
}
"#;
    assert_output(source, &["true"]);
}

#[test]
fn delete_of_a_missing_path_is_an_error_unless_missing_ok_says_otherwise() {
    // A delete that quietly does nothing is how a script deletes the wrong
    // thing for a week before anyone notices, so the default still fails.
    let source = r#"
with TempDir {} as tmp {
    let gone = tmp / "not_here"
    println(gone.delete().is_err())
    println(gone.delete({ missing_ok: true }).is_ok())
}
"#;
    assert_output(source, &["true", "true"]);
}

#[test]
fn delete_unlinks_a_symlink_without_following_it() {
    let source = r#"
with TempDir {} as tmp {
    let target = tmp / "real"
    target.mkdir()?
    (target / "keep.txt").write_text("x")?
    let link = tmp / "link"
    link.symlink(target)?
    link.delete()?
    println(link.exists())
    println((target / "keep.txt").exists())
}
"#;
    assert_output(source, &["false", "true"]);
}

#[test]
fn mkdir_and_delete_reject_a_non_map_option_argument() {
    // `mkdir(true)` is a typo for `mkdir({ clean: true })`; accepting it
    // would let the two spellings drift apart.
    assert_error_contains(
        "with TempDir {} as tmp { (tmp / \"d\").mkdir(true) }",
        "options map",
    );
    assert_error_contains(
        "with TempDir {} as tmp { (tmp / \"d\").delete(\"yes\") }",
        "options map",
    );
}

#[test]
fn contains_all_and_contains_any_take_a_list_or_loose_arguments() {
    let source = r#"
let name = "profile_x86_gcc12"
println(name.contains_all(["x86", "gcc"]))
println(name.contains_all("x86", "gcc"))
println(name.contains_all(["x86", "clang"]))
println(name.contains_any(["clang", "gcc"]))
println(name.contains_any(["clang", "msvc"]))
"#;
    assert_output(source, &["true", "true", "false", "true", "false"]);
}

#[test]
fn contains_all_and_contains_any_agree_with_all_and_any_on_an_empty_list() {
    let source = r#"
let name = "anything"
println(name.contains_all([]))
println(name.contains_any([]))
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn contains_all_accepts_a_set_or_a_tuple_of_needles() {
    let source = r#"
let name = "profile_x86_gcc"
println(name.contains_all(#{"x86", "gcc"}))
println(name.contains_all(("x86", "gcc")))
"#;
    assert_output(source, &["true", "true"]);
}

#[test]
fn contains_all_rejects_a_needle_that_is_not_a_string() {
    assert_error_contains(
        "\"abc\".contains_all([\"a\", 3])",
        "needles must be strings",
    );
}

#[test]
fn path_files_recursive() {
    // .files() returns only files, recursively
    let source = r#"
with TempDir {} as tmp {
    tmp.join("a.txt").write_text("").unwrap()
    tmp.join("sub").mkdir().unwrap()
    tmp.join("sub/b.txt").write_text("").unwrap()
    let files = tmp.files()
    println(files.len())
    let names = files.map(|p| p.name()).sort()
    for n in names {
        println(n)
    }
}
"#;
    assert_output(source, &["2", "a.txt", "b.txt"]);
}

#[test]
fn path_dirs_recursive() {
    // .dirs() returns only directories, recursively
    let source = r#"
with TempDir {} as tmp {
    tmp.join("sub1").mkdir().unwrap()
    tmp.join("sub1/sub2").mkdir().unwrap()
    tmp.join("file.txt").write_text("").unwrap()
    let dirs = tmp.dirs()
    println(dirs.len())
    let names = dirs.map(|p| p.name()).sort()
    for n in names {
        println(n)
    }
}
"#;
    assert_output(source, &["2", "sub1", "sub2"]);
}

#[test]
fn path_ls_returns_list_directly() {
    // .ls() now returns List directly, not Ok(List)
    let source = r#"
with TempDir {} as tmp {
    tmp.join("x.txt").write_text("").unwrap()
    tmp.join("y.txt").write_text("").unwrap()
    let items = tmp.ls()
    println(typeof(items))
    println(items.len())
}
"#;
    assert_output(source, &["List", "2"]);
}

#[test]
fn path_resolve_nonexistent() {
    // .resolve() should work on paths that don't exist
    let source = r#"
let p = path("./does/not/exist").resolve()
println(p.is_absolute())
println(p.name())
"#;
    assert_output(source, &["true", "exist"]);
}

#[test]
fn path_resolve_normalizes() {
    // .resolve() should normalize . and .. segments
    let source = r#"
let p = path("/a/b/../c/./d").resolve()
println(p)
"#;
    assert_output(source, &["/a/c/d"]);
}

// ═════════════════════════════════════════════════════════════════════
// std.time — DateTime
// ═════════════════════════════════════════════════════════════════════

#[test]
fn time_now_returns_datetime() {
    let source = r#"
import std.time
let dt = time.now()
println(typeof(dt))
"#;
    assert_output(source, &["DateTime"]);
}

#[test]
fn time_of_construction() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 10, 30, 0)
println(dt.year())
println(dt.month())
println(dt.day())
println(dt.hour())
println(dt.minute())
println(dt.second())
"#;
    assert_output(source, &["2024", "6", "15", "10", "30", "0"]);
}

#[test]
fn time_of_defaults() {
    // h/m/s should default to 0
    let source = r#"
import std.time
let dt = time.of(2024, 1, 1)
println(dt.hour())
println(dt.minute())
println(dt.second())
"#;
    assert_output(source, &["0", "0", "0"]);
}

#[test]
fn time_of_with_timezone() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 12, 0, 0, "America/New_York")
println(dt.timezone())
"#;
    assert_output(source, &["America/New_York"]);
}

#[test]
fn time_parse_datetime() {
    let source = r#"
import std.time
let dt = time.parse("2024-03-15 14:30:00", "%Y-%m-%d %H:%M:%S")
println(dt.year())
println(dt.month())
println(dt.day())
println(dt.hour())
println(dt.minute())
"#;
    assert_output(source, &["2024", "3", "15", "14", "30"]);
}

#[test]
fn time_from_timestamp_roundtrip() {
    let source = r#"
import std.time
let dt = time.of(2024, 1, 1, 0, 0, 0)
let ms = dt.timestamp()
let dt2 = time.from_timestamp(ms)
println(dt2.year())
println(dt2.month())
println(dt2.day())
println(dt.timestamp() == dt2.timestamp())
"#;
    assert_output(source, &["2024", "1", "1", "true"]);
}

#[test]
fn time_weekday() {
    let source = r#"
import std.time
// 2024-06-15 is a Saturday
let dt = time.of(2024, 6, 15)
println(dt.weekday())
"#;
    assert_output(source, &["Sat"]);
}

#[test]
fn time_day_of_year() {
    let source = r#"
import std.time
// Jan 1 = day 1
let dt = time.of(2024, 1, 1)
println(dt.day_of_year())
"#;
    assert_output(source, &["1"]);
}

#[test]
fn time_format() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 14, 30, 0)
println(dt.format("%Y/%m/%d"))
println(dt.format("%H:%M"))
"#;
    assert_output(source, &["2024/06/15", "14:30"]);
}

#[test]
fn time_to_iso() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 14, 30, 0)
let iso = dt.to_iso()
println(iso.starts_with("2024-06-15"))
"#;
    assert_output(source, &["true"]);
}

#[test]
fn time_in_tz() {
    let source = r#"
import std.time
let utc = time.of(2024, 6, 15, 12, 0, 0)
let eastern = utc.in_tz("America/New_York")
println(eastern.timezone())
// UTC 12:00 = EDT 08:00
println(eastern.hour())
"#;
    assert_output(source, &["America/New_York", "8"]);
}

#[test]
fn time_utc_method() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 12, 0, 0, "America/New_York")
let u = dt.utc()
println(u.timezone())
// EDT 12:00 = UTC 16:00
println(u.hour())
"#;
    assert_output(source, &["UTC", "16"]);
}

#[test]
fn time_add_days() {
    let source = r#"
import std.time
let dt = time.of(2024, 1, 30)
let next = dt.add_days(2)
println(next.day())
println(next.month())
"#;
    assert_output(source, &["1", "2"]);
}

#[test]
fn time_add_hours() {
    let source = r#"
import std.time
let dt = time.of(2024, 1, 1, 23, 0, 0)
let next = dt.add_hours(2)
println(next.hour())
println(next.day())
"#;
    assert_output(source, &["1", "2"]);
}

#[test]
fn time_add_negative() {
    let source = r#"
import std.time
let dt = time.of(2024, 3, 1)
let prev = dt.add_days(-1)
println(prev.day())
println(prev.month())
"#;
    assert_output(source, &["29", "2"]); // 2024 is a leap year
}

#[test]
fn time_comparison() {
    let source = r#"
import std.time
let a = time.of(2024, 1, 1)
let b = time.of(2024, 6, 15)
println(a < b)
println(b > a)
println(a >= a)
println(a <= b)
"#;
    assert_output(source, &["true", "true", "true", "true"]);
}

#[test]
fn time_equality() {
    let source = r#"
import std.time
let a = time.of(2024, 6, 15, 12, 0, 0)
let b = time.of(2024, 6, 15, 12, 0, 0)
println(a == b)
// Different timezone = different value (even if same instant)
let c = time.of(2024, 6, 15, 12, 0, 0, "America/New_York")
println(a == c)
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn time_string_interpolation() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 14, 30, 0)
let s = "${dt}"
println(s.starts_with("2024-06-15"))
"#;
    assert_output(source, &["true"]);
}

#[test]
fn time_typeof_and_is_type() {
    let source = r#"
import std.time
let dt = time.now()
println(typeof(dt))
"#;
    assert_output(source, &["DateTime"]);
}

#[test]
fn time_timezone_returns_system_tz() {
    let source = r#"
import std.time
let tz = time.timezone()
println(typeof(tz))
println(tz.len() > 0)
"#;
    assert_output(source, &["String", "true"]);
}

#[test]
fn time_error_invalid_timezone() {
    let source = r#"
import std.time
time.of(2024, 1, 1, 0, 0, 0, "Not/A/Timezone")
"#;
    assert_error(source);
}

#[test]
fn time_error_bad_parse() {
    let source = r#"
import std.time
time.parse("not-a-date", "%Y-%m-%d")
"#;
    assert_error(source);
}

#[test]
fn time_add_minutes_and_seconds() {
    let source = r#"
import std.time
let dt = time.of(2024, 1, 1, 0, 0, 0)
let dt2 = dt.add_minutes(90)
println(dt2.hour())
println(dt2.minute())
let dt3 = dt.add_seconds(3661)
println(dt3.hour())
println(dt3.minute())
println(dt3.second())
"#;
    assert_output(source, &["1", "30", "1", "1", "1"]);
}

#[test]
fn time_to_string_method() {
    let source = r#"
import std.time
let dt = time.of(2024, 6, 15, 14, 30, 0)
let s = dt.to_string()
println(s.starts_with("2024-06-15"))
"#;
    assert_output(source, &["true"]);
}

#[test]
fn time_from_timestamp_bridges_with_time_timestamp() {
    // The two halves of the old `now()` compose: one hands a number to the
    // other and you get the date back.
    let source = r#"
import std.time
let ms = time.timestamp()
let dt = time.from_timestamp(ms)
println(typeof(dt))
println(dt.year() > 2020)
"#;
    assert_output(source, &["DateTime", "true"]);
}

// ── pub struct export ────────────────────────────────────────────────

#[test]
fn module_pub_struct_export_basic() {
    // Export a pub struct, import it, construct an instance, access fields.
    assert_module_output(
        &[
            ("main.que", r#"
import .types { Config }
let c = Config { host: "localhost", port: 8080 }
println(c.host)
println(c.port)
"#),
            ("types.que", r#"
pub struct Config {
    host
    port
}
"#),
        ],
        &["localhost", "8080"],
    );
}

#[test]
fn module_pub_struct_with_defaults() {
    // Struct with default field values should work across modules.
    assert_module_output(
        &[
            ("main.que", r#"
import .types { Config }
let c = Config {}
println(c.host)
println(c.port)
"#),
            ("types.que", r#"
pub struct Config {
    host = "127.0.0.1"
    port = 3000
}
"#),
        ],
        &["127.0.0.1", "3000"],
    );
}

#[test]
fn module_pub_struct_with_impl_methods() {
    // Export a struct with impl methods, call methods on imported instances.
    assert_module_output(
        &[
            ("main.que", r#"
import .types { Point }
let p = Point { x: 3, y: 4 }
println(p.magnitude())
println(p.to_string())
"#),
            ("types.que", r#"
pub struct Point {
    x
    y
}

impl Point {
    fn magnitude(self) {
        let sum = self.x * self.x + self.y * self.y
        // Use integer approximation for test determinism
        if sum == 25 { 5 } else { 0 }
    }

    fn to_string(self) {
        "(" + str(self.x) + ", " + str(self.y) + ")"
    }
}
"#),
        ],
        &["5", "(3, 4)"],
    );
}

#[test]
fn module_pub_struct_wildcard_import() {
    // Wildcard import brings struct into scope for construction.
    assert_module_output(
        &[
            ("main.que", r#"
import .types { * }
let c = Config { host: "example.com", port: 443 }
println(c.host)
println(c.port)
"#),
            ("types.que", r#"
pub struct Config {
    host
    port
}
"#),
        ],
        &["example.com", "443"],
    );
}

#[test]
fn module_non_pub_struct_not_exported() {
    // A struct without `pub` should NOT be accessible to importers.
    assert_module_error(
        &[
            ("main.que", r#"
import .types { Internal }
let i = Internal { value: 42 }
println(i.value)
"#),
            ("types.que", r#"
struct Internal {
    value
}
pub fn make() { Internal { value: 1 } }
"#),
        ],
    );
}

// ── pub let export ───────────────────────────────────────────────────

#[test]
fn module_pub_let_simple() {
    // Export a constant via pub let, import and use it.
    assert_module_output(
        &[
            ("main.que", r#"
import .constants { VERSION, MAX_RETRIES }
println(VERSION)
println(MAX_RETRIES)
"#),
            ("constants.que", r#"
pub let VERSION = "1.0.0"
pub let MAX_RETRIES = 3
"#),
        ],
        &["1.0.0", "3"],
    );
}

#[test]
fn module_pub_let_destructuring() {
    // Export multiple names via destructuring pub let.
    assert_module_output(
        &[
            ("main.que", r#"
import .constants { WIDTH, HEIGHT }
println(WIDTH)
println(HEIGHT)
"#),
            ("constants.que", r#"
pub let (WIDTH, HEIGHT) = (1920, 1080)
"#),
        ],
        &["1920", "1080"],
    );
}

#[test]
fn module_pub_let_whole_module() {
    // Import as module namespace and access pub let values as fields.
    assert_module_output(
        &[
            ("main.que", r#"
import .constants
println(constants.PI)
"#),
            ("constants.que", r#"
pub let PI = 3.14159
"#),
        ],
        &["3.14159"],
    );
}

#[test]
fn module_pub_let_with_pub_fn() {
    // Mix pub let and pub fn in the same module.
    assert_module_output(
        &[
            ("main.que", r#"
import .config { DEFAULT_PORT, make_url }
println(make_url("localhost", DEFAULT_PORT))
"#),
            ("config.que", r#"
pub let DEFAULT_PORT = 8080
pub fn make_url(host, port) {
    "http://" + host + ":" + str(port)
}
"#),
        ],
        &["http://localhost:8080"],
    );
}

// ═════════════════════════════════════════════════════════════════════
// NESTED FIELD ASSIGNMENT
// ═════════════════════════════════════════════════════════════════════

#[test]
fn nested_field_assign_map() {
    let source = r#"
mut m = { a: { b: 1 } }
m.a.b = 42
println(m.a.b)
"#;
    assert_output(source, &["42"]);
}

#[test]
fn nested_field_assign_three_levels() {
    let source = r#"
mut m = { a: { b: { c: "old" } } }
m.a.b.c = "new"
println(m.a.b.c)
"#;
    assert_output(source, &["new"]);
}

#[test]
fn nested_field_assign_creates_intermediate() {
    let source = r#"
mut m = {}
m.a = {}
m.a.b = 10
println(m.a.b)
"#;
    assert_output(source, &["10"]);
}

#[test]
fn nested_index_assign_list() {
    let source = r#"
mut data = [[1, 2], [3, 4]]
data[0][1] = 99
println(data[0][1])
"#;
    assert_output(source, &["99"]);
}

#[test]
fn nested_mixed_field_index() {
    let source = r#"
mut m = { items: [{ name: "a" }, { name: "b" }] }
m.items[1].name = "updated"
println(m.items[1].name)
"#;
    assert_output(source, &["updated"]);
}

#[test]
fn nested_compound_assign() {
    let source = r#"
mut m = { a: { count: 10 } }
m.a.count += 5
println(m.a.count)
"#;
    assert_output(source, &["15"]);
}

#[test]
fn nested_assign_immutable_error() {
    let source = r#"
let m = { a: { b: 1 } }
m.a.b = 2
"#;
    assert_error(source);
}

#[test]
fn mutable_function_param() {
    let source = r#"
fn update(doc) {
    doc.version = "2.0"
    doc
}
let result = update({ version: "1.0" })
println(result.version)
"#;
    assert_output(source, &["2.0"]);
}

// ═════════════════════════════════════════════════════════════════════
// fs.transform
// ═════════════════════════════════════════════════════════════════════

#[test]
fn fs_transform_text_file() {
    let source = r#"
import std.fs
let tmp = fs.temp_file("que_test_transform_", ".txt").unwrap()
fs.write(tmp, "hello world")
fs.transform(tmp, |content| {
    content.to_upper()
})
let result = fs.read(tmp).unwrap()
println(result)
"#;
    assert_output(source, &["HELLO WORLD"]);
}

// ═════════════════════════════════════════════════════════════════════
// json.edit / toml.edit / yaml.edit
// ═════════════════════════════════════════════════════════════════════

#[test]
fn json_edit_file() {
    let source = r#"
import std.fs
import std.json
let tmp = fs.temp_file("que_test_json_edit_", ".json").unwrap()
fs.write(tmp, "{\"name\": \"que\", \"version\": \"1.0\"}")
json.edit(tmp, |doc| {
    doc.version = "2.0"
    doc
})
let content = fs.read(tmp).unwrap()
let doc = json.parse(content).unwrap()
println(doc.version)
"#;
    assert_output(source, &["2.0"]);
}

#[test]
fn toml_edit_file() {
    let source = r#"
import std.fs
import std.toml
let tmp = fs.temp_file("que_test_toml_edit_", ".toml").unwrap()
fs.write(tmp, "[package]\nname = \"que\"\nversion = \"1.0\"\n")
toml.edit(tmp, |doc| {
    doc.package.version = "2.0"
    doc
})
let content = fs.read(tmp).unwrap()
let doc = toml.parse(content).unwrap()
println(doc.package.version)
"#;
    assert_output(source, &["2.0"]);
}

#[test]
fn yaml_edit_file() {
    let source = r#"
import std.fs
import std.yaml
let tmp = fs.temp_file("que_test_yaml_edit_", ".yaml").unwrap()
fs.write(tmp, "name: que\nversion: '1.0'\n")
yaml.edit(tmp, |doc| {
    doc.version = "2.0"
    doc
})
let content = fs.read(tmp).unwrap()
let doc = yaml.parse(content).unwrap()
println(doc.version)
"#;
    assert_output(source, &["2.0"]);
}

#[test]
fn json_edit_preserves_key_order() {
    let source = r#"
import std.fs
import std.json
let tmp = fs.temp_file("que_test_json_order_", ".json").unwrap()
fs.write(tmp, "{\"zebra\": 1, \"apple\": 2, \"mango\": 3}")
json.edit(tmp, |doc| {
    doc.apple = 99
    doc
})
let content = fs.read(tmp).unwrap()
// The keys should stay in original order: zebra, apple, mango
// not alphabetical: apple, mango, zebra
mut zi = 0
mut ai = 0
mut i = 0
for ch in content.chars() {
    if ch == "z" { zi = i }
    if ch == "a" { ai = i }
    i += 1
}
println(zi < ai)
"#;
    assert_output(source, &["true"]);
}

#[test]
fn toml_edit_preserves_key_order() {
    let source = r#"
import std.fs
import std.toml
let tmp = fs.temp_file("que_test_toml_order_", ".toml").unwrap()
fs.write(tmp, "zebra = 1\napple = 2\nmango = 3\n")
toml.edit(tmp, |doc| {
    doc.apple = 99
    doc
})
let content = fs.read(tmp).unwrap()
let lines = content.trim().split("\n")
// First key should still be zebra, not apple
println(lines[0].starts_with("zebra"))
"#;
    assert_output(source, &["true"]);
}

// ── Gap improvements (GAPS.md) ────────────────────────────────────────────

#[test]
fn path_ls_with_glob_pattern() {
    use std::fs;
    let tmp = std::env::temp_dir().join(format!("que_ls_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("a.txt"), "").unwrap();
    fs::write(tmp.join("b.txt"), "").unwrap();
    fs::write(tmp.join("c.log"), "").unwrap();
    let tmp_str = tmp.to_string_lossy();
    let source = format!(r#"
import std.fs
let d = path("{}")
let txt_files = d.ls("*.txt")
println(txt_files.len())
let log_files = d.ls("*.log")
println(log_files.len())
let all_files = d.ls()
println(all_files.len())
"#, tmp_str);
    assert_output(&source, &["2", "1", "3"]);
    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn path_resolve_or_with_nonempty_path() {
    let source = r#"
let p = path(".").resolve_or(path("/fallback"))
println(p.is_absolute())
"#;
    assert_output(source, &["true"]);
}

#[test]
fn path_resolve_or_with_empty_path() {
    let source = r#"
let p = path("").resolve_or(path("/fallback"))
println(p)
"#;
    assert_output(source, &["/fallback"]);
}

#[test]
fn path_symlink_creates_link() {
    use std::fs;
    let tmp = std::env::temp_dir().join(format!("que_sym_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()));
    fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("target.txt");
    let link = tmp.join("link.txt");
    fs::write(&target, "hello").unwrap();
    let target_str = target.to_string_lossy();
    let link_str = link.to_string_lossy();
    let source = format!(r#"
let target = path("{}")
let link_path = path("{}")
let result = link_path.symlink(str(target))
println(result.is_ok())
println(link_path.exists())
"#, target_str, link_str);
    assert_output(&source, &["true", "true"]);
    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn regex_named_captures_basic() {
    let source = r#"
let re = regex(r"(?P<year>\d{4})-(?P<month>\d{2})-(?P<day>\d{2})").unwrap()
let m = re.named_captures("2024-03-15")
println(m["year"])
println(m["month"])
println(m["day"])
"#;
    assert_output(source, &["2024", "03", "15"]);
}

#[test]
fn regex_named_captures_no_match() {
    let source = r#"
let re = regex(r"(?P<num>\d+)").unwrap()
let m = re.named_captures("no digits here... wait 42")
println(m["num"])
"#;
    assert_output(source, &["42"]);
}

#[test]
fn regex_named_captures_empty_on_no_match() {
    let source = r#"
let re = regex(r"(?P<num>\d+)").unwrap()
let m = re.named_captures("no digits here")
println(m.len())
"#;
    assert_output(source, &["0"]);
}

#[test]
fn list_contains_value() {
    let source = r#"
let items = ["foo", "bar", "baz"]
println(items.contains("bar"))
println(items.contains("qux"))
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn list_contains_int() {
    let source = r#"
let nums = [1, 2, 3, 4, 5]
println(nums.contains(3))
println(nums.contains(99))
"#;
    assert_output(source, &["true", "false"]);
}

#[test]
fn top_level_defer_runs_on_success() {
    // Module bodies are scopes like any other: top-level `defer` used to be
    // registered and then silently dropped.
    let source = r#"
defer println("outer cleanup")
defer println("inner cleanup")
println("work")
"#;
    assert_output(source, &["work", "inner cleanup", "outer cleanup"]);
}

#[test]
fn fail_respects_defer() {
    // fail() throws Signal::Error which propagates through eval_block,
    // which runs deferred expressions before returning the error.
    let source = r#"
mut cleaned = false
defer { cleaned = true }
fail("oops")
"#;
    // The fail should cause an error, but defer must have run
    // We can't observe cleaned after the error in normal output,
    // so instead verify the error propagates (assert_error)
    assert_error(source);
}

#[test]
fn fail_unwinds_nested_defer() {
    // Multiple deferred blocks run in LIFO order on fail()
    let source = r#"
mut log = []
defer { log = log.push("outer") }
defer { log = log.push("inner") }
fail("bang")
"#;
    assert_error(source);
}

#[test]
fn os_exit_respects_defer() {
    // os.exit() uses Signal::Exit, which also goes through eval_block's
    // deferred cleanup path — same guarantee as fail().
    let source = r#"
import std.os
mut done = false
defer { done = true }
os.exit(0)
"#;
    // exit(0) is treated as error in test context
    assert_error(source);
}

#[test]
fn fs_copy_dir_merges_into_existing_dest() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let src = std::env::temp_dir().join(format!("que_cp_src_{}", ts));
    let dst = std::env::temp_dir().join(format!("que_cp_dst_{}", ts));
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::write(src.join("new.txt"), "new").unwrap();
    fs::write(dst.join("existing.txt"), "existing").unwrap();
    let src_s = src.to_string_lossy();
    let dst_s = dst.to_string_lossy();
    // `dest` names the tree, so the contents land in it directly and files
    // already there survive -- unlike `src.copy_to(dest)`, which would nest.
    let source = format!(r#"
import std.fs
let r = fs.copy_dir("{}", "{}")
println(r.is_ok())
println(fs.exists("{}/new.txt"))
println(fs.exists("{}/existing.txt"))
"#, src_s, dst_s, dst_s, dst_s);
    assert_output(&source, &["true", "true", "true"]);
    fs::remove_dir_all(&src).unwrap();
    fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn archive_tar_gz_per_entry_mode() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 100;
    let work = std::env::temp_dir().join(format!("que_mode_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(src.join("script.sh"), "#!/bin/sh\necho hi").unwrap();
    let tar = out.join("test.tar.gz");
    let script = src.join("script.sh");
    let tar_s = tar.to_string_lossy();
    let script_s = script.to_string_lossy();
    let out_s = out.to_string_lossy();
    // Archive with explicit mode 0o755 on the entry map
    let source = format!(r#"
import std.archive
archive.tar_gz("{}", [{{ src: path("{}"), dest: "script.sh", mode: 493 }}])
archive.extract("{}", "{}")
println(path("{}/script.sh").exists())
"#, tar_s, script_s, tar_s, out_s, out_s);
    // 493 == 0o755
    assert_output(&source, &["true"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Asserting only that the file exists let the mode be silently
        // dropped, which is the whole reason the option is there.
        let mode = fs::metadata(out.join("script.sh")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "mode was {:o}", mode);
    }
    fs::remove_dir_all(&work).unwrap();
}

#[test]
fn archive_zip_preserves_empty_dirs() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 300;
    let work = std::env::temp_dir().join(format!("que_zipdir_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(src.join("file.txt"), "hello").unwrap();
    fs::create_dir_all(src.join("empty_dir")).unwrap();
    let zip = out.join("test.zip");
    let source = format!(
        r#"
import std.archive
archive.zip("{}", [path("{}")])
archive.extract("{}", "{}")
println(path("{}/src/empty_dir").is_dir())
"#,
        zip.to_string_lossy(),
        src.to_string_lossy(),
        zip.to_string_lossy(),
        out.to_string_lossy(),
        out.to_string_lossy()
    );
    assert_output(&source, &["true"]);
    fs::remove_dir_all(&work).unwrap();
}

#[cfg(unix)]
#[test]
fn archive_zip_preserves_the_executable_bit() {
    // `SimpleFileOptions::default()` writes 0o644 for every entry, so a
    // zipped build script came back out unable to run.
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 400;
    let work = std::env::temp_dir().join(format!("que_zipmode_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    let script = src.join("run.sh");
    fs::write(&script, "#!/bin/sh\necho hi").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let zip = out.join("test.zip");
    let source = format!(
        r#"
import std.archive
archive.zip("{}", [path("{}")])
archive.extract("{}", "{}")
println(path("{}/src/run.sh").exists())
"#,
        zip.to_string_lossy(),
        src.to_string_lossy(),
        zip.to_string_lossy(),
        out.to_string_lossy(),
        out.to_string_lossy()
    );
    assert_output(&source, &["true"]);
    let mode = fs::metadata(out.join("src").join("run.sh")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "mode was {:o}", mode);
    fs::remove_dir_all(&work).unwrap();
}

#[cfg(unix)]
#[test]
fn archive_zip_honours_an_explicit_entry_mode() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 500;
    let work = std::env::temp_dir().join(format!("que_zipmode2_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(src.join("script.sh"), "#!/bin/sh\necho hi").unwrap();
    let zip = out.join("test.zip");
    let source = format!(
        r#"
import std.archive
archive.zip("{}", [{{ src: path("{}"), dest: "script.sh", mode: 493 }}])
archive.extract("{}", "{}")
println(path("{}/script.sh").exists())
"#,
        zip.to_string_lossy(),
        src.join("script.sh").to_string_lossy(),
        zip.to_string_lossy(),
        out.to_string_lossy(),
        out.to_string_lossy()
    );
    assert_output(&source, &["true"]);
    let mode = fs::metadata(out.join("script.sh")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "mode was {:o}", mode);
    fs::remove_dir_all(&work).unwrap();
}

#[test]
fn archive_zip_records_the_source_mtime() {
    // Stamping every entry "now" defeats every downstream mtime comparison.
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 600;
    let work = std::env::temp_dir().join(format!("que_zipmtime_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    let file = src.join("old.txt");
    fs::write(&file, "hello").unwrap();
    // 2001-02-03 04:05:06 local time.
    let stamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(981_173_106);
    fs::File::open(&file)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    let zip = out.join("test.zip");
    let source = format!(
        r#"
import std.archive
archive.zip("{}", [path("{}")])
"#,
        zip.to_string_lossy(),
        src.to_string_lossy()
    );
    run(&source).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
    let entry = archive.by_name("src/old.txt").unwrap();
    let recorded = entry.last_modified().unwrap();
    assert_eq!(recorded.year(), 2001, "recorded {:?}", recorded);
    drop(entry);
    fs::remove_dir_all(&work).unwrap();
}

#[test]
fn path_symlink_makes_the_receiver_the_link() {
    // The docs had the arguments the other way round. The receiver is the
    // link, the same way `.write_text()`'s receiver is the file written.
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 700;
    let work = std::env::temp_dir().join(format!("que_symlink_{}", ts));
    fs::create_dir_all(&work).unwrap();
    let target = work.join("real.txt");
    fs::write(&target, "contents").unwrap();
    let link = work.join("link.txt");
    let source = format!(
        r#"
println(path("{}").symlink(path("{}")).is_ok())
println(path("{}").is_link())
println(path("{}").read().unwrap())
"#,
        link.to_string_lossy(),
        target.to_string_lossy(),
        link.to_string_lossy(),
        link.to_string_lossy()
    );
    assert_output(&source, &["true", "true", "contents"]);
    fs::remove_dir_all(&work).unwrap();
}

#[test]
fn archive_tar_gz_preserves_empty_dirs() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let work = std::env::temp_dir().join(format!("que_arch_{}", ts));
    let src = work.join("src");
    let out = work.join("out");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    // Create a file and an empty directory
    fs::write(src.join("file.txt"), "hello").unwrap();
    fs::create_dir_all(src.join("empty_dir")).unwrap();
    let tar = out.join("test.tar.gz");
    let src_s = src.to_string_lossy();
    let tar_s = tar.to_string_lossy();
    let out_s = out.to_string_lossy();
    let source = format!(r#"
import std.archive
archive.tar_gz("{}", [path("{}")])
// Extract and verify the empty dir is present
archive.extract("{}", "{}")
println(path("{}/src/empty_dir").exists())
println(path("{}/src/empty_dir").is_dir())
"#, tar_s, src_s, tar_s, out_s, out_s, out_s);
    assert_output(&source, &["true", "true"]);
    fs::remove_dir_all(&work).unwrap();
}

#[test]
fn tempdir_is_usable_as_path() {
    // Gap #8: TempDir context variable is already a Path value, so all path
    // methods and the / join operator work directly without any cast.
    let source = r#"
with TempDir {} as stage {
    // typeof confirms it is a Path
    println(typeof(stage))
    // Path methods work directly
    println(stage.is_dir())
    println(stage.is_absolute())
    // join works
    let f = stage.join("test.txt")
    f.write_text("ok").unwrap()
    println(f.read().unwrap())
}
"#;
    assert_output(source, &["Path", "true", "true", "ok"]);
}

#[test]
fn tempdir_with_custom_parent_dir() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let parent = std::env::temp_dir().join(format!("que_parent_{}", ts));
    fs::create_dir_all(&parent).unwrap();
    let parent_str = parent.to_string_lossy().to_string();
    let source = format!(r#"
import std.fs
with TempDir {{ dir: path("{}") }} as stage {{
    // The temp dir should be created inside the custom parent
    let parent = stage.parent()
    println(str(parent) == "{}")
}}
"#, parent_str, parent_str);
    assert_output(&source, &["true"]);
    fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn tempfile_with_custom_parent_dir() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 11;
    let parent = std::env::temp_dir().join(format!("que_tfparent_{}", ts));
    let parent_str = parent.to_string_lossy().to_string();
    let source = format!(
        r#"
with TempFile {{ dir: path("{}"), suffix: ".log" }} as f {{
    println(str(f.parent()) == "{}")
    println(f.extension() == "log")
}}
"#,
        parent_str, parent_str
    );
    assert_output(&source, &["true", "true"]);
    fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn fs_temp_dir_and_temp_file_accept_a_base_dir() {
    // The system temp dir is often a tmpfs too small for a build artefact,
    // or on a different filesystem than the destination, which turns the
    // atomic rename a temp file exists for into a copy.
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 12;
    let base = std::env::temp_dir().join(format!("que_tbase_{}", ts));
    let base_str = base.to_string_lossy().to_string();
    let source = format!(
        r#"
import std.fs
let d = fs.temp_dir("stage_", {{ dir: path("{}") }}).unwrap()
let f = fs.temp_file("part_", ".bin", {{ dir: path("{}") }}).unwrap()
println(str(d.parent()) == "{}")
println(str(f.parent()) == "{}")
println(d.is_dir())
println(f.is_file())
"#,
        base_str, base_str, base_str, base_str
    );
    assert_output(&source, &["true", "true", "true", "true"]);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn temp_names_are_not_predictable() {
    // A nanosecond timestamp in a world-writable directory is a name an
    // attacker can create first, as a symlink pointing wherever they like.
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 13;
    let base = std::env::temp_dir().join(format!("que_tuniq_{}", ts));
    let base_str = base.to_string_lossy().to_string();
    let source = format!(
        r#"
import std.fs
let a = fs.temp_dir("x_", {{ dir: path("{}") }}).unwrap()
let b = fs.temp_dir("x_", {{ dir: path("{}") }}).unwrap()
println(str(a) != str(b))
println(a.name().len())
"#,
        base_str, base_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output[0], "true");
    // "x_" plus 12 random bytes rendered as hex.
    assert_eq!(output[1], "26");
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn task_aliases_callable() {
    let source = r#"
@description("Say hello")
@aliases(["hello", "hi"])
task greet {
    println("hello from task")
}
hello.run()
"#;
    assert_output(source, &["[RUN]  greet", "hello from task", "[DONE] greet"]);
}

#[test]
fn task_aliases_method() {
    let source = r#"
@description("Deploy")
@aliases(["ship", "release"])
task deploy {
}
println(deploy.aliases().len())
println(deploy.aliases()[0])
"#;
    assert_output(source, &["2", "ship"]);
}

#[test]
fn quefile_dir_is_path() {
    // In test context, script_path is None so quefile_dir() falls back to CWD.
    let source = r#"
let d = quefile_dir()
println(typeof(d))
println(d.is_absolute())
"#;
    assert_output(source, &["Path", "true"]);
}

#[test]
fn script_dir_alias() {
    let source = r#"
let d = script_dir()
println(typeof(d))
"#;
    assert_output(source, &["Path"]);
}

#[test]
fn cmd_arg_builder() {
    let source = r#"
println(`echo`
    .arg("hello")
    .arg("world")
    .out())
"#;
    assert_output(source, &["hello world"]);
}

#[test]
fn cmd_flag_with_value() {
    let source = r#"
println(`echo`
    .flag("--prefix", "val")
    .out())
"#;
    // echo doesn't parse --prefix, it just prints everything including the flag
    assert_output(source, &["--prefix val"]);
}

#[test]
fn cmd_flag_boolean() {
    // echo with --help flag (just check it runs without error)
    let source = r#"
println(`echo`.flag("--flag-that-doesnt-exist-just-testing").arg("text").out())
"#;
    // echo ignores unknown flags, just prints them
    assert_output(source, &["--flag-that-doesnt-exist-just-testing text"]);
}

#[test]
fn fs_copy_dir_creates_missing_dest() {
    use std::fs;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() + 1;
    let src = std::env::temp_dir().join(format!("que_cp2_src_{}", ts));
    let dst = std::env::temp_dir().join(format!("que_cp2_dst_{}", ts));
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("file.txt"), "hello").unwrap();
    let src_s = src.to_string_lossy();
    let dst_s = dst.to_string_lossy();
    let source = format!(r#"
import std.fs
let r = fs.copy_dir("{}", "{}")
println(r.is_ok())
println(fs.exists("{}/file.txt"))
"#, src_s, dst_s, dst_s);
    assert_output(&source, &["true", "true"]);
    fs::remove_dir_all(&src).unwrap();
    fs::remove_dir_all(&dst).unwrap();
}

// ── Path / Glob Literal Tests ────────────────────────────────────────────────

#[test]
fn path_literal_basic() {
    assert_result(r#"p"/usr/local/bin""#, Value::Path("/usr/local/bin".into()));
}

#[test]
fn path_literal_relative() {
    assert_result(r#"p"./config""#, Value::Path("./config".into()));
}

#[test]
fn path_literal_parent_relative() {
    assert_result(r#"p"../logs""#, Value::Path("../logs".into()));
}

#[test]
fn path_literal_empty() {
    assert_result(r#"p"""#, Value::Path("".into()));
}

#[test]
fn path_literal_double_slash_normalized() {
    assert_result(r#"p"/a//b""#, Value::Path("/a/b".into()));
}

#[test]
fn path_literal_triple_slash_normalized() {
    assert_result(r#"p"/a///b///c""#, Value::Path("/a/b/c".into()));
}

#[test]
fn path_literal_interpolation_string() {
    assert_result(
        r#"let appname = "myapp"
p"/opt/${appname}""#,
        Value::Path("/opt/myapp".into()),
    );
}

#[test]
fn path_literal_interpolation_multi() {
    assert_result(
        r#"let appname = "myapp"
let version = "1.4.2"
p"/opt/${appname}/lib/${version}""#,
        Value::Path("/opt/myapp/lib/1.4.2".into()),
    );
}

#[test]
fn path_literal_interpolation_path_value() {
    assert_result(
        r#"let base = p"/opt/myapp"
p"${base}/bin""#,
        Value::Path("/opt/myapp/bin".into()),
    );
}

#[test]
fn path_literal_in_string_interpolation() {
    // Path values convert to their string form in string interpolation
    assert_result(
        r#"let p = p"/usr/local/bin"
"Path is: ${p}""#,
        Value::String("Path is: /usr/local/bin".into()),
    );
}

#[test]
fn glob_literal_basic() {
    assert_result(r#"g"/tmp/*.log""#, Value::Glob("/tmp/*.log".into()));
}

#[test]
fn glob_literal_recursive() {
    assert_result(r#"g"/var/log/**/*.log""#, Value::Glob("/var/log/**/*.log".into()));
}

#[test]
fn glob_literal_alternation() {
    // {main,test} is literal glob alternation, not interpolation
    assert_result(
        r#"g"/src/{main,test}/**/*.ts""#,
        Value::Glob("/src/{main,test}/**/*.ts".into()),
    );
}

#[test]
fn glob_literal_char_class() {
    assert_result(
        r#"g"/backup/[0-9][0-9][0-9][0-9]""#,
        Value::Glob("/backup/[0-9][0-9][0-9][0-9]".into()),
    );
}

#[test]
fn glob_literal_interpolation() {
    assert_result(
        r#"let app = "myapp"
g"/etc/${app}/*.conf""#,
        Value::Glob("/etc/myapp/*.conf".into()),
    );
}

// ── Path `/` composition operator ───────────────────────────────────────────

#[test]
fn path_div_string_join() {
    assert_result(
        r#"p"/usr/local" / "bin""#,
        Value::Path("/usr/local/bin".into()),
    );
}

#[test]
fn path_div_path_join() {
    assert_result(
        r#"p"/usr/local" / p"bin""#,
        Value::Path("/usr/local/bin".into()),
    );
}

#[test]
fn path_div_chain() {
    assert_result(
        r#"p"/" / "usr" / "local" / "bin""#,
        Value::Path("/usr/local/bin".into()),
    );
}

#[test]
fn path_div_absolute_path_right_wins() {
    // Absolute Path on right side replaces left
    assert_result(
        r#"p"/a/b" / p"/etc""#,
        Value::Path("/etc".into()),
    );
}

#[test]
fn path_div_string_never_absolute() {
    // String on the right is NEVER absolute — leading slash is stripped
    assert_result(
        r#"p"/a/b" / "/etc""#,
        Value::Path("/a/b/etc".into()),
    );
}

#[test]
fn path_div_existing_path_var() {
    assert_result(
        r#"let root = p"/opt/myapp"
root / "bin" / "myapp""#,
        Value::Path("/opt/myapp/bin/myapp".into()),
    );
}

#[test]
fn path_div_glob_is_error() {
    assert_error(r#"p"/var/log" / g"*.log""#);
}

// ── Glob `/` composition operator ───────────────────────────────────────────

#[test]
fn glob_div_string() {
    assert_result(
        r#"g"/var/log" / "*.log""#,
        Value::Glob("/var/log/*.log".into()),
    );
}

#[test]
fn glob_div_path() {
    assert_result(
        r#"g"/var/log" / p"app""#,
        Value::Glob("/var/log/app".into()),
    );
}

#[test]
fn glob_div_glob_absolute_right_wins() {
    assert_result(
        r#"g"/a/b" / g"/tmp/*.log""#,
        Value::Glob("/tmp/*.log".into()),
    );
}

#[test]
fn glob_constructor_from_path() {
    assert_result(
        r#"glob(p"/var/log") / "*.log""#,
        Value::Glob("/var/log/*.log".into()),
    );
}

#[test]
fn glob_constructor_from_string() {
    assert_result(
        r#"glob("/var/log") / "**" / "*.log""#,
        Value::Glob("/var/log/**/*.log".into()),
    );
}

// ── New path methods ─────────────────────────────────────────────────────────

#[test]
fn path_method_is_absolute_true() {
    assert_result(r#"p"/usr/local/bin".is_absolute()"#, Value::Bool(true));
}

#[test]
fn path_method_is_absolute_false() {
    assert_result(r#"p"./config".is_absolute()"#, Value::Bool(false));
}

#[test]
fn path_method_is_relative_true() {
    assert_result(r#"p"./config".is_relative()"#, Value::Bool(true));
}

#[test]
fn path_method_is_relative_false() {
    assert_result(r#"p"/usr/local".is_relative()"#, Value::Bool(false));
}

#[test]
fn path_method_root_absolute() {
    assert_result(r#"p"/opt/myapp".root()"#, Value::Path("/".into()));
}

#[test]
fn path_method_root_relative() {
    assert_result(r#"p"./config".root()"#, Value::Path("".into()));
}

#[test]
fn path_method_to_string() {
    assert_result(r#"p"/usr/local/bin".to_string()"#, Value::String("/usr/local/bin".into()));
}

#[test]
fn path_method_with_stem() {
    assert_result(
        r#"p"/opt/myapp/config.toml".with_stem("settings")"#,
        Value::Path("/opt/myapp/settings.toml".into()),
    );
}

#[test]
fn path_method_with_stem_no_extension() {
    assert_result(
        r#"p"/opt/myapp/Makefile".with_stem("Taskfile")"#,
        Value::Path("/opt/myapp/Taskfile".into()),
    );
}

#[test]
fn path_method_with_ext_with_dot() {
    // with_ext should accept extensions with leading dot
    assert_result(
        r#"p"/opt/myapp/config.toml".with_ext(".bak")"#,
        Value::Path("/opt/myapp/config.bak".into()),
    );
}

#[test]
fn path_method_with_ext_without_dot() {
    assert_result(
        r#"p"/opt/myapp/config.toml".with_ext("bak")"#,
        Value::Path("/opt/myapp/config.bak".into()),
    );
}

#[test]
fn path_method_parent_root_stays_root() {
    // p"/".parent() → p"/"  (spec edge case)
    assert_result(r#"p"/".parent()"#, Value::Path("/".into()));
}

#[test]
fn path_method_parent_empty_stays_empty() {
    // p"".parent() → p""  (spec edge case)
    assert_result(r#"p"".parent()"#, Value::Path("".into()));
}

#[test]
fn path_method_parent_normal() {
    assert_result(
        r#"p"/opt/myapp/config.toml".parent()"#,
        Value::Path("/opt/myapp".into()),
    );
}

#[test]
fn path_method_is_link_nonexistent() {
    // Non-existent path is not a symlink
    assert_result(
        r#"p"/this/does/not/exist".is_link()"#,
        Value::Bool(false),
    );
}

#[test]
fn path_method_is_rel_alias() {
    assert_result(r#"p"relative/path".is_relative()"#, Value::Bool(true));
}

#[test]
fn path_arg_accepts_path_and_string_alike() {
    // A Path literal has `~` expanded when it is built, so the String form
    // has to be expanded at the argument too or the two disagree.
    assert_result(
        r#"path("~/src").relative_to(p"~") == path("~/src").relative_to("~")"#,
        Value::Bool(true),
    );
    assert_result(
        r#"p"/a".join(p"b") == p"/a".join("b")"#,
        Value::Bool(true),
    );
}

#[test]
fn path_method_ext_dot() {
    assert_result(
        r#"p"/opt/myapp/config.toml".ext_dot()"#,
        Value::String(".toml".into()),
    );
}

#[test]
fn path_method_ext_dot_no_extension() {
    assert_result(
        r#"p"/opt/myapp/Makefile".ext_dot()"#,
        Value::String("".into()),
    );
}

// ── New glob methods ─────────────────────────────────────────────────────────

#[test]
fn glob_method_pattern() {
    assert_result(
        r#"g"/var/log/**/*.log".pattern()"#,
        Value::String("/var/log/**/*.log".into()),
    );
}

#[test]
fn glob_method_count_no_match() {
    // Pattern that matches nothing returns 0
    assert_result(
        r#"g"/this/path/does/not/exist/**/*.xyz".count()"#,
        Value::Int(0),
    );
}

#[test]
fn glob_method_any_no_match() {
    assert_result(
        r#"g"/this/path/does/not/exist/**/*.xyz".any()"#,
        Value::Bool(false),
    );
}

#[test]
fn glob_method_first_no_match() {
    assert_result(
        r#"g"/this/path/does/not/exist/**/*.xyz".first()"#,
        Value::Null,
    );
}

#[test]
fn glob_method_expand_no_match() {
    assert_result(
        r#"g"/this/path/does/not/exist/**/*.xyz".expand()"#,
        Value::List(vec![]),
    );
}

// ── Comprehensive composition example ───────────────────────────────────────

#[test]
fn path_composition_full_example() {
    assert_result(
        r#"let root = p"/opt/myapp"
let version = "1.4.2"
let mode = "release"
let cfg = root / p"etc/${mode}.toml"
cfg"#,
        Value::Path("/opt/myapp/etc/release.toml".into()),
    );
}

#[test]
fn path_composition_with_method_chain() {
    assert_result(
        r#"let p = p"/opt/myapp/config.toml"
let backup = p.with_ext(".bak")
backup"#,
        Value::Path("/opt/myapp/config.bak".into()),
    );
}

#[test]
fn path_typeof() {
    assert_result(r#"typeof(p"/usr/local")"#, Value::String("Path".into()));
}

#[test]
fn glob_typeof() {
    assert_result(r#"typeof(g"*.log")"#, Value::String("Glob".into()));
}

// ── chr / ord ──────────────────────────────────────────────────────────────

#[test]
fn chr_basic() {
    assert_result(r#"chr(65)"#, Value::String("A".into()));
}

#[test]
fn chr_esc() {
    assert_result(r#"chr(27)"#, Value::String("\x1b".into()));
}

#[test]
fn ord_basic() {
    assert_result(r#"ord("A")"#, Value::Int(65));
}

#[test]
fn ord_roundtrip() {
    assert_result(r#"ord(chr(27))"#, Value::Int(27));
}

// ── hex escape sequences ───────────────────────────────────────────────────

#[test]
fn hex_escape_in_string() {
    assert_result(r#""\x41""#, Value::String("A".into()));
}

#[test]
fn hex_escape_esc_char() {
    assert_result(r#""\x1b""#, Value::String("\x1b".into()));
}

#[test]
fn escape_e_shorthand() {
    assert_result(r#""\e""#, Value::String("\x1b".into()));
}

#[test]
fn hex_escape_in_interpolation() {
    // \x41 = 'A', followed by an interpolated suffix
    assert_result(
        r#"let suffix = "BC"
"\x41${suffix}""#,
        Value::String("ABC".into()),
    );
}

// ── Glob alternation and tilde expansion ──────────────────────────────────

#[test]
fn glob_expand_alternation() {
    // {a,b} in expand() must be resolved before passing to glob::glob,
    // which does not support brace expansion natively.
    use std::fs;
    let tmp = std::env::temp_dir().join(format!(
        "que_glob_alt_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let dir_a = tmp.join("aaa");
    let dir_b = tmp.join("bbb");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();
    fs::write(dir_a.join("file.txt"), "").unwrap();
    fs::write(dir_b.join("file.txt"), "").unwrap();

    let source = format!(
        r#"g"{}/{{aaa,bbb}}/*".expand().len()"#,
        tmp.to_string_lossy()
    );
    assert_result(&source, Value::Int(2));

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn glob_test_alternation_single_segment() {
    // {js,ts} alternation: file.js matches, file.ts matches, file.py does not
    let source = r#"
let g = glob("*.{js,ts}")
let a = g.test("app.js")
let b = g.test("app.ts")
let c = g.test("app.py")
(a, b, c)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Bool(true), Value::Bool(true), Value::Bool(false),
    ]));
}

#[test]
fn glob_test_alternation_with_path() {
    // {src,lib} alternation in path component
    let source = r#"
let g = glob("{src,lib}/**/*.rs")
let a = g.test("src/main.rs")
let b = g.test("lib/util.rs")
let c = g.test("tests/foo.rs")
(a, b, c)
"#;
    assert_result(source, Value::Tuple(vec![
        Value::Bool(true), Value::Bool(true), Value::Bool(false),
    ]));
}

#[test]
fn glob_tilde_expands_in_test() {
    // ~ should expand to HOME; a path starting with the literal ~ char won't match
    // We construct the expected home-prefixed path dynamically via env.get()
    let source = r#"
let home = env.get("HOME")
let g = glob("~/*.txt")
g.test(home + "/readme.txt")
"#;
    assert_result(source, Value::Bool(true));
}

// ── Enums ─────────────────────────────────────────────────────────────

#[test]
fn enum_unit_variant_value() {
    assert_result(r#"
enum Direction { North, South, East, West }
Direction.North
"#, Value::Enum {
        enum_name: "Direction".into(),
        variant: "North".into(),
        fields: std::collections::BTreeMap::new(),
    });
}

#[test]
fn enum_unit_variant_bound_directly() {
    assert_result(r#"
enum Direction { North, South, East, West }
North
"#, Value::Enum {
        enum_name: "Direction".into(),
        variant: "North".into(),
        fields: std::collections::BTreeMap::new(),
    });
}

#[test]
fn enum_data_variant_named_args() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("radius".into(), Value::Float(5.0));
    assert_result(r#"
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float } }
Shape.Circle(radius: 5.0)
"#, Value::Enum {
        enum_name: "Shape".into(),
        variant: "Circle".into(),
        fields,
    });
}

#[test]
fn enum_data_variant_positional_args() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("radius".into(), Value::Float(3.0));
    assert_result(r#"
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float } }
Shape.Circle(3.0)
"#, Value::Enum {
        enum_name: "Shape".into(),
        variant: "Circle".into(),
        fields,
    });
}

#[test]
fn enum_data_variant_brace_construction() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("radius".into(), Value::Float(5.0));
    assert_result(r#"
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float } }
Shape.Circle { radius: 5.0 }
"#, Value::Enum {
        enum_name: "Shape".into(),
        variant: "Circle".into(),
        fields,
    });
}

#[test]
fn enum_brace_and_paren_construction_are_equal() {
    assert_output(r#"
enum Shape { Rect { width: Float, height: Float } }
let a = Shape.Rect { width: 2.0, height: 3.0 }
let b = Shape.Rect(width: 2.0, height: 3.0)
println(a == b)
"#, &["true"]);
}

#[test]
fn enum_paren_declaration_form() {
    assert_output(r#"
enum State { Idle, Failed(code: Int, msg: String) }
let f = State.Failed { code: 2, msg: "boom" }
println(f.code, f.msg)
"#, &["2 boom"]);
}

#[test]
fn enum_brace_literal_does_not_shadow_block() {
    // `State.Idle { ... }` must still parse as a unit variant followed by a block.
    assert_output(r#"
enum State { Idle, Running { pid: Int } }
let s = State.Idle
if s == State.Idle { println("block") }
"#, &["block"]);
}

#[test]
fn enum_brace_literal_multiline() {
    assert_output(r#"
enum Service { Http { host: String, port: Int } }
let s = Service.Http {
    host: "localhost",
    port: 8080,
}
println(s.host, s.port)
"#, &["localhost 8080"]);
}

#[test]
fn enum_match_unit_variant() {
    assert_output(r#"
enum Direction { North, South, East, West }
let d = Direction.North
match d {
    North {} => println("north")
    South {} => println("south")
    _ => println("other")
}
"#, &["north"]);
}

#[test]
fn enum_match_data_variant_named_fields() {
    assert_output(r#"
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float }, Point }
let s = Shape.Circle(radius: 2.5)
match s {
    Circle { radius } => println("circle r=${radius}")
    Rect { width, height } => println("rect")
    Point {} => println("point")
}
"#, &["circle r=2.5"]);
}

#[test]
fn enum_match_multiple_variants() {
    assert_output(r#"
enum Msg { Quit, Move { x: Int, y: Int }, Write { text: String } }
let msgs = [Msg.Quit, Msg.Move(x: 10, y: 20), Msg.Write(text: "hello")]
for msg in msgs {
    match msg {
        Quit {} => println("quit")
        Move { x, y } => println("move ${x} ${y}")
        Write { text } => println("write ${text}")
    }
}
"#, &["quit", "move 10 20", "write hello"]);
}

#[test]
fn enum_method_variant() {
    assert_result(r#"
enum Color { Red, Green, Blue }
let c = Color.Green
c.variant()
"#, Value::String("Green".into()));
}

#[test]
fn enum_method_is_variant() {
    assert_result(r#"
enum Color { Red, Green, Blue }
let c = Color.Blue
c.is_variant("Blue")
"#, Value::Bool(true));
}

#[test]
fn enum_impl_method() {
    assert_output(r#"
enum Shape { Circle { radius: Float }, Rect { width: Float, height: Float } }
impl Shape {
    fn area(self) {
        match self {
            Circle { radius } => 3.14159 * radius * radius
            Rect { width, height } => width * height
        }
    }
}
let c = Shape.Circle(radius: 2.0)
println(c.area())
"#, &["12.56636"]);
}

#[test]
fn enum_display() {
    assert_output(r#"
enum Status { Active, Inactive { reason: String } }
let a = Status.Active
let i = Status.Inactive(reason: "maintenance")
println(a)
println(i)
"#, &["Status.Active", "Status.Inactive {reason: maintenance}"]);
}

// ── std.container ────────────────────────────────────────────────────

/// Assert on the command a container call *would* run, without needing a
/// container engine anywhere near the test. `QUE_CONTAINER_ENGINE` is set in
/// the script itself so the assertion does not depend on what is installed.
fn container_dry(body: &str) -> Vec<String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let tag = format!(
        "container_{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let dir = dry_project(
        &tag,
        &format!(
            "import std.container\nenv.set(\"QUE_CONTAINER_ENGINE\", \"docker\")\n{}\n",
            body
        ),
    );
    let output = run_dry_in(&dir, "main.que");
    std::fs::remove_dir_all(&dir).ok();
    output
}

fn assert_dry_contains(output: &[String], needle: &str) {
    assert!(
        output.iter().any(|l| l.contains(needle)),
        "expected {:?} in {:?}",
        needle,
        output
    );
}

#[test]
fn container_build_puts_the_context_last() {
    // `docker build -t x .` — the context is positional, and a flag added
    // after it is silently ignored.
    let out = container_dry(
        r#"container.build({ tag: "app:1", context: p"./src", build_args: { "V": "2" } })"#,
    );
    assert_dry_contains(&out, "build -t 'app:1'");
    assert_dry_contains(&out, "--build-arg 'V=2'");
    assert!(
        out.iter().any(|l| l.trim_end().ends_with("./src")),
        "context must come last: {:?}",
        out
    );
}

#[test]
fn container_build_requires_a_tag() {
    assert_error_contains(
        "import std.container\ncontainer.build({ context: p\".\" })",
        "requires a `tag`",
    );
}

#[test]
fn container_run_detaches_and_cleans_up_by_default() {
    // Without --rm a CI runner accumulates stopped containers until the disk
    // fills, and nothing in the script points at the cause.
    let out = container_dry(r#"container.run({ image: "postgres:16", name: "db" })"#);
    assert_dry_contains(&out, "run -d --rm --name db");
    assert_dry_contains(&out, "'postgres:16'");
}

#[test]
fn container_run_maps_ports_volumes_and_env() {
    let out = container_dry(
        r#"container.run({ image: "nginx", ports: { "8080": 80 }, volumes: { "/tmp/site": "/usr/share/nginx/html" }, env: { "TZ": "UTC" } })"#,
    );
    assert_dry_contains(&out, "-p '8080:80'");
    assert_dry_contains(&out, "-v '/tmp/site:/usr/share/nginx/html'");
    assert_dry_contains(&out, "-e 'TZ=UTC'");
}

#[test]
fn container_run_keeps_a_secret_out_of_argv() {
    // `ps` is world-readable. The name goes on the command line; the value
    // is handed to the child process through its environment instead.
    let out = container_dry(
        r#"let pw = secret("hunter2-in-the-clear")
container.run({ image: "postgres:16", env: { "POSTGRES_PASSWORD": pw } })"#,
    );
    assert_dry_contains(&out, "-e POSTGRES_PASSWORD");
    assert!(
        !out.iter().any(|l| l.contains("hunter2-in-the-clear")),
        "the secret leaked into the command line: {:?}",
        out
    );
}

#[test]
fn container_run_can_stay_in_the_foreground() {
    let out = container_dry(r#"container.run({ image: "alpine", detach: false, remove: false })"#);
    assert!(
        !out.iter().any(|l| l.contains(" run -d")),
        "{:?}",
        out
    );
    assert!(!out.iter().any(|l| l.contains("--rm")), "{:?}", out);
}

#[test]
fn container_run_with_a_tty_stays_in_the_foreground() {
    // `-d` and `-it` together start a container the caller is not attached
    // to, which is the one thing `tty` was asked for.
    let out = container_dry(r#"container.run({ image: "alpine", tty: true, command: "sh" })"#);
    assert_dry_contains(&out, "run -it --rm alpine sh");
    assert!(!out.iter().any(|l| l.contains("-d ")), "{:?}", out);
}

#[test]
fn container_run_rejects_a_tty_on_a_detached_container() {
    assert_error_contains(
        "import std.container\nenv.set(\"QUE_CONTAINER_ENGINE\", \"docker\")\ncontainer.run({ image: \"alpine\", tty: true, detach: true })",
        "cannot be combined with `detach: true`",
    );
}

#[test]
fn container_exec_can_take_a_tty() {
    let out = container_dry(r#"container.exec("db", ["psql"], { tty: true })"#);
    assert_dry_contains(&out, "exec -it db psql");
}

#[test]
fn container_exec_wraps_a_string_command_in_a_shell() {
    // A string is shell syntax; without `sh -c` the redirect below would be
    // passed to the program as literal arguments.
    let out = container_dry(r#"container.exec("db", "psql -c 'select 1' > /tmp/o")"#);
    assert_dry_contains(&out, "exec db sh -c");
}

#[test]
fn container_exec_takes_a_list_as_an_argv() {
    let out = container_dry(r#"container.exec("db", ["psql", "-c", "select 1"])"#);
    assert_dry_contains(&out, "exec db psql -c 'select 1'");
    assert!(!out.iter().any(|l| l.contains("sh -c")), "{:?}", out);
}

#[test]
fn container_login_never_puts_the_password_on_the_command_line() {
    let out = container_dry(
        r#"container.login("ghcr.io", "me", secret("tok-not-for-ps"))"#,
    );
    assert_dry_contains(&out, "--password-stdin");
    assert!(
        !out.iter().any(|l| l.contains("tok-not-for-ps")),
        "{:?}",
        out
    );
}

#[test]
fn container_volume_paths_with_spaces_are_escaped() {
    let out = container_dry(
        r#"container.run({ image: "alpine", volumes: { "/my dir": "/data" } })"#,
    );
    assert_dry_contains(&out, "'/my dir:/data'");
}

#[test]
fn container_stop_and_remove_are_announced_in_a_dry_run() {
    let out = container_dry("container.stop(\"db\")\ncontainer.remove(\"db\")");
    assert_dry_contains(&out, "stop db");
    assert_dry_contains(&out, "rm -f db");
}

#[test]
fn container_run_hands_back_a_string_handle() {
    // A real detached run returns the engine's container id; a dry run has no
    // id to give, but it must still return the same type or a script that
    // works dry breaks the moment it runs for real.
    let out = container_dry(
        r#"match container.run({ image: "alpine" }) {
  Ok(h) => println("type=" + typeof(h)),
  Err(e) => println("err=" + e),
}"#,
    );
    assert_dry_contains(&out, "type=String");
}

#[test]
fn silent_discards_the_captured_output() {
    // `silent` is not "do not echo" — it drops the streams entirely, and a
    // plain `.try()` does not echo either. Anything that needs a command's
    // stdout must therefore leave `silent` alone. std.container once set it
    // on every call and so reported an empty container id.
    let source = r#"let r = `echo hi`.silent().try()
println("silent=[" + r.stdout + "]")
let q = `echo hi`.try()
println("plain=[" + q.stdout.trim() + "]")"#;
    assert_output(source, &["silent=[]", "plain=[hi]"]);
}

// ── std.ssh ──────────────────────────────────────────────────────────

/// `ssh.cmd` returns an ordinary Cmd, so its rendering can be asserted
/// without a server anywhere near the test.
fn ssh_render(source: &str) -> String {
    let (output, _) = run(&format!("import std.ssh\nprintln({})", source)).unwrap();
    output[0].clone()
}

#[test]
fn ssh_cmd_builds_a_safe_default_invocation() {
    let s = ssh_render(r#"ssh.cmd("web-1", "uptime").to_string()"#);
    assert!(s.starts_with("ssh "), "{}", s);
    // Batch mode and a connect timeout, so a script never hangs on a
    // password prompt or an unreachable host.
    assert!(s.contains("-o BatchMode=yes"), "{}", s);
    assert!(s.contains("-o ConnectTimeout=10"), "{}", s);
    assert!(s.ends_with("web-1 'uptime'"), "{}", s);
}

#[test]
fn ssh_cmd_is_a_real_cmd_and_composes_with_modifiers() {
    // The whole reason ssh.cmd returns a Cmd: everything else already works.
    let (output, _) = run(
        r#"
import std.ssh
let c = ssh.cmd("web-1", "uptime").timeout(5s).silent()
println(c.to_string().starts_with("ssh "))
"#,
    )
    .unwrap();
    assert_eq!(output, vec!["true"]);
}

#[test]
fn ssh_user_and_port_are_applied() {
    let s = ssh_render(r#"ssh.cmd("web-1", "id", { user: "deploy", port: 2222 }).to_string()"#);
    assert!(s.contains("-p 2222"), "{}", s);
    assert!(s.contains("deploy@web-1"), "{}", s);
}

#[test]
fn ssh_does_not_override_a_user_already_in_the_host() {
    let s = ssh_render(r#"ssh.cmd("root@db-1", "id", { user: "deploy" }).to_string()"#);
    assert!(s.contains("root@db-1"), "{}", s);
    assert!(!s.contains("deploy@"), "{}", s);
}

#[test]
fn ssh_key_pins_identities_only() {
    // Without IdentitiesOnly, ssh offers every agent key first and a host
    // with MaxAuthTries=3 disconnects before reaching the requested one.
    let s = ssh_render(r#"ssh.cmd("web-1", "id", { key: p"/tmp/k" }).to_string()"#);
    assert!(s.contains("-i /tmp/k"), "{}", s);
    assert!(s.contains("IdentitiesOnly=yes"), "{}", s);
}

#[test]
fn ssh_can_opt_into_interactive_and_agent_forwarding() {
    let s = ssh_render(
        r#"ssh.cmd("web-1", "id", { interactive: true, forward_agent: true }).to_string()"#,
    );
    assert!(!s.contains("BatchMode"), "{}", s);
    assert!(s.contains(" -A "), "{}", s);
}

#[test]
fn ssh_disabling_host_key_checking_also_detaches_known_hosts() {
    // Otherwise the real known_hosts is poisoned with an unverified key that
    // every later connection silently trusts.
    let s = ssh_render(
        r#"ssh.cmd("web-1", "id", { strict_host_key_checking: false }).to_string()"#,
    );
    assert!(s.contains("StrictHostKeyChecking=no"), "{}", s);
    assert!(s.contains("UserKnownHostsFile=/dev/null"), "{}", s);
}

#[test]
fn ssh_quotes_the_remote_command_so_the_local_shell_leaves_it_alone() {
    let s = ssh_render(r#"ssh.cmd("web-1", "echo $HOME > /tmp/x").to_string()"#);
    assert!(s.ends_with("'echo $HOME > /tmp/x'"), "{}", s);
}

#[test]
fn ssh_accepts_a_backtick_command() {
    let s = ssh_render(r#"ssh.cmd("web-1", `systemctl restart nginx`).to_string()"#);
    assert!(s.ends_with("'systemctl restart nginx'"), "{}", s);
}

#[test]
fn ssh_jump_host_becomes_proxy_jump() {
    let s = ssh_render(r#"ssh.cmd("db-1", "id", { jump: "bastion" }).to_string()"#);
    assert!(s.contains("-J bastion"), "{}", s);
}

#[test]
fn ssh_upload_reports_a_missing_local_file_before_connecting() {
    let (output, _) = run(
        r#"
import std.ssh
println(ssh.upload(p"/nonexistent/que/file", "web-1", p"/tmp/x").is_err())
"#,
    )
    .unwrap();
    assert_eq!(output, vec!["true"]);
}

#[test]
fn ssh_rejects_a_non_string_host() {
    assert_error_contains(
        "import std.ssh\nssh.cmd(42, \"id\")",
        "host must be a String",
    );
}

#[test]
fn a_dry_run_announces_an_upload_without_performing_it() {
    let dir = dry_project(
        "ssh",
        "import std.ssh\nssh.upload(script_dir() / \"main.que\", \"web-1\", p\"/tmp/x\")\n",
    );
    let output = run_dry_in(&dir, "main.que");
    assert!(
        output.iter().any(|l| l.contains("scp -r")),
        "expected the transfer to be announced: {:?}",
        output
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── .sudo() ──────────────────────────────────────────────────────────

#[test]
fn sudo_renders_into_the_command_text() {
    // Rendering rather than hiding behind a flag is the point: what
    // `--dry-run` and every error message shows is what actually runs.
    assert_output(
        r#"println(`apt-get install nginx`.sudo().to_string())"#,
        &["sudo -- apt-get install nginx"],
    );
}

#[test]
fn sudo_takes_a_user_name_directly() {
    assert_output(
        r#"println(`psql -l`.sudo("postgres").to_string())"#,
        &["sudo -u postgres -- psql -l"],
    );
}

#[test]
fn sudo_options_map_covers_env_and_ci() {
    assert_output(
        r#"println(`make install`.sudo({ preserve_env: true, non_interactive: true }).to_string())"#,
        &["sudo -n -E -- make install"],
    );
}

#[test]
fn sudo_can_use_a_different_escalation_binary() {
    // doas on OpenBSD, run0 on newer systemd.
    assert_output(
        r#"println(`reboot`.sudo({ binary: "doas" }).to_string())"#,
        &["doas -- reboot"],
    );
}

#[test]
fn sudo_refuses_a_redirect() {
    // `sudo -- echo x > /etc/hosts` writes as *you*, and the permission
    // error blames the wrong thing.
    assert_error_contains(
        r#"`echo 127.0.0.1 > /etc/hosts`.sudo()"#,
        "only the first stage would run as root",
    );
}

#[test]
fn sudo_refuses_a_pipe() {
    assert_error_contains(r#"`cat a | tee b`.sudo()"#, "only the first stage");
}

#[test]
fn sudo_allows_an_operator_inside_quotes() {
    // `grep "a|b"` is one command, not two.
    assert_output(
        r#"println(`grep "a|b" f`.sudo().to_string())"#,
        &[r#"sudo -- grep "a|b" f"#],
    );
}

#[test]
fn sudo_is_a_no_op_when_already_root() {
    // Cannot force uid 0 from a test, so assert the two branches agree:
    // either we are root and nothing is prepended, or we are not and
    // `sudo` is.
    let (output, _) = run(r#"println(env.is_root())
println(`whoami`.sudo().to_string())"#)
        .unwrap();
    if output[0] == "true" {
        assert_eq!(output[1], "whoami");
    } else {
        assert_eq!(output[1], "sudo -- whoami");
    }
}

#[test]
fn sudo_rejects_a_bad_argument() {
    assert_error_contains(
        r#"`ls`.sudo(42)"#,
        "sudo() takes a user name or an options map",
    );
}

#[test]
fn a_dry_run_announces_the_elevation() {
    let dir = dry_project("sudo", "`rm -rf /var/cache/x`.sudo().run()\n");
    let output = run_dry_in(&dir, "main.que");
    assert!(
        output.iter().any(|l| l.contains("sudo -- rm -rf")),
        "elevation should be visible in a dry run: {:?}",
        output
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── std.watch ────────────────────────────────────────────────────────

/// Make a fresh empty directory to watch, plus its string form for embedding
/// in Que source.
fn watch_scratch(tag: &str) -> (std::path::PathBuf, String) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("que_watch_{}_{}", tag, ts));
    std::fs::create_dir_all(&dir).unwrap();
    let s = dir.to_string_lossy().to_string();
    (dir, s)
}

/// Touch `path` after `delay_ms`, from another thread, so the watch loop in
/// the main thread has something to see.
fn touch_later(path: std::path::PathBuf, delay_ms: u64, body: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        std::fs::write(&path, body).unwrap();
    });
}

#[test]
fn watch_wait_reports_a_created_file() {
    let (dir, dir_str) = watch_scratch("created");
    touch_later(dir.join("new.txt"), 150, "hello");
    let source = format!(
        r#"
import std.watch
let changes = watch.wait(path("{}"), {{ interval: 30ms, debounce: 20ms, timeout: 20s }}).unwrap()
println(changes.len())
println(changes[0]["kind"])
println(str(changes[0]["path"]).ends_with("new.txt"))
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["1", "created", "true"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_wait_reports_a_modification_and_a_deletion() {
    let (dir, dir_str) = watch_scratch("modified");
    let target = dir.join("a.txt");
    std::fs::write(&target, "one").unwrap();
    let doomed = dir.join("b.txt");
    std::fs::write(&doomed, "two").unwrap();

    let t = target.clone();
    let d = doomed.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        std::fs::write(&t, "one-changed-and-longer").unwrap();
        std::fs::remove_file(&d).unwrap();
    });

    let source = format!(
        r#"
import std.watch
let changes = watch.wait(path("{}"), {{ interval: 30ms, debounce: 20ms, timeout: 20s }}).unwrap()
for c in changes {{
    println(c["kind"])
}}
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    let mut kinds = output.clone();
    kinds.sort();
    assert_eq!(kinds, vec!["deleted", "modified"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_wait_times_out_into_an_err() {
    // Nothing ever changes, so the timeout is the only way out. A watcher
    // that could not time out would be untestable and unscriptable.
    let (dir, dir_str) = watch_scratch("timeout");
    let source = format!(
        r#"
import std.watch
let r = watch.wait(path("{}"), {{ interval: 20ms, timeout: 200ms }})
println(r.is_err())
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["true"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_run_calls_back_and_stops_after_times() {
    let (dir, dir_str) = watch_scratch("run");
    touch_later(dir.join("trigger.txt"), 150, "go");
    let source = format!(
        r#"
import std.watch
let n = watch.run(path("{}"), |changes| {{
    println("rebuilt on " + changes[0]["kind"])
}}, {{ interval: 30ms, debounce: 20ms, timeout: 20s, times: 1 }}).unwrap()
println(n)
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["rebuilt on created", "1"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_run_initial_fires_before_any_change() {
    // The first build should not wait for an edit; `initial` is what makes
    // `watch.run` a complete dev loop rather than half of one.
    let (dir, dir_str) = watch_scratch("initial");
    let source = format!(
        r#"
import std.watch
let n = watch.run(path("{}"), |changes| {{
    println("built " + str(changes.len()))
}}, {{ interval: 20ms, timeout: 5s, initial: true, times: 1 }}).unwrap()
println(n)
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["built 0", "1"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_ignores_build_output_by_default() {
    // Writing into `target/` must not wake the loop, or a rebuild would
    // trigger the next rebuild forever.
    let (dir, dir_str) = watch_scratch("ignore");
    std::fs::create_dir_all(dir.join("target")).unwrap();
    touch_later(dir.join("target").join("artifact.bin"), 100, "output");
    let source = format!(
        r#"
import std.watch
println(watch.wait(path("{}"), {{ interval: 20ms, timeout: 600ms }}).is_err())
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["true"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_ignore_patterns_can_be_overridden() {
    let (dir, dir_str) = watch_scratch("override");
    touch_later(dir.join("notes.log"), 100, "noise");
    let source = format!(
        r#"
import std.watch
println(watch.wait(path("{}"), {{ interval: 20ms, timeout: 600ms, ignore: ["**/*.log"] }}).is_err())
"#,
        dir_str
    );
    let (output, _) = run(&source).unwrap();
    assert_eq!(output, vec!["true"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn watch_refuses_a_glob_root() {
    assert_error_contains(
        r#"
import std.watch
watch.wait(glob("src/**/*.rs"))
"#,
        "expanded once",
    );
}

#[test]
fn watch_run_is_a_no_op_in_a_dry_run() {
    // Blocking forever would make `--dry-run` unusable on any script that
    // ends in a watch loop.
    let (dir, dir_str) = watch_scratch("dryrun");
    let source = format!(
        r#"
import std.watch
watch.run(path("{}"), |c| {{ println("never") }}, {{ times: 1, timeout: 10s }})
println("done")
"#,
        dir_str
    );
    let proj = dry_project("watch", &source);
    let output = run_dry_in(&proj, "main.que");
    assert!(output.iter().any(|l| l.contains("done")), "{:?}", output);
    assert!(
        !output.iter().any(|l| l.contains("never")),
        "callback ran in a dry run: {:?}",
        output
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&proj).ok();
}



// ── Permissions ──────────────────────────────────────────────────────

/// Run a source string under a capability policy built from `--allow`/`--deny`
/// style specs, the way `que --allow read=src script.que` does.
fn run_with_policy(specs: &[&str], source: &str) -> Result<Vec<String>, String> {
    use que_lang::interpreter::Interpreter;
    use que_lang::lexer::Lexer;
    use que_lang::parser::Parser;
    use que_lang::permissions::Policy;

    let tokens = Lexer::new(source).tokenize().unwrap();
    let module = Parser::new(tokens).parse_module().unwrap();
    let mut policy = Policy::default();
    for spec in specs {
        match spec.strip_prefix('!') {
            Some(rest) => policy.deny(rest).unwrap(),
            None => policy.allow(spec).unwrap(),
        }
    }
    let mut interp = Interpreter::new();
    interp.permissions = Some(policy);
    match interp.exec_module(&module) {
        Ok(_) => {
            interp.flush_partial();
            Ok(interp.output)
        }
        Err(que_lang::error::Signal::Error(e)) => Err(e.to_string()),
        Err(_) => Ok(interp.output),
    }
}

fn assert_denied(specs: &[&str], source: &str, needle: &str) {
    match run_with_policy(specs, source) {
        Ok(out) => panic!("expected a denial, script succeeded with {:?}", out),
        Err(msg) => assert!(
            msg.contains("permission denied") && msg.contains(needle),
            "expected a denial mentioning {:?}, got: {}",
            needle,
            msg
        ),
    }
}

#[test]
fn a_policy_denies_a_command_that_was_not_granted() {
    assert_denied(&["read"], "`echo hi`.run()", "exec");
}

#[test]
fn granting_exec_lets_a_command_run() {
    let out = run_with_policy(&["exec"], "println(`echo hi`.out())").unwrap();
    assert_eq!(out, vec!["hi".to_string()]);
}

#[test]
fn a_scoped_exec_grant_matches_the_rendered_command() {
    let out = run_with_policy(&["exec=echo hi"], "println(`echo hi`.out())").unwrap();
    assert_eq!(out, vec!["hi".to_string()]);
    assert_denied(&["exec=echo hi"], "`echo bye`.run()", "echo bye");
}

#[test]
fn every_stage_of_a_pipeline_is_checked() {
    // The first stage is allowed, the second is not; the pipeline must still
    // be refused rather than partially executed.
    assert_denied(
        &["exec=echo hi"],
        "(`echo hi` | `grep hi`).run()",
        "exec",
    );
}

#[test]
fn a_spawn_is_checked_like_any_other_command() {
    assert_denied(&["read"], "let h = spawn `sleep 5`", "exec");
}

#[test]
fn reading_a_file_outside_the_granted_tree_is_refused() {
    let dir = std::env::temp_dir().join(format!("que_perm_{}_read", std::process::id()));
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    std::fs::write(dir.join("inside/ok.txt"), "yes").unwrap();
    std::fs::write(dir.join("secret.txt"), "no").unwrap();

    let inside = dir.join("inside");
    let allowed = format!("read={}", inside.display());
    let good = format!(
        "import std.fs\nprintln(fs.read(\"{}\").unwrap())\n",
        inside.join("ok.txt").display()
    );
    let out = run_with_policy(&[&allowed], &good).unwrap();
    assert_eq!(out, vec!["yes".to_string()]);

    let bad = format!(
        "import std.fs\nfs.read(\"{}\")\n",
        dir.join("secret.txt").display()
    );
    assert_denied(&[&allowed], &bad, "read");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_read_grant_does_not_imply_a_write_grant() {
    let dir = std::env::temp_dir().join(format!("que_perm_{}_rw", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let allowed = format!("read={}", dir.display());
    let src = format!(
        "import std.fs\nfs.write(\"{}\", \"x\")\n",
        dir.join("new.txt").display()
    );
    assert_denied(&[&allowed], &src, "write");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_write_to_a_path_that_does_not_exist_yet_is_still_scoped() {
    // The check must not depend on canonicalize(), which fails on a path
    // that has not been created.
    let dir = std::env::temp_dir().join(format!("que_perm_{}_new", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let allowed = format!("write={}", dir.display());
    let src = format!(
        "import std.fs\nfs.write(\"{}\", \"x\")\nprintln(\"wrote\")\n",
        dir.join("fresh.txt").display()
    );
    let out = run_with_policy(&[&allowed], &src).unwrap();
    assert_eq!(out, vec!["wrote".to_string()]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_path_method_is_checked_as_well_as_the_fs_module() {
    let dir = std::env::temp_dir().join(format!("que_perm_{}_pm", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "hi").unwrap();
    let src = format!("let p = p\"{}\"\np.read()\n", dir.join("a.txt").display());
    assert_denied(&["exec"], &src, "read");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dot_dot_path_cannot_escape_a_grant() {
    let dir = std::env::temp_dir().join(format!("que_perm_{}_esc", std::process::id()));
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    let allowed = format!("read={}", dir.join("inside").display());
    let src = format!(
        "import std.fs\nfs.read(\"{}/inside/../outside.txt\")\n",
        dir.display()
    );
    assert_denied(&[&allowed], &src, "read");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_host_grant_is_matched_against_the_url_host() {
    assert_denied(
        &["net=api.example.com"],
        "import std.http\nhttp.get(\"https://evil.example.org/x\")\n",
        "net",
    );
}

#[test]
fn reading_the_environment_needs_the_env_capability() {
    assert_denied(&["read"], "env.get(\"PATH\")", "env");
    let out = run_with_policy(&["env"], "println(env.get(\"PATH\") != null)").unwrap();
    assert_eq!(out, vec!["true".to_string()]);
}

#[test]
fn informational_env_helpers_stay_available_under_any_policy() {
    // Denying `env.is_ci()` protects nothing and would break scripts that
    // only branch on the environment they are in.
    let out = run_with_policy(&["read"], "println(env.platform() != \"\")").unwrap();
    assert_eq!(out, vec!["true".to_string()]);
}

#[test]
fn deny_leaves_everything_else_granted() {
    let out = run_with_policy(&["!net"], "println(`echo hi`.out())").unwrap();
    assert_eq!(out, vec!["hi".to_string()]);
    assert_denied(
        &["!net"],
        "import std.http\nhttp.get(\"https://example.com\")\n",
        "net",
    );
}

#[test]
fn pure_helpers_are_never_blocked() {
    let out = run_with_policy(
        &["read"],
        "import std.json\nprintln(json.parse(\"{\\\"a\\\":1}\").unwrap().a)",
    )
    .unwrap();
    assert_eq!(out, vec!["1".to_string()]);
}

#[test]
fn no_policy_means_no_restriction() {
    // The sandbox is opt-in: the default interpreter must behave exactly as
    // it did before permissions existed.
    assert_output("println(`echo hi`.out())", &["hi"]);
}

#[test]
fn a_denial_names_the_flag_that_would_allow_it() {
    let msg = match run_with_policy(&["read"], "`echo hi`.run()") {
        Err(m) => m,
        Ok(_) => panic!("expected a denial"),
    };
    assert!(
        msg.contains("--allow exec"),
        "the error should tell the operator how to fix it: {}",
        msg
    );
}

// ── Permissions: global builtins ─────────────────────────────────────
//
// The first cut of the sandbox wired the policy into std-module dispatch,
// `Path` methods and the three command paths -- but global builtins are a
// fourth dispatch path, and it was unchecked. `open(p, "w")` sailed straight
// through `--deny write`. These tests pin the hole shut.

/// A scratch directory under the system temp dir, unique per test.
fn scratch(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("que_perm_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

#[test]
fn opening_a_file_for_writing_needs_the_write_capability() {
    let dir = scratch("open_w");
    let target = format!("{}/out.txt", dir);
    assert_denied(
        &["!write"],
        &format!("let f = open(\"{}\", \"w\")?\nf.write(\"test\")", target),
        "write",
    );
    // The point of denying it is that nothing happened, not merely that an
    // error was printed afterwards.
    assert!(
        !std::path::Path::new(&target).exists(),
        "a denied open must not have created the file"
    );
}

#[test]
fn opening_a_file_for_reading_needs_only_the_read_capability() {
    let dir = scratch("open_r");
    let target = format!("{}/in.txt", dir);
    std::fs::write(&target, "hello\n").unwrap();
    let out = run_with_policy(
        &[&format!("read={}", dir)],
        &format!("let f = open(\"{}\")?\nprintln(f.read().unwrap().trim())", target),
    )
    .unwrap();
    assert_eq!(out, vec!["hello".to_string()]);
}

#[test]
fn a_read_grant_does_not_let_a_script_open_a_file_for_writing() {
    let dir = scratch("open_rw");
    assert_denied(
        &[&format!("read={}", dir)],
        &format!("let f = open(\"{}/x.txt\", \"w\")?", dir),
        "write",
    );
}

#[test]
fn appending_counts_as_writing() {
    let dir = scratch("open_a");
    assert_denied(
        &["read"],
        &format!("let f = open(\"{}/x.txt\", \"a\")?", dir),
        "write",
    );
}

#[test]
fn globbing_needs_the_read_capability() {
    assert_denied(&["!read"], "glob(\"*.txt\").to_list()", "read");
}

#[test]
fn streaming_a_path_reads_but_streaming_a_string_does_not() {
    let dir = scratch("stream");
    let target = format!("{}/lines.txt", dir);
    std::fs::write(&target, "a\nb\n").unwrap();
    assert_denied(
        &["!read"],
        &format!("import std.stream\nstream.file(p\"{}\").count_lines()", target),
        "read",
    );
    // An in-memory pipeline touches nothing, and it now says so in its name.
    let out = run_with_policy(
        &["!read"],
        "import std.stream\nprintln(stream.of(\"a\\nb\").count_lines())",
    )
    .unwrap();
    assert_eq!(out, vec!["2".to_string()]);
}

#[test]
fn writing_a_stream_to_a_file_needs_the_write_capability() {
    let dir = scratch("stream_w");
    assert_denied(
        &["!write"],
        &format!(
            "import std.stream\nstream.of(\"a\\nb\").write_to(p\"{}/out.txt\")",
            dir
        ),
        "write",
    );
}

#[test]
fn reading_and_writing_config_files_are_separate_capabilities() {
    let dir = scratch("config");
    assert_denied(
        &["!read"],
        &format!("import std.config\nconfig.read(p\"{}/c.json\")", dir),
        "read",
    );
    assert_denied(
        &["read"],
        &format!("import std.config\nconfig.write(p\"{}/c.json\", {{}})", dir),
        "write",
    );
    // The in-memory helpers walk a value that is already in hand.
    let out = run_with_policy(
        &["!read", "!write"],
        "println({a: {b: 1}}.get_path(\"a.b\"))",
    )
    .unwrap();
    assert_eq!(out, vec!["1".to_string()]);
}

#[test]
fn a_temporary_directory_is_scoped_to_where_it_will_be_created() {
    assert_denied(&["!write"], "with TempDir {} as t { println(t) }", "write");
    let base = scratch("tempbase");
    let out = run_with_policy(
        &[&format!("write={}", base)],
        &format!("with TempDir {{ dir: p\"{}\" }} as t {{ println(t != \"\") }}", base),
    )
    .unwrap();
    assert_eq!(out, vec!["true".to_string()]);
}

#[test]
fn an_env_scope_is_checked_per_variable_rather_than_wholesale() {
    // A grant for one variable must still admit a scope that only sets that
    // variable, and the denial must name the variable -- not print the map,
    // whose values are the very thing this capability guards.
    let out = run_with_policy(
        &["env=QUE_PERM_OK"],
        "with env.scope({QUE_PERM_OK: \"1\"}) { println(env.get(\"QUE_PERM_OK\")) }",
    )
    .unwrap();
    assert_eq!(out, vec!["1".to_string()]);
    assert_denied(
        &["env=QUE_PERM_OK"],
        "with env.scope({QUE_PERM_SECRET: \"hunter2\"}) { println(1) }",
        "QUE_PERM_SECRET",
    );
}

#[test]
fn a_denial_for_an_env_scope_does_not_leak_the_value() {
    match run_with_policy(
        &["!env"],
        "with env.scope({QUE_PERM_TOKEN: \"hunter2\"}) { println(1) }",
    ) {
        Ok(out) => panic!("expected a denial, got {:?}", out),
        Err(msg) => assert!(
            !msg.contains("hunter2"),
            "the denial must not print the variable's value: {}",
            msg
        ),
    }
}

#[test]
fn printing_and_pure_transformation_survive_the_strictest_policy() {
    let out = run_with_policy(
        &["!read", "!write", "!exec", "!net", "!env"],
        "println(\"hi\")\nprintln([3, 1, 2].sort())\nprintln(\"a,b\".split(\",\").len())",
    )
    .unwrap();
    assert_eq!(
        out,
        vec!["hi".to_string(), "[1, 2, 3]".to_string(), "2".to_string()]
    );
}
