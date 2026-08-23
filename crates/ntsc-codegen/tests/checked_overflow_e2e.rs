//! End-to-end tests for defined integer overflow and shift behavior.
//!
//! Plain `add`/`sub`/`mul` wrap and the `nsw` variants are poison on overflow,
//! and `shl`/`ashr` are poison when the shift amount is out of range. Poison
//! lets an optimized build compute a different value than a debug build from the
//! same source, which the language forbids, so every one of those cases throws a
//! catchable exception instead.
//!
//! Each program therefore runs in a debug *and* an optimized build and both must
//! print the same thing. The debug build also has leak detection enabled, so the
//! throw edges out of the middle of an expression are checked for dropping the
//! temporaries and locals that were already live.

use std::path::Path;

/// Build the runtime static library if missing and return its path.
fn runtime_lib(rewrite_dir: &Path) -> std::path::PathBuf {
    let lib = rewrite_dir.join(
        Path::new("target")
            .join("debug")
            .join(ntsc_codegen::runtime_lib_name()),
    );
    if !lib.exists() {
        let status = std::process::Command::new("cargo")
            .args(["build", "-p", "ntsc-runtime"])
            .current_dir(rewrite_dir)
            .status()
            .expect("failed to run cargo");
        assert!(status.success(), "failed to build ntsc-runtime");
    }
    assert!(lib.exists(), "runtime lib not found at {lib:?}");
    lib
}

/// Compile + link + run `source`, returning (exit-ok, stdout, stderr).
fn compile_run(
    name: &str,
    source: &str,
    opt_level: inkwell::OptimizationLevel,
) -> (bool, String, String) {
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = std::env::temp_dir().join(format!("ntsc_checked_overflow_{name}"));
    std::fs::create_dir_all(&dir).unwrap();

    let program = {
        let tokens = ntsc_lexer::tokenize(source);
        ntsc_parser::parse(&tokens).expect("parse failed")
    };
    ntsc_codegen::compile_program(
        &program,
        ntsc_codegen::host_triple(),
        opt_level,
        name,
        &dir,
        false,
    )
    .expect("compile failed");

    let bin_path = dir.join(name);
    ntsc_codegen::link_binary(
        &dir.join(format!("{name}.{}", ntsc_codegen::object_extension())),
        &runtime_lib(rewrite_dir),
        &bin_path,
    )
    .expect("link failed");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");
    let _ = std::fs::remove_dir_all(&dir);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Assert `source` prints `expected` in a debug build and in an optimized build,
/// and that the debug build reports no leak. Equal output across the two builds
/// is the property that matters here: a wrapping or poison result would differ.
fn assert_same_in_both_builds(name: &str, source: &str, expected: &str) {
    let (ok, stdout, stderr) = compile_run(name, source, inkwell::OptimizationLevel::None);
    assert!(ok, "debug build failed: {stderr:?}\n{stdout}");
    assert_eq!(stdout.trim(), expected, "debug build output");
    assert!(
        !stderr.contains("memory leak detected"),
        "debug build leaked: {stderr:?}"
    );

    let (ok, opt_stdout, opt_stderr) = compile_run(
        &format!("{name}_opt"),
        source,
        inkwell::OptimizationLevel::Aggressive,
    );
    assert!(ok, "optimized build failed: {opt_stderr:?}\n{opt_stdout}");
    assert_eq!(
        opt_stdout.trim(),
        expected,
        "optimized build must agree with the debug build"
    );
}

#[test]
fn addition_and_subtraction_overflow_throw() {
    assert_same_in_both_builds(
        "addsub",
        r#"fun main() {
    var max = 9223372036854775807
    var min = -9223372036854775807 - 1
    try {
        var x = max + 1
        say("a1: WRONG " + x)
    } catch (e) { say("a1: " + e) }
    try {
        var x = min + min
        say("a2: WRONG " + x)
    } catch (e) { say("a2: " + e) }
    try {
        var x = min - 1
        say("s1: WRONG " + x)
    } catch (e) { say("s1: " + e) }
    try {
        var x = max - min
        say("s2: WRONG " + x)
    } catch (e) { say("s2: " + e) }
    say("ok: " + (max - 1) + " " + (min + 1) + " " + (max + min))
}
"#,
        "a1: integer addition overflow\n\
         a2: integer addition overflow\n\
         s1: integer subtraction overflow\n\
         s2: integer subtraction overflow\n\
         ok: 9223372036854775806 -9223372036854775807 -1",
    );
}

#[test]
fn multiplication_overflow_throws() {
    assert_same_in_both_builds(
        "mul",
        r#"fun main() {
    var max = 9223372036854775807
    var min = -9223372036854775807 - 1
    try {
        var x = max * 2
        say("m1: WRONG " + x)
    } catch (e) { say("m1: " + e) }
    try {
        var x = min * -1
        say("m2: WRONG " + x)
    } catch (e) { say("m2: " + e) }
    try {
        var x = 4611686018427387904 * 2
        say("m3: WRONG " + x)
    } catch (e) { say("m3: " + e) }
    say("ok: " + (max * 1) + " " + (max * 0) + " " + (3 * 5))
}
"#,
        "m1: integer multiplication overflow\n\
         m2: integer multiplication overflow\n\
         m3: integer multiplication overflow\n\
         ok: 9223372036854775807 0 15",
    );
}

