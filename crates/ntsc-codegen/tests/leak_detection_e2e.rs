//! End-to-end test for 4.2 memory-leak detection: `ntsc_runtime_shutdown` is
//! emitted by the generated `main` with a report flag that is nonzero only in
//! debug (unoptimized) builds. Debug builds warn on stderr when RC allocations
//! remain at exit; release builds stay silent.
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

/// Compile + link + run `source`, returning (status, stdout, stderr).
fn compile_run(
    rewrite_dir: &Path,
    name: &str,
    source: &str,
    opt_level: inkwell::OptimizationLevel,
) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("ntsc_leak_e2e_{name}"));
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

#[test]
fn debug_build_warns_on_leaked_allocations() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    // `var c = b` gives the instance a second name. The class-drop analysis
    // rejects a candidate that is aliased, because either name could still read
    // the fields and freeing them once per name would double-free them, so the
    // fields are deliberately leaked instead. That makes this the positive
    // control for the detector itself: a debug build must report the abandoned
    // array.
    let source = r#"class Bag {
    var array[int] items

    fun init() {
        this.items = [1, 2]
    }
}

fun main() -> int {
    var b = Bag()
    var c = b
    say("aliased " + arrays.length(c.items))
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "debug_warn",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "aliased 2");
    assert!(
        stderr.contains("warning[NTSC-W0002]: memory leak detected:"),
        "debug build must warn about leaked allocations, stderr was: {stderr:?}"
    );
    assert!(
        stderr.contains("--> <source>:5:22")
            && stderr.contains("array handle")
            && stderr.contains("was allocated here"),
        "leak warning must identify the surviving registry object, stderr was: {stderr:?}"
    );
}

/// Regression: overwriting a field drops the value it held, including the empty
/// array an `init`-less class default-initializes it with. This program used to
/// leak that abandoned default.
#[test]
fn overwritten_field_default_is_reclaimed() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"class Box {
    var array[int] data
}

fun main() -> int {
    var b = Box()
    b.data = [1, 2, 3]
    say("boxed")
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "field_default_reclaimed",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "boxed");
    assert!(
        !stderr.contains("memory leak detected"),
        "the overwritten default must be reclaimed, stderr was: {stderr:?}"
    );
}

/// Regression: a class with a declared `init` stores an owned array in a field
/// and is heap-allocated. The instance's fields are now reclaimed by the class
/// drop thunk, so a debug build must not report the array as leaked.
#[test]
fn heap_class_with_owned_field_is_reclaimed() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"class Ledger {
    var array[int] amounts

    fun init() {
        this.amounts = [];
    }

    fun add(int v) {
        arrays.push(this.amounts, v);
    }
}

fun main() -> int {
    var ledger = Ledger()
    ledger.add(5)
    ledger.add(5)
    say("total: " + arrays.length(ledger.amounts))
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "class_field_reclaimed",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "total: 2");
    assert!(
        !stderr.contains("memory leak detected"),
        "class field arrays must be reclaimed, stderr was: {stderr:?}"
    );
}

/// Regression: an `init`-less class (stack-allocated by escape analysis) must
/// default-initialize its owned container fields. Otherwise pushing into
/// `this.amounts` writes to a null handle and the value is silently lost.
#[test]
fn initless_class_default_initializes_owned_fields() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"class Ledger {
    var array[int] amounts

    fun add(int v) {
        arrays.push(this.amounts, v);
    }
}

fun main() -> int {
    var ledger = Ledger()
    ledger.add(5)
    ledger.add(5)
    say("total: " + arrays.length(ledger.amounts))
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "initless_defaults",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "total: 2");
    assert!(
        !stderr.contains("memory leak detected"),
        "init-less class fields must be reclaimed, stderr was: {stderr:?}"
    );
}

#[test]
fn debug_build_clean_program_is_silent() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    // No heap arrays / RC objects are allocated, so nothing is leaked.
    let source = r#"fun main() -> int {
    say("no allocations")
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "debug_clean",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "no allocations");
    assert!(
        !stderr.contains("memory leak detected"),
        "clean program must not warn, stderr was: {stderr:?}"
    );
}

#[test]
fn release_build_is_silent_even_with_allocations() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"fun main() -> int {
    var a = [1, 2, 3]
    say("sum")
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "release_silent",
        source,
        inkwell::OptimizationLevel::Aggressive,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "sum");
    assert!(
        !stderr.contains("memory leak detected"),
        "release build must stay silent, stderr was: {stderr:?}"
    );
}

/// Regression: the codegen now emits release code for arrays the program owns
/// (local variables, fresh temporaries, function returns), so a debug build
/// must not report RC leaks for programs that exercise those paths.
#[test]
fn reclaimed_arrays_do_not_warn() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"fun total(array[int] xs) -> int {
    var sum = 0;
    for (var x in xs) {
        sum = sum + x;
    }
    return sum;
}

