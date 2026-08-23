//! End-to-end regression test for `test` blocks and the `compile_tests`
//! (test-mode) pipeline: `test name { ... }` bodies are compiled to
//! `test_<name>` functions and a generated harness `main` runs each one,
//! printing `PASS`/`FAIL` lines, a summary, and a non-zero exit code when any
//! test throws.

use std::path::Path;
fn runtime_lib() -> std::path::PathBuf {
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
    runtime_lib
}

fn build_and_run(
    source: &str,
    dir: &std::path::Path,
    runtime_lib: &std::path::Path,
) -> std::process::Output {
    let obj_path = dir.join(format!("test_blocks.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("test_blocks_bin");

    ntsc_codegen::compile_tests(source, ntsc_codegen::host_triple(), "test_blocks", dir)
        .expect("compile_tests failed");
    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, runtime_lib, &bin_path).expect("link_binary failed");
    assert!(bin_path.exists(), "binary not produced");

    std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary")
}

#[test]
fn test_blocks_all_pass() {
    let source = r#"use testing

fun add(int a, int b) -> int {
    return a + b;
}

test add_works {
    testing.assert_eq(add(2, 3), 5);
    testing.assert_ne(add(2, 3), 6);
}

test strings_compare {
    testing.assert_eq("hello", "hello");
}

fun main() {
    say("user main must not run in test mode");
}
"#;

    let dir = std::env::temp_dir().join("ntsc_test_blocks_e2e_pass");
    std::fs::create_dir_all(&dir).unwrap();

    let output = build_and_run(source, &dir, &runtime_lib());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert!(
        output.status.success(),
        "expected exit code 0, got {:?}:\n{stdout}",
        output.status.code()
    );
    for line in [
        "PASS add_works".to_string(),
        "PASS strings_compare".to_string(),
        format!("{} 2 passed, 0 failed", ntsc_codegen::SUMMARY_MARKER),
    ] {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing line {line:?}:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("user main must not run"),
        "user main ran in test mode:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_blocks_failure_throws_and_fails() {
    let source = r#"use testing

test passing_one {
    testing.assert_eq(1, 1);
}

test failing_one {
    testing.assert_true(false);
}
"#;

    let dir = std::env::temp_dir().join("ntsc_test_blocks_e2e_fail");
    std::fs::create_dir_all(&dir).unwrap();

    let output = build_and_run(source, &dir, &runtime_lib());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert!(
        !output.status.success(),
        "expected non-zero exit code, got success:\n{stdout}"
    );
    assert!(
        stdout.lines().any(|l| l == "PASS passing_one"),
        "missing PASS line:\n{stdout}"
    );
    assert!(
        stdout.lines().any(|l| l.starts_with("FAIL failing_one: ")),
        "missing FAIL line:\n{stdout}"
    );
    assert!(
        stdout.contains("testing.assert_true"),
        "FAIL line should include the thrown message:\n{stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l == format!("{} 1 passed, 1 failed", ntsc_codegen::SUMMARY_MARKER)),
        "missing summary line:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
