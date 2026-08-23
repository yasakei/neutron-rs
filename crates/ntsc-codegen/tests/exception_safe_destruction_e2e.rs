//! End-to-end tests for exception-safe destruction: every initialized owned
//! value is reclaimed exactly once on normal return, throw, rethrow, retry,
//! break, and continue, including partially initialized instances and
//! temporaries.
//!
//! Each program is compiled and run twice. The debug build has leak detection
//! enabled, so an under-drop shows up as `memory leak detected`.
//! on stderr. Both builds assert the program's exact output, which is what
//! catches the other direction: a value dropped twice is freed while another
//! owner still reads it, so the reads print the wrong bytes or the process
//! dies. Release builds additionally check that the optimizer's different
//! block layout does not change which drops run.
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
    let dir = std::env::temp_dir().join(format!("ntsc_exc_destr_{name}"));
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

/// Assert `source` prints `expected` and leaks nothing, in a debug build (leak
/// detection on) and in an optimized build (leak detection off, different block
/// layout).
fn assert_clean(name: &str, source: &str, expected: &str) {
    let (ok, stdout, stderr) = compile_run(name, source, inkwell::OptimizationLevel::None);
    assert!(ok, "debug binary must exit 0, stderr was: {stderr:?}");
    assert_eq!(stdout.trim(), expected, "debug build output");
    assert!(
        !stderr.contains("memory leak detected"),
        "debug build must not leak, stderr was: {stderr:?}"
    );

    let (ok, stdout, stderr) = compile_run(
        &format!("{name}_opt"),
        source,
        inkwell::OptimizationLevel::Aggressive,
    );
    assert!(ok, "optimized binary must exit 0, stderr was: {stderr:?}");
    assert_eq!(stdout.trim(), expected, "optimized build output");
    assert!(
        !stderr.contains("memory leak detected"),
        "optimized build must stay silent, stderr was: {stderr:?}"
    );
}

/// A declaration inside a loop body reuses one entry-block alloca per
/// iteration, so the previous iteration's value has to be reclaimed before the
/// slot is overwritten.
#[test]
fn loop_body_locals_are_reclaimed_each_iteration() {
    assert_clean(
        "loop_locals",
        r#"fun main() -> int {
    for (var i = 0; i < 3; i = i + 1) {
        var xs = [i, i + 1]
        var s = "n" + i
        say(s + ":" + xs[1])
    }
    return 0
}
"#,
        "n0:1\nn1:2\nn2:3",
    );
}

/// `break` and `continue` leave the body without reaching its end, so the
/// locals initialized before them must be dropped on those edges too.
#[test]
fn break_and_continue_reclaim_locals() {
    assert_clean(
        "break_continue",
        r#"fun main() -> int {
    for (var i = 0; i < 5; i = i + 1) {
        var xs = [i]
        if (i == 1) { continue }
        if (i == 3) { break }
        say("" + xs[0])
    }
    return 0
}
"#,
        "0\n2",
    );
}

/// An inner block that redeclares a name shares the outer slot, so the shadowed
/// value must be reclaimed as well as the shadowing one.
#[test]
fn shadowed_slot_is_reclaimed() {
    assert_clean(
        "shadowed_slot",
        r#"fun main() -> int {
    var xs = [1]
    say("outer " + arrays.length(xs))
    {
        var xs = [2, 2]
        say("inner " + arrays.length(xs))
    }
    return 0
}
"#,
        "outer 1\ninner 2",
    );
}

/// Throwing past initialized locals must reclaim each of them — an array, a
/// freshly concatenated string, and an object temporary.
#[test]
fn throw_past_owned_locals_reclaims_them() {
    assert_clean(
        "throw_past_locals",
        r#"fun main() -> int {
    try {
        var xs = [1, 2]
        var s = "hello " + "world"
        var o = {k: 1}
        throw "boom"
    } catch (e) { say("caught " + e) }
    return 0
}
"#,
        "caught boom",
    );
}

/// The exception message is transferred into the catch binding, which owns it
/// for the handler and drops it at the end.
#[test]
fn catch_binding_owns_the_exception_message() {
    assert_clean(
        "catch_binding",
        r#"fun main() -> int {
    try { throw "boom" } catch (e) { say("caught " + e) }
    try { throw "x" + "y" } catch (e) { say("caught " + e) }
    return 0
}
"#,
        "caught boom\ncaught xy",
    );
}

