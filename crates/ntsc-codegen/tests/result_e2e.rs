//! End-to-end test: the built-in `result[.., ..]` type, the `?` propagation
//! operator, throw-to-Err integration, and the result/option combinators.

use std::path::Path;

#[test]
fn result_e2e() {
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

    let source = r#"fun double_it(int x) -> int {
    return x * 2
}

fun half(int x) -> result[int, string] {
    if (x % 2 == 0) {
        return Ok(x / 2)
    }
    return Err("odd input")
}

fun recover(string msg) -> result[int, string] {
    return Ok(99)
}

fun parse_num(bool good) -> result[int, string] {
    if (good) {
        return Ok(42)
    }
    return Err("not a number")
}

fun caller_ok() -> result[int, string] {
    var n = parse_num(true)?
    return Ok(n)
}

fun caller_err() -> result[int, string] {
    var n = parse_num(false)?
    return Ok(n)
}

fun thrower() -> result[string, string] {
    throw "kaboom"
    return Ok("never")
}

fun converter() -> result[int, string] {
    var code = Err(7)?
    return Ok(code)
}

// Re-wraps an error payload as an Ok value so tests can observe it.
fun reveal(string msg) -> result[string, string] {
    return Ok(msg)
}

fun reveal_int(int code) -> result[int, int] {
    return Ok(code)
}

fun main() -> int {
    say(fmt.i64_to_str(Ok(5).unwrap_or(0)))
    var e = Err("boom")
    say(e.unwrap_or("safe"))

    var ok = caller_ok()
    say(fmt.i64_to_str(ok.unwrap_or(0)))
    var bad = caller_err()
    say(fmt.i64_to_str(bad.unwrap_or(-1)))

    var m = Ok(21).map(fun(int x) -> int { return x * 2 })
    say(fmt.i64_to_str(m.unwrap_or(0)))

    var c1 = Ok(8).and_then(half)
    say(fmt.i64_to_str(c1.unwrap_or(-2)))
    var c2 = Ok(7).and_then(half)
    say(fmt.i64_to_str(c2.unwrap_or(-3)))

    var c3 = Err("lost").or_else(recover)
    say(fmt.i64_to_str(c3.unwrap_or(-4)))

    var t = thrower()
    say(t.or_else(reveal).unwrap_or("never"))

    say(converter().or_else(reveal).unwrap_or("none"))

    var option[int] none = nil
    say(fmt.i64_to_str(none.ok_or(-5).or_else(reveal_int).unwrap_or(-6)))
    none = 3
    say(fmt.i64_to_str(none.ok_or(-5).unwrap_or(-6)))

    var option[string] empty = nil
    say(empty.ok_or_else(fun() -> string { return "made" }).or_else(reveal).unwrap_or("?"))

    return 0
}
"#;

    let dir = std::env::temp_dir().join("ntsc_result_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("result.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("result_ntsc_test");

    // Compile.
    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "result", &dir)
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
        "binary exited with non-zero status: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    let expected = [
        "5",      // Ok(5).unwrap_or(0)
        "safe",   // Err("boom").unwrap_or("safe")
        "42",     // ? through caller_ok
        "-1",     // ? propagates parse failure; unwrap_or default
        "42",     // map over Ok(21)
        "4",      // and_then on even input
        "-3",     // and_then short-circuits the odd-input error
        "99",     // or_else recovers from Err
        "kaboom", // throw inside a result function becomes Err (revealed via or_else)
        "7",      // int error converted to the string error type by ?
        "-5",     // option.ok_or turns nil into Err(-5)
        "3",      // option.ok_or passes a value through as Ok
        "made",   // option.ok_or_else builds the error lazily
    ];
    assert_eq!(lines, expected, "unexpected program output:\n{stdout}");

    // Cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}
