//! End-to-end regression test for awaited offloaded http futures over the
//! ntroutine scheduler: concurrent `go` goroutines each `await
//! http.get_async(...)` against a local server and report over a channel.
//!
//! TLS is covered by the runtime's unit tests (`https_request_round_trip_uses_tls`,
//! `https_request_rejects_untrusted_cert`): the language client trusts only the
//! bundled Mozilla roots, so a self-signed local server cannot be used here.

use std::io::{Read, Write};
use std::path::Path;

fn body_response(body: &str) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body.as_bytes());
    response
}

/// Serve `count` sequential connections, answering each `/name` request with
/// the path's name as the body.
fn spawn_server(count: usize) -> (u16, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buffer = [0u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                continue;
            };
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .trim_start_matches('/')
                .to_string();
            let body = if path.is_empty() {
                "index".to_string()
            } else {
                path
            };
            let _ = stream.write_all(&body_response(&body));
            let _ = stream.flush();
        }
    });
    (port, handle)
}

fn build_and_run(source: &str, name: &str) -> (String, String, std::process::Output) {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let runtime_lib = rewrite_dir.join(
        Path::new("target")
            .join("debug")
            .join(ntsc_codegen::runtime_lib_name()),
    );
    assert!(
        runtime_lib.exists(),
        "runtime lib not found; run `cargo build -p ntsc-runtime`"
    );

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

#[test]
fn concurrent_async_http_fetches() {
    // 3 goroutines + 1 direct await = 4 requests the server must answer.
    let (port, server) = spawn_server(4);

    let source = format!(
        r#"
use http

async fun fetch(string url, chan[string] out) {{
    var resp = await http.get_async(url)
    resp |> out
}}

async fun main() -> int {{
    var chan[string] results = chan.new(8)
    go fetch("http://127.0.0.1:{port}/alpha", results)
    go fetch("http://127.0.0.1:{port}/beta", results)
    go fetch("http://127.0.0.1:{port}/gamma", results)
    r1 <| results
    r2 <| results
    r3 <| results
    say(r1)
    say(r2)
    say(r3)
    var direct = await http.get_async("http://127.0.0.1:{port}/delta")
    say(direct)
    return 0
}}
"#
    );

    let (stdout, stderr, output) = build_and_run(&source, "async_http");
    server.join().expect("server thread");

    assert!(
        output.status.success(),
        "binary exited with non-zero status:\n{stdout}\n{stderr}"
    );
    for body in ["alpha", "beta", "gamma", "delta"] {
        assert!(
            stdout.contains(body),
            "missing response body {body:?}:\n{stdout}\n{stderr}"
        );
    }
    // The bodies ride inside the response JSON objects.
    for body in ["alpha", "beta", "gamma"] {
        assert!(
            stdout.contains(&format!("\"body\":\"{body}\"")),
            "response for {body} was not a well-formed result object:\n{stdout}"
        );
    }
    assert!(
        !stderr.contains("memory leak detected"),
        "concurrent fetch results must be reclaimed:\n{stderr}"
    );
}
