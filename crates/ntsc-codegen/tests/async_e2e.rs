//! End-to-end regression test for the Phase-2 async slice
//! (see rewrite/docs/async-rfc.md §8): `async fun` lowered to a poll-based
//! state machine driven by the runtime executor.
//!
//! Scenarios covered (one program, compiled and linked against the runtime
//! static library, then executed):
//!
//! - **Real suspension** — `await async.sleep` suspends for actual time (the
//!   future returns "pending" and the executor re-polls after a 1 ms quantum),
//!   exercising the suspend/resume path rather than a mock.
//! - **Locals survive suspension** — `var int n = 1` is written before an
//!   `await async.sleep(1)`, incremented after it, and its value (2) is passed
//!   into `wait_for` as a *sleep duration*; a dropped value would change
//!   behavior or corrupt the call.
//! - **Await results flow back across a suspension** — `var ms = await
//!   wait_for(n)` and `var first = await fetch("a")` are initializers whose
//!   child futures complete in a later poll; the resume half reloads the
//!   result and stores it into the variable's future field.
//! - **Multiple awaits in one function** — `main` awaits `sleep`, then
//!   `wait_for`, then `fetch` twice: each suspension splits the body into
//!   another segment, and the results (int `ms`, two strings) are re-read in
//!   later segments.
//! - **String results from a parameterized child** — `fetch(string url)`
//!   takes a parameter (stored in the child's future before it polls), awaits
//!   `async.sleep`, and returns `"data for " + url`; both awaits resume to the
//!   exact same URL they were called with.
//! - **Value persistence is observable** — the `if (ms == 2)` check prints
//!   "value persisted" only if the int survived the two suspensions intact.
//! - **Async `main`** — drives the root future through `__ntsc_user_main` and
//!   returns its result (0) as the process exit code, asserted via
//!   `output.status.success()`.
//!
//! The program:
//!
//! ```nt
//! async fun fetch(string url) -> string {
//!     await async.sleep(10);
//!     return "data for " + url
//! }
//!
//! async fun wait_for(int ms) -> int {
//!     await async.sleep(ms);
//!     return ms
//! }
//!
//! async fun main() -> int {
//!     var int n = 1;
//!     await async.sleep(1);
//!     n = n + 1;
//!     var ms = await wait_for(n);
//!     var first = await fetch("a");
//!     var second = await fetch("b");
//!     say(first);
//!     say(second);
//!     if (ms == 2) { say("value persisted") }
//!     return 0
//! }
//! ```

use std::path::Path;
#[test]
fn async_e2e() {
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

    let source = r#"async fun fetch(string url) -> string {
    await async.sleep(10);
    return "data for " + url
}

async fun wait_for(int ms) -> int {
    await async.sleep(ms);
    return ms
}

async fun main() -> int {
    var int n = 1;
    await async.sleep(1);
    n = n + 1;

    var ms = await wait_for(n);
    var first = await fetch("a");
    var second = await fetch("b");
    say(first);
    say(second);
    if (ms == 2) {
        say("value persisted")
    }
    return 0
}
"#;

    let dir = std::env::temp_dir().join("ntsc_async_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("async.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("async_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "async", &dir)
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
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let expected_lines = ["data for a", "data for b", "value persisted"];
    for line in expected_lines {
        assert!(
            stdout.lines().any(|l| l == line),
            "missing output line {line:?}:\n{stdout}"
        );
    }
    assert!(
        !stderr.contains("memory leak detected"),
        "async owned locals and results must be reclaimed:\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
