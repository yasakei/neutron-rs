//! End-to-end regression test for string comparison.
//!
//! Regression: `==` / `!=` on two `string` operands passed the operands to
//! `ntsc_string_equals` as pointers, but a string is a registry handle in an
//! `i64`, so comparing two string variables crashed the compiler ("Found
//! IntValue ... but expected PointerValue variant"). `!=` also returned the
//! equality result unnegated. Comparing through a `view string` fell into the
//! `Ty::Any` fallback and produced a value of the wrong LLVM type, which failed
//! module verification.

use std::path::Path;

/// Compile + link + run `source`, returning its stdout.
fn run(name: &str, source: &str) -> String {
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
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

    let dir = std::env::temp_dir().join(format!("ntsc_string_compare_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join(format!("{name}.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join(format!("{name}_bin"));

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), name, &dir)
        .expect("compile_source failed");
    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "binary failed: {stderr:?}");
    assert!(
        !stderr.contains("memory leak detected"),
        "comparison must not leak, stderr was: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    stdout
}

#[test]
fn string_equality_compares_contents() {
    let stdout = run(
        "streq",
        r#"fun main() {
    var a = "bad"
    var b = "bad"
    var c = "good"
    say("" + (a == b))
    say("" + (a == c))
    say("" + (a != b))
    say("" + (a != c))
    say("" + ("x" + "y" == "xy"))
}
"#,
    );
    assert_eq!(stdout.trim(), "true\nfalse\nfalse\ntrue\ntrue");
}

#[test]
fn string_equality_works_through_parameters_fields_and_views() {
    let stdout = run(
        "streq_places",
        r#"class P {
    var string a

    fun init(string r) {
        this.a = r
    }

    fun bad() -> bool { return this.a == "bad" }
}

fun owned(string r) -> bool { return r == "bad" }

fun borrowed(view string r) -> bool { return r == "bad" }

fun main() {
    var p = P("bad")
    say("" + p.bad())
    say("" + owned("bad"))
    say("" + owned("ok"))
    var s = "bad"
    say("" + borrowed(s))
    view var v = s
    say("" + (v == "bad"))
}
"#,
    );
    assert_eq!(stdout.trim(), "true\ntrue\nfalse\ntrue\ntrue");
}
