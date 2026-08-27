//! End-to-end regression test for the spread operator (`...expr`).
//!
//! Regression: call-argument spreads were statically unrolled but the argument
//! count seen by ownership transfer still included the spread node, so a call
//! like `sum(...[1, 2, 3], 4)` evaluated 4 values against a 2-entry argument
//! list and mis-zipped moves/copies. Array-literal spreads (`[...[1, 2, 3], 4]`)
//! also allocated the array with `elements.len()` instead of the flattened
//! length, so trailing elements wrote past the buffer and later reads hit
//! misaligned pointers.

use std::path::Path;

#[test]
fn spread_e2e() {
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

    let source = r#"use arrays
fun sum4(int a, int b, int c, int d) -> int {
    return a + b + c + d;
}

fun main() {
    var mid = [1, ...[2, 3], 4];
    say("mid: " + arrays.join(mid, ","));
    var nested = [1, ...[2, ...[3, 4]]];
    say("nested: " + arrays.join(nested, ","));
    var all = [...[1, 2, 3]];
    say("all: " + arrays.join(all, ","));
    var strs = ["a", ...["b", "c"], "d"];
    say("strs: " + arrays.join(strs, ","));
    var t = sum4(1, ...[2, 3], 4);
    say("t: " + t);
}
"#;

    let dir = std::env::temp_dir().join("ntsc_spread_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("spread.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("spread_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "spread", &dir)
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
        "mid: 1,2,3,4\nnested: 1,2,3,4\nall: 1,2,3\nstrs: a,b,c,d\nt: 10"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
