//! End-to-end regression test for calling lambda values held in variables.
//!
//! Regression: the lambda expression's `Ty::Function` used to be emitted with
//! an empty `params` list, so any indirect call against that type produced a
//! verifier failure ("Incorrect number of arguments passed to called function").

use std::path::Path;
#[test]
fn lambda_call_e2e() {
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
    var f = fun(int x) -> int { return x * 2 };
    var r = f(21);
    say("lambda: " + fmt.i64_to_str(r));

    var g = fun(int x) -> int { return x + 100 };
    var h = g;
    say("alias: " + fmt.i64_to_str(h(1)));

    var direct = (fun(int x) -> int { return x - 5 })(30);
    say("direct: " + fmt.i64_to_str(direct));

    var greet = fun() { say("void lambda"); };
    greet();
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_lambda_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("lambda.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("lambda_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "lambda", &dir)
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
        "lambda: 42\nalias: 101\ndirect: 25\nvoid lambda\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