/// Rethrowing from a handler hands the message on without dropping it twice,
/// and the locals of the throwing frame are still reclaimed.
#[test]
fn rethrow_transfers_the_message_once() {
    assert_clean(
        "rethrow",
        r#"fun inner() {
    var xs = [1, 2]
    throw "deep"
}

fun main() -> int {
    try {
        try { inner() } catch (e) { throw e }
    } catch (e) { say("outer " + e) }
    return 0
}
"#,
        "outer deep",
    );
}

/// Each `retry` attempt runs the body again, so every attempt's locals are
/// reclaimed on the throwing edge as well as on the successful one.
#[test]
fn retry_reclaims_every_attempt() {
    assert_clean(
        "retry_attempts",
        r#"fun main() -> int {
    var attempts = 0
    retry 3 {
        var xs = [1, 2]
        attempts = attempts + 1
        if (attempts < 3) { throw "again" }
        say("ok " + arrays.length(xs))
    } catch (e) { say("gave up") }
    say("attempts " + attempts)
    return 0
}
"#,
        "ok 2\nattempts 3",
    );
}

/// Destructuring an array or an object literal takes ownership of the pieces it
/// binds and reclaims the temporary it unpacked.
#[test]
fn destructuring_reclaims_the_temporary() {
    assert_clean(
        "destructuring",
        r#"fun main() -> int {
    var [a, b] = [[1, 2], [3]]
    say("" + arrays.length(a) + arrays.length(b))
    var {name, age} = {name: "ada", age: 3}
    say("" + name + age)
    return 0
}
"#,
        "21\n\"ada\"3",
    );
}

/// A constructor argument is emitted once by the caller and handed to `init`:
/// re-emitting it would allocate a second array and orphan the first.
#[test]
fn constructor_arguments_are_constructed_once() {
    assert_clean(
        "ctor_arguments",
        r#"class Bag {
    var array[int] items
    var string tag

    fun init(array[int] xs, string t) {
        this.items = xs
        this.tag = t
    }
}

fun main() -> int {
    var b = Bag([1, 2, 3], "one")
    say(b.tag + " " + arrays.length(b.items))
    return 0
}
"#,
        "one 3",
    );
}

/// An instance built inside a loop body or a nested block owns its fields
/// exactly like one built at the top level.
#[test]
fn nested_and_loop_instances_are_reclaimed() {
    assert_clean(
        "nested_instances",
        r#"class Counter {
    var int n
    var array[int] seen

    fun init(int start) {
        this.n = start
        this.seen = [start]
    }
}

fun main() -> int {
    for (var i = 0; i < 3; i = i + 1) {
        var c = Counter(i)
        say("" + c.n + arrays.length(c.seen))
    }
    {
        var c = Counter(9)
        say("" + c.n)
    }
    return 0
}
"#,
        "01\n11\n21\n9",
    );
}

/// An `init` that throws half-way has already moved values into fields, and the
/// instance never reaches the caller's slot, so the constructor reclaims
/// exactly the fields it had initialized.
#[test]
fn partially_initialized_instance_is_reclaimed() {
    assert_clean(
        "partial_construction",
        r#"class Pair {
    var string left
    var string right

    fun init(string l, string r) {
        this.left = l
        if (r == "bad") { throw "mid" }
        this.right = r
    }
}

fun main() -> int {
    try {
        var p = Pair("a", "bad")
        say("built")
    } catch (e) { say("caught " + e) }
    var ok = Pair("x", "y")
    say(ok.left + ok.right)
    return 0
}
"#,
        "caught mid\nxy",
    );
}

/// Throwing before any field is written leaves every field at the zero the
/// allocation wrote, and a null handle drops as a no-op.
#[test]
fn instance_that_throws_before_any_field_is_reclaimed() {
    assert_clean(
        "partial_construction_early",
        r#"class Pair {
    var string left
    var array[int] items

    fun init(string l) {
        throw "early"
    }
}

fun main() -> int {
    try {
        var p = Pair("a")
        say("built")
    } catch (e) { say("caught " + e) }
    return 0
}
"#,
        "caught early",
    );
}

