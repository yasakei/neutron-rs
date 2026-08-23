//! End-to-end regression test for the release-only async sleep misalignment.
//!
//! Regression: the optimization pass pipeline resolves `getelementptr` byte
//! offsets and struct sizes against the *module's* data layout. When the module
//! carried no explicit data layout, `instcombine` fell back to a default where
//! `i64` is 4-byte aligned. That shrank `{ i32, i64, i64 }` (the sleep future)
//! from 24 to 20 bytes, so the *second* `await async.sleep` slot landed at a
//! 4-mod-8 address. The runtime's `#[repr(C)]` `AsyncSleepFuture` dereferences
//! it as an 8-aligned struct and panicked on a misaligned pointer dereference.
//!
//! The unoptimized (`compile_source`) path never runs the pass pipeline, so the
//! bug only appeared in `--release`. This test compiles with the aggressive
//! pipeline, runs two `await async.sleep` calls (the second slot is the one that
//! was misaligned), and asserts every segment of the timeline executes.

use std::path::Path;
#[test]
fn async_sleep_release_data_layout_e2e() {
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

    // Two sleeps in one function: the second child future slot is at a
    // different struct offset than the first, which is what exposed the bug.
    let source = r#"async fun main() -> int {
    say("t0");
    await async.sleep(1);
    say("t1");
    await async.sleep(1);
    say("t2");
    return 0
}
"#;

    let dir = std::env::temp_dir().join("ntsc_async_release_dl_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!(
        "async_release_dl.{}",
        ntsc_codegen::object_extension()
    ));
    let bin_path = dir.join("async_release_dl_ntsc_test");

    ntsc_codegen::compile_source_release(
        source,
        ntsc_codegen::host_triple(),
        "async_release_dl",
        &dir,
    )
    .expect("compile_source_release failed");

    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");

    assert!(
        output.status.success(),
        "binary exited with non-zero status (misaligned sleep future)"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let expected_lines = ["t0", "t1", "t2"];
    for line in expected_lines {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing output line {line:?}:\n{stdout}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
