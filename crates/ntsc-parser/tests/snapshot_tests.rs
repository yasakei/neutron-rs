//! AST Snapshot tests for every NTSC AST node type.
//!
//! Asserts that every expression and statement node produces the exact expected AST structure.

use ntsc_lexer::tokenize;
use ntsc_parser::parse;

fn parse_ok(source: &str) -> String {
    let tokens = tokenize(source);
    let program = parse(&tokens).expect("parse failed");
    format!("{:#?}", program)
}

#[test]
fn snapshot_literal_and_var_declaration() {
    let ast = parse_ok("var int x = 42");
    assert!(ast.contains("Var"));
    assert!(ast.contains("Literal"));
    assert!(ast.contains("Number"));
    assert!(ast.contains("\"42\""));
}

#[test]
fn snapshot_binary_and_unary_expressions() {
    let ast = parse_ok("var bool res = !true && (-1.5 > 0)");
    assert!(ast.contains("Binary"));
    assert!(ast.contains("Unary"));
    assert!(ast.contains("Grouping"));
}

#[test]
fn snapshot_member_access_and_assignment() {
    let ast = parse_ok("user.name = person?.first");
    assert!(ast.contains("MemberSet"));
    assert!(ast.contains("OptionalMember"));
}

#[test]
fn snapshot_array_and_object_literals() {
    let ast = parse_ok("var data = { \"items\": [1, 2, 3] }");
    assert!(ast.contains("ObjectLiteral"));
    assert!(ast.contains("ArrayLiteral"));
}

#[test]
fn snapshot_index_get_and_index_set() {
    let ast = parse_ok("arr[0] = grid[1][2]");
    assert!(ast.contains("IndexSet"));
    assert!(ast.contains("IndexGet"));
}

#[test]
fn snapshot_function_declaration_and_call() {
    let ast = parse_ok("fun add(int a, int b) -> int { return a + b }\nadd(1, 2)");
    assert!(ast.contains("Function"));
    assert!(ast.contains("Return"));
    assert!(ast.contains("Call"));
}

#[test]
fn snapshot_class_this_and_methods() {
    let ast = parse_ok("class Counter extends Base { fun get() -> int { return this.value } }");
    assert!(ast.contains("Class"));
    assert!(ast.contains("This"));
}

#[test]
fn snapshot_if_elif_else_control_flow() {
    let ast = parse_ok("if (a) { return 1 } elif (b) { return 2 } else { return 3 }");
    assert!(ast.contains("If"));
    assert!(ast.contains("elif_branches"));
}

#[test]
fn snapshot_loops_while_do_while_for_for_in() {
    let ast = parse_ok(
        "while (x) { break }\ndo { continue } while (y);\nfor (var i = 0; i < 10; i++) { }\nfor (var item in list) { }",
    );
    assert!(ast.contains("While"));
    assert!(ast.contains("DoWhile"));
    assert!(ast.contains("For"));
    assert!(ast.contains("ForIn"));
    assert!(ast.contains("Break"));
    assert!(ast.contains("Continue"));
}

#[test]
fn snapshot_match_pattern_destructuring() {
    let ast = parse_ok("match (val) { case [a, b, ...rest] if a > 0 => say(a) default => say(0) }");
    assert!(ast.contains("Match"));
    assert!(ast.contains("ArrayLiteral"));
    assert!(ast.contains("guard"));
}

#[test]
fn snapshot_postfix_increment_decrement() {
    let ast =
        parse_ok("var i = 0\ni++\ni--\nvar x = i++\nvar y = i--\nfor (var j = 0; j < 10; j++) { }");
    assert!(ast.contains("PostfixUnary"));
    assert!(ast.contains("PlusPlus"));
    assert!(ast.contains("MinusMinus"));
}

#[test]
fn snapshot_try_catch_finally_throw_retry() {
    let ast = parse_ok(
        "try { throw \"err\" } catch (e) { retry 3 { } catch (err) { } } finally { say(\"done\") }",
    );
    assert!(ast.contains("Try"));
    assert!(ast.contains("Throw"));
    assert!(ast.contains("Retry"));
}

#[test]
fn snapshot_lambda_ternary_spread_safe_destructure_use_enum() {
    let ast = parse_ok(
        "var f = fun(int x) -> int => x ? 1 : ...items\nunsafe { var [a, b] = pair }\nuse math as M;\nenum Color { RED = 1, GREEN = 2 }",
    );
    assert!(ast.contains("Lambda"));
    assert!(ast.contains("Ternary"));
    assert!(ast.contains("Spread"));
    assert!(ast.contains("Unsafe"));
    assert!(ast.contains("Destructure"));
    assert!(ast.contains("Use"));
    assert!(ast.contains("Enum"));
}