fun doubles(array[int] xs) -> array[int] {
    var array[int] out = [];
    for (var x in xs) {
        arrays.push(out, x * 2);
    }
    return out;
}

fun main() -> int {
    var a = [1, 2, 3];
    for (var i = 0; i < 10; i = i + 1) {
        a = [i, i + 1];
    }
    var nested = [[1], [2, 2], [3, 3, 3]];
    var t = total([1, 2, 3, 4]);
    var d = doubles([5, 6, 7]);
    var sum = t;
    for (var x in d) {
        sum = sum + x;
    }
    for (var row in [[1, 1], [2, 2]]) {
        sum = sum + total(row);
    }
    say("sum: " + sum);
    say("a: " + arrays.length(a));
    say("nested: " + arrays.length(nested));
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "reclaimed",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "sum: 52\na: 2\nnested: 3");
    assert!(
        !stderr.contains("memory leak detected"),
        "owned arrays must be reclaimed, stderr was: {stderr:?}"
    );
}

/// Regression: nested arrays, element overwrites, fresh element reads, and
/// aliased arrays must all reclaim their RC references in a debug build.
#[test]
fn nested_and_aliased_arrays_do_not_warn() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"fun main() -> int {
    var a = [[1, 2], [3, 4]];
    var x = a[0][1];
    a[0] = [9, 9];
    var b = copy(a);
    b[1] = [7, 7];
    var c = [[[1]], [[2]]];
    c[0] = [[7]];
    var lit = [[1, 2], [3, 4]][1][0];
    var same = [1, 2];
    var alias = copy(same);
    same = copy(alias);
    say("x: " + x);
    say("a00: " + a[0][0]);
    say("b10: " + b[1][0]);
    say("c: " + c[0][0][0]);
    say("lit: " + lit);
    say("sum: " + (same[0] + alias[1]));
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "nested_alias",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "x: 2\na00: 9\nb10: 7\nc: 7\nlit: 3\nsum: 3");
    assert!(
        !stderr.contains("memory leak detected"),
        "nested/aliased arrays must be reclaimed, stderr was: {stderr:?}"
    );
}

/// Regression: `sort.*` and `random.shuffle` clone their input into a new
/// array. The fresh result must be balanced at the consuming site and the
/// borrowed input must not be released.
#[test]
fn sort_and_shuffle_do_not_warn() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"fun main() -> int {
    var nums = [3, 1, 2];
    var sorted = sort.stable_sort(nums);
    var inline = sort.stable_sort([9, 7, 8]);
    var shuffled = random.shuffle(nums);
    say("sorted: " + sorted[0]);
    say("inline: " + inline[2]);
    say("len: " + arrays.length(shuffled));
    say("nums: " + nums[0]);
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "sort_shuffle",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "binary must exit 0");
    assert_eq!(stdout.trim(), "sorted: 1\ninline: 9\nlen: 3\nnums: 3");
    assert!(
        !stderr.contains("memory leak detected"),
        "sort/shuffle results must be reclaimed, stderr was: {stderr:?}"
    );
}

/// Regression: an async future reuses its class local across loop iterations.
/// Fresh copied instances must be dropped before the next iteration replaces
/// the future field, including owned string fields.
#[test]
fn async_channel_workers_reclaim_reused_class_locals() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();

    let source = r#"class Ticket {
    var int id
    var string requester

    fun init(int id, string requester) {
        this.id = id
        this.requester = requester
    }
}

fun worker(int worker_id, int rx) {
    while (true) {
        var job = collections.channel_recv(rx)
        if (job == "") {
            break
        }
        say("worker " + worker_id + " took: " + job)
    }
}

async fun main() -> int {
    var array[Ticket] tickets = [Ticket(0, "caller-0"), Ticket(1, "caller-1"), Ticket(2, "caller-2")]
    var int rx = collections.channel(8)
    var int w1 = process.spawn_thread(fun(int rx) { worker(1, rx) }, rx)
    var int w2 = process.spawn_thread(fun(int rx) { worker(2, rx) }, rx)
    var int tx = collections.channel_sender(rx)
    for (var i = 0; i < arrays.length(tickets); i = i + 1) {
        var Ticket ticket = copy(tickets[i])
        collections.channel_send(tx, "job for #" + ticket.id)
    }
    collections.channel_close(tx)
    process.thread_join(w1)
    process.thread_join(w2)
    collections.channel_close(rx)
    say("done")
    return 0
}
"#;
    let (ok, stdout, stderr) = compile_run(
        rewrite_dir,
        "async_channel_class_locals",
        source,
        inkwell::OptimizationLevel::None,
    );
    assert!(ok, "async worker program must run, stderr was: {stderr:?}");
    assert!(stdout.lines().any(|line| line.starts_with("worker ")));
    assert!(stdout.lines().any(|line| line == "done"));
    assert!(
        !stderr.contains("memory leak detected"),
        "reused async class locals must be reclaimed, stderr was: {stderr:?}"
    );
}