/// An object literal is lowered to its JSON text and parsed, so every
/// intermediate concatenation and every converted property value is reclaimed
/// once it has been folded in — including in a loop, on reassignment, and when
/// a throw jumps past the result.
#[test]
fn object_literals_reclaim_their_intermediates() {
    assert_clean(
        "object_literals",
        r#"fun main() -> int {
    var o = {name: "ada", age: 3}
    say(json.get(o, "age"))
    for (var i = 0; i < 3; i = i + 1) {
        var each = {index: i}
        say(json.get(each, "index"))
    }
    var r = {tag: "x"}
    r = {tag: "y"}
    say(json.get(r, "tag"))
    var fresh = {joined: "a" + "b"}
    say(json.get(fresh, "joined"))
    try {
        var thrown = {k: 1}
        throw "boom"
    } catch (e) { say("caught " + e) }
    return 0
}
"#,
        "3\n0\n1\n2\n\"y\"\n\"ab\"\ncaught boom",
    );
}

/// An `object` is a registry handle like a string, so passing one to a
/// function, returning one, and storing one in a field all transfer ownership
/// rather than duplicating or orphaning it.
#[test]
fn objects_crossing_function_boundaries_are_reclaimed() {
    assert_clean(
        "object_boundaries",
        r#"fun tag(object o) -> string { return json.get(o, "t") }

fun make() -> object { return {t: "5"} }

fun main() -> int {
    say(tag({t: "1"}))
    var m = make()
    say(json.get(m, "t"))
    return 0
}
"#,
        "\"1\"\n\"5\"",
    );
}

/// Assigning a field from a read of the same field is a self-assignment: the
/// write must not free the value the read is still using.
#[test]
fn field_self_assignment_keeps_the_value() {
    assert_clean(
        "field_self_assignment",
        r#"class Bag {
    var array[int] items
    var string tag

    fun init() {
        this.items = [1, 2]
        this.tag = "a"
    }
}

fun main() -> int {
    var Bag b = Bag()
    b.items = b.items
    b.tag = b.tag
    say(b.tag + arrays.length(b.items))
    return 0
}
"#,
        "a2",
    );
}

/// Overwriting a container field drops the value it held, including the empty
/// array an `init`-less class default-initializes it with.
#[test]
fn field_overwrite_reclaims_the_previous_value() {
    assert_clean(
        "field_overwrite",
        r#"class Box {
    var array[int] data
    var string label
}

fun main() -> int {
    var b = Box()
    b.data = [1, 2, 3]
    b.data = [4]
    b.label = "first"
    b.label = "second"
    say(b.label + " " + arrays.length(b.data))
    return 0
}
"#,
        "second 1",
    );
}

/// A declaration emits a drop of whatever its slot held, so the slot has to be
/// null before the first one runs. A class slot was left uninitialized: a debug
/// build read the zero a fresh stack page happens to hold, but `mem2reg` turned
/// the pre-store load into poison, the drop thunk's null check folded to
/// "not null", and it released the handles it read out of garbage — which freed
/// a live string the program went on to print.
///
/// Each shape below reads a class slot before its first store: a plain
/// declaration, a redeclaration in a loop body, a redeclaration in a nested
/// block, and a slot whose declaration a branch skips.
#[test]
fn a_class_slot_is_null_before_its_first_store() {
    assert_clean(
        "class_slot_init",
        r#"class Bag {
    var array[int] items
    var string tag

    fun init(array[int] xs, string t) {
        this.items = xs
        this.tag = t
    }
}

fun main() -> int {
    var b = Bag([1, 2, 3], "one")
    say(b.tag + " " + arrays.length(b.items))

    for (var i = 0; i < 2; i = i + 1) {
        var loop_bag = Bag([i], "n" + i)
        say(loop_bag.tag + " " + arrays.length(loop_bag.items))
    }

    if (arrays.length(b.items) > 0) {
        var inner = Bag([9], "inner")
        say(inner.tag + " " + arrays.length(inner.items))
    }

    if (false) {
        var skipped = Bag([0], "never")
        say(skipped.tag)
    }

    say(b.tag)
    return 0
}
"#,
        "one 3\nn0 1\nn1 1\ninner 1\none",
    );
}
