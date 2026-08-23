//! End-to-end regression test for standard library error reporting.
//!
//! Regression: several `sys.*`, `fmt.*`, `json.*`, `regex.*`, `crypto.*`,
//! `process.*`, `http.*`, and `math.*` functions silently returned empty or
//! default values on failure (e.g. `fmt.to_int("x")` returned 0, `sys.read`
//! returned null, `json.parse` returned an error-object string). They now throw
//! exceptions carrying a `module.func: detail` message so callers can
//! distinguish error kinds with `try`/`catch`.

use std::path::Path;
#[test]
fn stdlib_errors_e2e() {
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
    try {
        var n = fmt.to_int("oops");
        say("unreached-int");
    } catch (err) {
        say("int: " + err);
    }

    try {
        var f = fmt.to_float("nope");
        say("unreached-float");
    } catch (err) {
        say("float: " + err);
    }

    try {
        var r = sys.read("/nonexistent_file_xyz_123");
        say("unreached-read");
    } catch (err) {
        say("read: " + err);
    }

    try {
        var w = sys.write("/nonexistent_dir_xyz_123/file", "x");
        say("unreached-write");
    } catch (err) {
        say("write: " + err);
    }

    try {
        var j = json.parse("not json");
        say("unreached-json");
    } catch (err) {
        say("json: " + err);
    }

    try {
        var m = regex.find("hello", "[invalid");
        say("unreached-regex");
    } catch (err) {
        say("regex: " + err);
    }

    try {
        var b = crypto.base64_decode("!!!not-base64!!!");
        say("unreached-b64");
    } catch (err) {
        say("b64: " + err);
    }

    try {
        var h = crypto.hex_decode("zz");
        say("unreached-hex");
    } catch (err) {
        say("hex: " + err);
    }

    try {
        var s = math.sqrt(-1.0);
        say("unreached-sqrt");
    } catch (err) {
        say("sqrt: " + err);
    }

    try {
        var out = process.spawn("/nonexistent_program_xyz_123", "");
        say("unreached-spawn");
    } catch (err) {
        say("spawn: " + err);
    }

    try {
        var resp = http.get("http://127.0.0.1:1/");
        say("unreached-http");
    } catch (err) {
        say("http: " + err);
    }

    var n = fmt.to_int("123");
    say("ok: " + n);
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_stdlib_errors_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!(
        "stdlib_errors.{}",
        ntsc_codegen::object_extension()
    ));
    let bin_path = dir.join("stdlib_errors_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "stdlib_errors", &dir)
        .expect("compile_source failed");

    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");

    assert!(
        output.status.success(),
        "binary exited with non-zero status\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    assert!(
        !stdout.contains("unreached"),
        "unexpected output:\n{stdout}"
    );

    let expected_prefixes = [
        "int: ", "float: ", "read: ", "write: ", "json: ", "regex: ", "b64: ", "hex: ", "sqrt: ",
        "spawn: ", "http: ",
    ];
    for prefix in expected_prefixes {
        assert!(
            stdout.lines().any(|l| l.starts_with(prefix)),
            "missing output line starting with {prefix:?}:\n{stdout}"
        );
    }

    // Every caught message must identify the failing function so callers can
    // distinguish error types.
    let expected_kinds = [
        "fmt.to_int:",
        "fmt.to_float:",
        "sys.read:",
        "sys.write:",
        "json.parse:",
        "regex.find:",
        "crypto.base64_decode:",
        "crypto.hex_decode:",
        "math.sqrt:",
        "process.spawn:",
        "http.get:",
    ];
    for kind in expected_kinds {
        assert!(
            stdout.contains(kind),
            "missing error kind {kind:?}:\n{stdout}"
        );
    }

    assert!(stdout.contains("ok: 123"), "unexpected output:\n{stdout}");
    assert!(
        stdout.trim().ends_with("done"),
        "unexpected output:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
