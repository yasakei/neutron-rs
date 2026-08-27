//! End-to-end regression test for the Phase-1 concurrency primitives
//! (see rewrite/docs/async-rfc.md): `process.spawn_thread`, `process.thread_join`,
//! and the `collections.channel` family.
//!
//! Coverage:
//! - A producer thread sends values through a channel; the main thread receives.
//! - The main thread sends; a consumer thread receives and prints.
//! - `collections.channel_try_recv` non-blocking receive.
//! - `process.thread_join` waits for the spawned workers.

use std::path::Path;
#[test]
fn concurrency_e2e() {
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

    let source = r#"use collections
use process
fun main() {
    // ── Producer thread → main receives ───────────────────────────────────
    var rx = collections.channel(4);
    var tx = collections.channel_sender(rx);
    var producer = process.spawn_thread(fun(int tx) {
        collections.channel_send(tx, "ping");
        collections.channel_send(tx, "pong");
        collections.channel_close(tx);
    }, tx);
    say("c0: " + collections.channel_recv(rx));
    say("c1: " + collections.channel_recv(rx));
    process.thread_join(producer);
    collections.channel_close(rx);

    // ── Main sends → consumer thread receives ────────────────────────────
    var rx2 = collections.channel(2);
    var tx2 = collections.channel_sender(rx2);
    var consumer = process.spawn_thread(fun(int rx2) {
        say("c2: " + collections.channel_recv(rx2));
        collections.channel_close(rx2);
    }, rx2);
    collections.channel_send(tx2, "hello from main");
    process.thread_join(consumer);
    collections.channel_close(tx2);

    // ── Non-blocking try_recv ─────────────────────────────────────────────
    // `channel_try_recv` returns the received string, or the empty string
    // when the channel is empty or every sender end is closed.
    var rx3 = collections.channel(0);
    var tx3 = collections.channel_sender(rx3);
    say("c3: [" + collections.channel_try_recv(rx3) + "]");
    collections.channel_send(tx3, "tryme");
    say("c4: [" + collections.channel_try_recv(rx3) + "]");
    collections.channel_close(tx3);
    collections.channel_close(rx3);

    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_concurrency_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("concurrency.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("concurrency_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "concurrency", &dir)
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
        "c0: ping",
        "c1: pong",
        "c2: hello from main",
        "c3: []",
        "c4: [tryme]",
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
