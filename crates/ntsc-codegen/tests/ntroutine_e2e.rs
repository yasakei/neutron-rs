//! End-to-end regression tests for the ntroutine phases (see ntroutine.md):
//! `go` spawns, `chan[T]` channels with `<|`/`|>`/`close`/`for v in chan`,
//! and awaited offloaded `http.*_async` futures, compiled and linked against
//! the runtime, then executed.

use std::io::{Read, Write};
use std::path::Path;

fn build_and_run(source: &str, name: &str) -> (String, String, std::process::Output) {
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

    let dir = std::env::temp_dir().join(format!("ntsc_{name}_e2e_test"));
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("{name}.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join(format!("{name}_ntsc_test"));

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), name, &dir)
        .expect("compile_source failed");
    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    (stdout, stderr, output)
}

fn assert_clean(stdout: &str, stderr: &str, output: &std::process::Output, expected: &[&str]) {
    assert!(
        output.status.success(),
        "binary exited with non-zero status:\n{stdout}\n{stderr}"
    );
    for line in expected {
        assert!(
            stdout.lines().any(|l| l == *line),
            "missing output line {line:?}:\n{stdout}\n{stderr}"
        );
    }
    assert!(
        !stderr.contains("memory leak detected"),
        "goroutine and channel values must be reclaimed:\n{stderr}"
    );
}

#[test]
fn go_call_and_channel_handshake() {
    let source = r#"
async fun producer(chan[int] ch) {
    7 |> ch
    close(ch)
}

async fun main() -> int {
    var chan[int] ch = chan.new(2)
    go producer(ch)
    x <| ch
    if (x == 7) {
        say("handshake ok")
    }
    y <| ch
    return 0
}
"#;
    let (stdout, stderr, output) = build_and_run(source, "ntroutine_go_call");
    assert_clean(&stdout, &stderr, &output, &["handshake ok"]);
}

#[test]
fn go_block_captures_and_for_in_chan() {
    let source = r#"
async fun main() -> int {
    var chan[string] words = chan.new(4)
    go {
        for w in words {
            say(w)
        }
    }
    "alpha" |> words
    "beta" |> words
    close(words)
    await async.sleep(100)
    return 0
}
"#;
    let (stdout, stderr, output) = build_and_run(source, "ntroutine_go_block");
    assert_clean(&stdout, &stderr, &output, &["alpha", "beta"]);
}

#[test]
fn go_block_scalar_capture() {
    let source = r#"
async fun main() -> int {
    var int base = 40
    go {
        say("capture " + base)
    }
    await async.sleep(100)
    return 0
}
"#;
    let (stdout, stderr, output) = build_and_run(source, "ntroutine_go_scalar");
    assert_clean(&stdout, &stderr, &output, &["capture 40"]);
}

#[test]
fn await_offloaded_http_get() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\nhello from test\r\n",
            );
            let _ = stream.flush();
        }
    });

    let source = format!(
        r#"
use http

async fun main() -> int {{
    var resp = await http.get_async("http://127.0.0.1:{port}/")
    say(resp)
    return 0
}}
"#
    );

    let (stdout, stderr, output) = build_and_run(&source, "ntroutine_http");
    server.join().expect("server thread");
    assert!(
        output.status.success(),
        "binary exited with non-zero status:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("hello from test"),
        "missing response body:\n{stdout}\n{stderr}"
    );
    assert!(
        !stderr.contains("memory leak detected"),
        "offloaded http futures must be reclaimed:\n{stderr}"
    );
}
