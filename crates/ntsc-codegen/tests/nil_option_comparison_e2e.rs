//! End-to-end regression test for `nil` / `option[T]` equality.
//!
//! Regression: `maybe == nil` for a `var option[int] maybe = nil` was resolved
//! to `Ty::Any` by the type checker (no unification rule) and emitted by
//! codegen as an untyped null pointer, so `say("" + (maybe == nil))` printed an
//! empty string instead of `true`. `nil == nil` had the same failure. Equality
//! is now an address comparison: `option == nil` is a nullness test and
//! `option == option` is an identity test.

use std::path::Path;
#[test]
fn nil_option_comparison_e2e() {
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
    var option[int] maybe = nil
    var option[int] other = nil
    say("" + (maybe == nil))
    say("" + (maybe != nil))
    say("" + (nil == nil))
    say("" + (nil != nil))
    say("" + (nil == maybe))
    say("" + (maybe == other))
    say("" + (maybe != other))
    if (maybe == nil) {
        say("is nil")
    }
}
"#;

    let dir = std::env::temp_dir().join("ntsc_nil_option_cmp_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("nilcmp.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("nilcmp_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "nilcmp", &dir)
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
    assert_eq!(
        stdout.trim(),
        "true\nfalse\ntrue\nfalse\ntrue\ntrue\nfalse\nis nil"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
