//! End-to-end test: compile a NTSC hello world to a native binary and run it.

use std::path::Path;

#[test]
fn hello_world_e2e() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let runtime_lib = rewrite_dir.join(
        Path::new("target")
            .join("debug")
            .join(ntsc_codegen::runtime_lib_name()),
    );

    // Ensure runtime is built.
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

    let source = r#"fun main() -> int {
    say("Hello, World!")
    return 0
}
"#;

    let dir = std::env::temp_dir().join("ntsc_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("hello.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("hello_ntsc_test");

    // Compile.
    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "hello", &dir)
        .expect("compile_source failed");

    assert!(obj_path.exists(), "object file not produced");

    // Link.
    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    assert!(bin_path.exists(), "binary not produced");

    // Run and capture output.
    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");

    assert!(
        output.status.success(),
        "binary exited with non-zero status"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Hello, World!");

    // Cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}
