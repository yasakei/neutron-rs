//! End-to-end regression test for enums (lowered to int constants) and
//! `match` (literal / enum / string cases, `_` wildcard, `default`, guards).
//!
//! Regression: enums used to be skipped entirely, so `Color.RED` failed with
//! "undefined variable `Color`" and bare `case North` / `case _` were
//! undefined-name errors. Enums now register members as int constants and
//! `match` handles scalar equality, the `_` wildcard, `default`, and guards.

use std::path::Path;

fn run_program(source: &str) -> String {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let runtime_lib = rewrite_dir.join(
        Path::new("target")
            .join("debug")
            .join(ntsc_codegen::runtime_lib_name()),
    );

    if !runtime_lib.exists() {
        let status = std::process::Command::new("cargo")
            .args(["build", "-p", "ntsc-runtime"])
            .current_dir(rewrite_dir)
            .status()
            .expect("failed to run cargo");
        assert!(status.success(), "failed to build ntsc-runtime");
    }
    assert!(
        runtime_lib.exists(),
        "runtime lib not found at {runtime_lib:?}"
    );

    let dir = std::env::temp_dir().join("ntsc_enum_match_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("enummatch.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("enummatch_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "enummatch", &dir)
        .expect("compile_source failed");

    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");

    assert!(
        output.status.success(),
        "binary exited with non-zero status"
    );

    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn enum_and_match_e2e() {
    let source = r#"enum Direction {
    North,
    South,
    East = 8,
    West,
}
enum Color {
    RED = 1,
    GREEN,
    BLUE,
}

fun classify(int n) -> string {
    match (n) {
        case 0 => return "zero";
        case 1 => return "one";
        case _ => return "many";
    }
    return "?";
}
fun compass(int dir) -> string {
    match (dir) {
        case North => return "up";
        case South => return "down";
        default => return "sideways";
    }
    return "?";
}
fun mood(string s) -> string {
    match (s) {
        case "happy" => return "nice";
        default => return "meh";
    }
    return "?";
}
fun gated(int x) -> string {
    match (x) {
        case 10 if x > 5 => return "big ten";
        case _ => return "other";
    }
    return "?";
}

fun main() {
    say("red: " + Color.RED);
    say("green: " + Color.GREEN);
    say("blue: " + Color.BLUE);
    say("north: " + Direction.North);
    say("east: " + Direction.East);
    say("west: " + Direction.West);
    say("bare north: " + North);
    say("classify0: " + classify(0));
    say("classify5: " + classify(5));
    say("north: " + compass(Direction.North));
    say("east: " + compass(Direction.East));
    say("happy: " + mood("happy"));
    say("tired: " + mood("tired"));
    say("gated: " + gated(10));
    say("done");
}
"#;

    let stdout = run_program(source);

    let expected_lines = [
        "red: 1",
        "green: 2",
        "blue: 3",
        "north: 0",
        "east: 8",
        "west: 9",
        "bare north: 0",
        "classify0: zero",
        "classify5: many",
        "north: up",
        "east: sideways",
        "happy: nice",
        "tired: meh",
        "gated: big ten",
        "done",
    ];
    for line in expected_lines {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing output line {line:?}:\n{stdout}"
        );
    }
}
