//! End-to-end diagnostics tests: real Neutron sources flow through
//! resolution, type checking, and linting, then render through `ntsc-diag`
//! with suggestions, fix-it hints, and lint names intact.

use ntsc_ast::stmt::Program;
use ntsc_diag::{
    DiagConfig, Diagnostic, EmitMode, SourceBuffer, SourceMap, Writer, diagnostics_to_json,
};
use ntsc_lexer::tokenize;
use ntsc_parser::parse;
use ntsc_typeck::{check_program, lint_program, resolve_program};

fn parse_ok(source: &str) -> Program {
    parse(&tokenize(source)).unwrap_or_else(|errors| {
        panic!(
            "test source failed to parse: {:?}",
            errors.first().map(|e| &e.message)
        )
    })
}

fn plain_writer() -> Writer {
    Writer::new(DiagConfig {
        mode: EmitMode::Plain,
        max_errors: None,
    })
}

fn rendered_with_source(diags: &[Diagnostic], source: &str) -> String {
    let mut map = SourceMap::new();
    map.add(SourceBuffer::new(source, "test.nt"));
    let writer = plain_writer();
    diags
        .iter()
        .map(|diag| {
            let diag = diag.clone().with_file("test.nt");
            writer.render(&diag, Some(&map))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn undefined_variable_typo_gets_did_you_mean_suggestion() {
    let source = r#"
fun main() -> int {
    var int count = 3
    return coutn
}
"#;
    let errors = resolve_program(&parse_ok(source)).expect_err("typo should be rejected");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].suggestion.as_deref(), Some("count"));

    let diags: Vec<Diagnostic> = errors.iter().map(Diagnostic::from).collect();
    let text = rendered_with_source(&diags, source);
    assert!(text.contains("did you mean `count`?"), "{text}");
}

#[test]
fn struct_literal_typo_field_gets_did_you_mean_suggestion() {
    let source = r#"
class Rect {
    var int width
    var int height
}

fun main() -> int {
    var Rect r = Rect { widht: 3, height: 4 }
    return r.width + r.height
}
"#;
    let program = parse_ok(source);
    // Name resolution must pass so the field typo reaches the type checker.
    resolve_program(&program).expect("names should resolve");
    let errors = check_program(&program).expect_err("misspelled field should be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.help.as_deref() == Some("did you mean `width`?")),
        "{errors:?}"
    );
}

#[test]
fn slices_function_typo_gets_did_you_mean_suggestion() {
    let source = r#"
fun main() -> int {
    var array[int] data = [1, 2, 3]
    say("" + slices.lenght(data))
    return 0
}
"#;
    let program = parse_ok(source);
    let _ = resolve_program(&program);
    let errors = check_program(&program).expect_err("unknown slices fn should be rejected");
    assert!(
        errors.iter().any(
            |error| error.message.contains("unknown function `slices.lenght`")
                && error.help.as_deref() == Some("did you mean `length`?")
        ),
        "{errors:?}"
    );
}

#[test]
fn unused_variable_warning_carries_lint_name_and_quiet_help() {
    let source = r#"
fun main() -> int {
    var int spare = 7
    return 0
}
"#;
    let warnings = lint_program(&parse_ok(source));
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].lint, "unused_variable");

    let diag = Diagnostic::from(&warnings[0]);
    assert_eq!(diag.lint.as_deref(), Some("unused_variable"));
    assert_eq!(
        diag.help.as_deref(),
        Some("silence locally with `quiet [unused_variable] { ... }`")
    );

    let json = diagnostics_to_json(&[diag]);
    assert!(json.contains("\"lint\":\"unused_variable\""), "{json}");

    let text = rendered_with_source(
        &[Diagnostic::from(&lint_program(&parse_ok(source))[0])],
        source,
    );
    assert!(
        text.contains("warning[NTSC-W0001]: unused variable `spare`"),
        "{text}"
    );
    assert!(text.contains("quiet [unused_variable] { ... }"), "{text}");
}