#[test]
fn negation_and_increment_overflow_throw() {
    assert_same_in_both_builds(
        "neginc",
        r#"fun main() {
    var max = 9223372036854775807
    var min = -9223372036854775807 - 1
    try {
        var x = -min
        say("n1: WRONG " + x)
    } catch (e) { say("n1: " + e) }
    say("n2: " + (-max))
    var up = max
    try {
        up++
        say("i1: WRONG " + up)
    } catch (e) { say("i1: " + e) }
    var down = min
    try {
        down--
        say("i2: WRONG " + down)
    } catch (e) { say("i2: " + e) }
    var n = 5
    n++
    n--
    n--
    say("i3: " + n)
}
"#,
        "n1: integer subtraction overflow\n\
         n2: -9223372036854775807\n\
         i1: integer addition overflow\n\
         i2: integer subtraction overflow\n\
         i3: 4",
    );
}

#[test]
fn out_of_range_shift_amounts_throw() {
    assert_same_in_both_builds(
        "shift",
        r#"fun main() {
    var wide = 64
    try {
        var x = 1 << wide
        say("l1: WRONG " + x)
    } catch (e) { say("l1: " + e) }
    try {
        var x = 1 << -1
        say("l2: WRONG " + x)
    } catch (e) { say("l2: " + e) }
    try {
        var x = 8 >> 64
        say("r1: WRONG " + x)
    } catch (e) { say("r1: " + e) }
    try {
        var x = 8 >> -2
        say("r2: WRONG " + x)
    } catch (e) { say("r2: " + e) }
    say("ok: " + (1 << 62) + " " + (-8 >> 2) + " " + (1 << 0) + " " + (5 >> 63))
}
"#,
        "l1: shift amount out of range\n\
         l2: shift amount out of range\n\
         r1: shift amount out of range\n\
         r2: shift amount out of range\n\
         ok: 4611686018427387904 -2 1 0",
    );
}

#[test]
fn an_overflow_throw_reclaims_live_values() {
    // The guard branches to the enclosing handler from the middle of an
    // expression, so the drops for values already live on that path have to run
    // on the throw edge like any other exception.
    assert_same_in_both_builds(
        "cleanup",
        r#"fun main() {
    var max = 9223372036854775807
    try {
        var kept = "one" + "two"
        var xs = [1, 2, 3]
        var x = max + arrays.length(xs)
        say("c1: WRONG " + x + kept)
    } catch (e) { say("c1: " + e) }
    for (var i = 0; i < 3; i = i + 1) {
        try {
            var label = "iter" + i
            var x = max * (i + 2)
            say("c2: WRONG " + x + label)
        } catch (e) { say("c2: " + e) }
    }
    say("done")
}
"#,
        "c1: integer addition overflow\n\
         c2: integer multiplication overflow\n\
         c2: integer multiplication overflow\n\
         c2: integer multiplication overflow\n\
         done",
    );
}

#[test]
fn overflow_propagates_out_of_a_function_and_is_retryable() {
    assert_same_in_both_builds(
        "propagate",
        r#"fun grow(int n) -> int {
    return n * 2
}

fun main() {
    var big = 4611686018427387904
    try {
        var x = grow(big)
        say("p1: WRONG " + x)
    } catch (e) { say("p1: " + e) }
    say("p2: " + grow(21))

    var attempts = 0
    retry 3 {
        attempts = attempts + 1
        var x = grow(big)
        say("p3: WRONG " + x)
    } catch (e) {
        say("p3: " + e + " after " + attempts)
    }
}
"#,
        "p1: integer multiplication overflow\n\
         p2: 42\n\
         p3: integer multiplication overflow after 3",
    );
}

#[test]
fn an_overflow_in_an_async_body_completes_the_future_as_uncaught() {
    // `try`/`throw`/`retry` are rejected inside async bodies, so a guard that
    // fires there has nothing to catch it: the poll completes the future and the
    // pending exception is reported as uncaught instead of the state machine
    // running on with a wrapped value. Regression: the throw edge used to leave
    // the poll function's exception block without a terminator, which failed
    // module verification.
    let source = r#"async fun main() -> int {
    var int n = 4611686018427387904
    await async.sleep(1)
    var int doubled = n * 4
    say("WRONG " + doubled)
    return 0
}
"#;
    for (name, opt) in [
        ("async_overflow", inkwell::OptimizationLevel::None),
        ("async_overflow_opt", inkwell::OptimizationLevel::Aggressive),
    ] {
        let (ok, stdout, stderr) = compile_run(name, source, opt);
        assert!(!ok, "{name}: overflow must not be ignored: {stdout}");
        assert!(
            stderr.contains("uncaught exception: integer multiplication overflow"),
            "{name}: unexpected stderr {stderr:?}"
        );
        assert!(!stdout.contains("WRONG"), "{name}: kept running: {stdout}");
    }
}
