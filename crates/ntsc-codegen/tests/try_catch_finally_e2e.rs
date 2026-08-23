//! End-to-end regression test for `try`/`catch`/`finally` control flow.
//!
//! Coverage:
//! - `try`/`catch`/`finally` where the `catch` handles a thrown exception and
//!   the `finally` block runs afterwards.
//! - `try`/`finally` (no `catch`): the exception propagates out of the
//!   function *after* the `finally` block runs.
//! - `try`/`catch`/`finally` where no exception is thrown.
//!
//! Regression: a `finally` block with a present `catch` used to panic the
//! code generator ("rethrow block present"); the rethrow block is required
//! whenever a `finally` must propagate an active exception, i.e. when there is
//! no `catch` or when a `finally` follows one.

use std::path::Path;
#[test]
fn try_catch_finally_e2e() {
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

    let source = r#"fun propagate() -> int {
    try {
        throw "boom"
    } finally {
        say("f1: finally before propagate")
    }
}

fun main() {
    try {
        say("c0: try")
        throw "failure"
    } catch (err) {
        say("c1: caught " + err)
    } finally {
        say("c2: finally after catch")
    }

    try {
        say("c3: no error")
    } catch (err) {
        say("unexpected " + err)
    } finally {
        say("c4: finally, nothing thrown")
    }

    try {
        var r = propagate()
        say("unreachable " + r)
    } catch (err) {
        say("c5: propagated " + err)
    }

    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_try_catch_finally_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!(
        "try_catch_finally.{}",
        ntsc_codegen::object_extension()
    ));
    let bin_path = dir.join("try_catch_finally_ntsc_test");

    ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "try_catch_finally",
        &dir,
    )
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
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let expected_lines = [
        "c0: try",
        "c1: caught failure",
        "c2: finally after catch",
        "c3: no error",
        "c4: finally, nothing thrown",
        "f1: finally before propagate",
        "c5: propagated boom",
    ];
    for line in expected_lines {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing output line {line:?}:\n{stdout}"
        );
    }

    assert!(
        stdout.trim().ends_with("done"),
        "unexpected output:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: `retry count` used to lower to a plain block, so a throwing body
/// propagated on the first attempt and the `catch` variable was never bound
/// (a compile error "undefined variable"). The loop now retries while attempts
/// remain, binds the last exception message in `catch`, and re-throws outward
/// when there is no `catch`.
#[test]
fn retry_e2e() {
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

    let source = r#"fun flaky(int n) {
    if (n < 3) {
        throw "boom " + n
    }
}

fun main() {
    var attempts = 0;
    retry 5 {
        attempts = attempts + 1;
        flaky(attempts);
    } catch (err) {
        say("unexpected: " + err)
    }
    say("r1: attempts " + attempts)

    var retries = 0;
    retry 3 {
        retries = retries + 1;
        throw "always";
    } catch (err) {
        say("r2: caught " + err)
    }
    say("r3: retries " + retries)

    try {
        retry 2 {
            throw "inner";
        }
    } catch (err) {
        say("r4: rethrown " + err)
    }
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_retry_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("retry.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("retry_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "retry", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let expected_lines = [
        "r1: attempts 3",
        "r2: caught always",
        "r3: retries 3",
        "r4: rethrown inner",
        "done",
    ];
    for line in expected_lines {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing output line {line:?}:\n{stdout}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
