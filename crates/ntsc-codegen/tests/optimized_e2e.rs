//! End-to-end regression tests for the IR optimization pipeline (PROMT §9.1).
//!
//! Debug builds emit alloca-based slots and skip the IR pass pipeline, so the
//! unoptimized build is the reference implementation. Release builds run
//! mem2reg/instcombine/simplifycfg/sccp/dce/gvn, which promote slots to SSA
//! registers. These tests assert the optimizer never changes observable
//! behavior — especially for the ownership codegen (drops, moves, shared
//! retains, escape-analysis stack slots), where passes could in principle
//! delete a load-bearing store.
use std::path::Path;

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

fn compile_run(
    rewrite_dir: &Path,
    name: &str,
    source: &str,
    opt_level: inkwell::OptimizationLevel,
) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("ntsc_opt_{name}"));
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

fn workspace_root() -> std::path::PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

/// A program exercising every ownership pattern the optimizer could disturb:
/// loops with accumulator slots (mem2reg), array RC drops, for-in iterators,
/// string concatenation, escape-analysis class stack slots, shared retains,
/// and move-null patterns.
const OWNERSHIP_SOURCE: &str = r#"class Counter {
    var int total

    fun bump(int n) -> int {
        this.total = this.total + n;
        return this.total;
    }
}

fun sum_upto(int n) -> int {
    var int acc = 0;
    var int i = 0;
    while (i < n) {
        acc = acc + i;
        i = i + 1;
    }
    return acc;
}

fun total(array[int] xs) -> int {
    var int acc = 0;
    for (var x in xs) {
        acc = acc + x;
    }
    return acc;
}

fun main() {
    var c = Counter();
    c.bump(10);
    c.bump(5);
    say("counter: " + c.total);

    say("sum_upto: " + sum_upto(100));

    var xs = [1, 2, 3, 4, 5];
    xs[0] = 99;
    say("xs: " + total(xs));

    var moved = [6, 7, 8];
    say("moved: " + total(moved));

    var s = "hello";
    s = s + " " + "world";
    say("s: " + s);

    shared array[int] sh = [7, 8, 9];
    var view array[int] v = sh;
    v[1] = 42;
    say("sh: " + v[1] + " " + sh[1]);

    var acc2 = 0;
    var j = 0;
    while (j < 1000) {
        acc2 = acc2 + j;
        j = j + 1;
    }
    say("acc2: " + acc2);
    say("done");
}
"#;

#[test]
fn optimized_and_unoptimized_builds_behave_identically() {
    let rewrite_dir = workspace_root();

    let (debug_ok, debug_stdout, debug_stderr) = compile_run(
        &rewrite_dir,
        "agree_debug",
        OWNERSHIP_SOURCE,
        inkwell::OptimizationLevel::None,
    );
    let (rel_ok, rel_stdout, rel_stderr) = compile_run(
        &rewrite_dir,
        "agree_release",
        OWNERSHIP_SOURCE,
        inkwell::OptimizationLevel::Aggressive,
    );

    assert!(
        debug_ok,
        "unoptimized build must run, stderr: {debug_stderr:?}"
    );
    assert!(
        rel_ok,
        "optimized build must run (miscompile suspected), stderr: {rel_stderr:?}"
    );
    assert_eq!(
        debug_stdout, rel_stdout,
        "optimized build changed program output"
    );
}

#[test]
fn optimized_build_does_not_double_free_shared_values() {
    let rewrite_dir = workspace_root();
    let source = r#"fun main() {
    shared array[int] a = [1, 2, 3];
    shared array[int] b = a;
    shared array[int] c = a;
    say("n: " + a[0]);
    b[0] = 10;
    c[0] = 20;
    say("a: " + a[0]);
    a = [4, 5];
    say("b: " + b[0]);
    say("done");
}
"#;
    let (ok, stdout, stderr) = compile_run(
        &rewrite_dir,
        "shared_opt",
        source,
        inkwell::OptimizationLevel::Aggressive,
    );
    assert!(ok, "optimized shared program must run, stderr: {stderr:?}");
    assert_eq!(
        stdout.trim(),
        "n: 1\na: 20\nb: 20\ndone",
        "optimized shared program produced wrong output: {stdout:?}"
    );
}
