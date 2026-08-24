//! Golden semantic tests for typed Neutron source programs.

use ntsc_lexer::tokenize;
use ntsc_parser::parse;
use ntsc_typeck::check_program;

fn check(source: &str) -> Result<(), Vec<String>> {
    let program = parse(&tokenize(source)).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
    })?;
    check_program(&program)
        .map_err(|errors| errors.into_iter().map(|error| error.message).collect())
}

#[test]
fn typed_fibonacci_program_is_accepted() {
    let source = r#"
fun fibonacci(int number) -> int {
    if (number <= 1) { return number }
    return fibonacci(number - 1) + fibonacci(number - 2)
}

fun main() -> int {
    var int total = fibonacci(8)
    return total
}
"#;
    assert!(check(source).is_ok());
}

#[test]
fn typed_class_signature_and_explicit_option_are_accepted() {
    let source = r#"
class Person { }

fun keep(Person person) -> option[Person] {
    return nil
}

fun main() -> int {
    var option[Person] owner = nil
    return 0
}
"#;
    assert!(check(source).is_ok());
}

#[test]
fn option_and_nil_comparisons_yield_bool() {
    let source = r#"
fun main() -> int {
    var option[int] maybe = nil
    var option[int] other = nil
    var bool a = maybe == nil
    var bool b = maybe != nil
    var bool c = nil == nil
    var bool d = nil != nil
    var bool e = nil == maybe
    var bool f = maybe == other
    var bool g = maybe != other
    if (maybe == nil) {
        return 0
    }
    return 1
}
"#;
    assert!(check(source).is_ok());
}

#[test]
fn golden_diagnostics_cover_name_type_and_call_errors() {
    let source = r#"
fun add(int left, int right) -> int { return left + right }
fun main() -> int {
    var array[int] values = [1, "two"]
    return add(missing, 2)
}
"#;
    let errors = check(source).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.contains("undefined name `missing`"))
    );
}
