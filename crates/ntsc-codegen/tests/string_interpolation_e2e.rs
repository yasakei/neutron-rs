//! End-to-end regression test for string interpolation that begins with an
//! expression, e.g. `"${n}"` or `"${n}!"`.
//!
//! Regression: an interpolated string whose first character is `${` was lexed
//! so that the first token was the expression token rather than a
//! `StringSegment`, so the parser did not enter `parse_string_with_interpolation`.
//! `say("${n}")` was parsed as a bare `n` and rejected ("say expects a string"),
//! and `say("${n}!")` failed with a parse error.

use std::path::Path;
#[test]
fn string_interpolation_leading_expression_e2e() {
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

    let source = r#"fun main() {
    var n = 42
    var fs = [2.5, 3.5]
    say("${n}")
    say("${n}!")
    say("${n}${n}")
    say("${fs[1]}")
    say("value: ${n}")
    say("Hello, ${n}!")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_string_interp_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("interp.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("interp_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "interp", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "42\n42!\n4242\n3.5\nvalue: 42\nHello, 42!");

    let _ = std::fs::remove_dir_all(&dir);
}
