//! End-to-end tests for the `shared T` refcounted escape hatch (RFC work
//! item 7.6 / `ownership-rfc.md` §`shared T`):
//!
//! - `shared` values alias by design: assignments, argument passing, and
//!   returns copy the reference (retain) and never move the value.
//! - Mutation through one handle is visible through every other handle.
//! - The last release frees the wrapped value (the runtime drop thunk); shared
//!   programs run leak-free and double-free-free under the leak detector.
//! - `view of shared` borrows the pointee, `copy of shared` deep-copies to an
//!   owned value.
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

fn compile_run(rewrite_dir: &Path, name: &str, source: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("ntsc_shared_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let program = {
        let tokens = ntsc_lexer::tokenize(source);
        ntsc_parser::parse(&tokens).expect("parse failed")
    };
    ntsc_codegen::compile_program(
        &program,
        ntsc_codegen::host_triple(),
        inkwell::OptimizationLevel::None,
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

fn compile_error(_rewrite_dir: &Path, name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("ntsc_shared_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let result = ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), name, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(_) => String::new(),
        Err(e) => format!("{e}"),
    }
}

fn workspace_root() -> std::path::PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

/// `shared` copies the reference: reassigning a shared variable to another
/// shared variable never moves the value, and both handles alias it.
#[test]
fn shared_values_alias_and_are_never_moved() {
    let source = r#"use arrays
fun main() {
    shared array[int] a = [1, 2, 3];
    shared array[int] b = a;
    arrays.push(b, 4);
    say("a_len: " + arrays.length(a));
    say("b_len: " + arrays.length(b));
    say("a3: " + a[3]);
    a = a;
    say("still: " + arrays.length(a));
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "alias", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "a_len: 4\nb_len: 4\na3: 4\nstill: 4\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// A shared array is borrowed (not boxed) by `arrays.*`: in-place ops mutate
/// the wrapped array and functional ops read it without consuming it.
#[test]
fn shared_array_works_with_arrays_module() {
    let source = r#"use arrays
fun main() {
    shared array[int] s = [5, 1, 3];
    arrays.push(s, 9);
    say("len: " + arrays.length(s));
    var sorted = arrays.sort(s);
    say("sorted0: " + sorted[0]);
    say("sorted3: " + sorted[3]);
    say("orig_len: " + arrays.length(s));
    var last = arrays.pop(s);
    say("last: " + last);
    say("after_pop: " + arrays.length(s));
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "arrays", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(
        stdout.trim(),
        "len: 4\nsorted0: 1\nsorted3: 9\norig_len: 4\nlast: 9\nafter_pop: 3\ndone"
    );
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// `view of shared` borrows the wrapped value: the view sees mutations made
/// through the shared handle and does not own anything.
#[test]
fn view_of_shared_borrows_the_pointee() {
    let source = r#"use arrays
fun read(view array[int] v) {
    say("v0: " + v[0])
}
fun main() {
    shared array[int] s = [10, 20];
    var view array[int] v = s;
    arrays.push(s, 30);
    say("via_view: " + v[2]);
    read(v);
    read(s);
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "view", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "via_view: 30\nv0: 10\nv0: 10\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// `copy of shared` deep-copies the wrapped value to a new owned value: the
/// copy is independent of the shared handle.
#[test]
fn copy_of_shared_deep_copies_to_owned() {
    let source = r#"use arrays
fun main() {
    shared array[int] s = [1, 2];
    var c = copy(s);
    arrays.push(c, 99);
    say("c_len: " + arrays.length(c));
    say("s_len: " + arrays.length(s));
    shared string g = "hello";
    var gcopy = copy(g);
    say(gcopy);
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "copy", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "c_len: 3\ns_len: 2\nhello\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// Shared values cross function boundaries by retain: a shared argument stays
/// usable after the call, and a shared return is safe to drop.
#[test]
fn shared_parameters_and_returns_retain() {
    let source = r#"use arrays
fun bump(shared array[int] a) {
    arrays.push(a, 7)
}
fun make() -> shared array[int] {
    return [1, 2]
}
fun main() {
    shared array[int] x = [0];
    bump(x);
    bump(x);
    say("len: " + arrays.length(x));
    var fresh = make();
    bump(fresh);
    say("fresh_len: " + arrays.length(fresh));
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "params", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "len: 3\nfresh_len: 3\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// An owned value adopted into a shared slot is boxed (and moved, if it was a
/// bare variable). A shared string initialized from a literal boxes an owned
/// copy of the literal.
#[test]
fn owned_values_are_adopted_into_shared() {
    let source = r#"use arrays
fun main() {
    var arr = [1, 2];
    shared array[int] s = arr;
    arrays.push(s, 3);
    say("s_len: " + arrays.length(s));
    shared string g = "world";
    say(copy(g));
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "adopt", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "s_len: 3\nworld\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// `shared` requires a heap type: wrapping a scalar is rejected at type check.
#[test]
fn shared_of_scalar_is_rejected() {
    let source = r#"fun main() {
    shared int x = 5;
    say("unreachable")
}
"#;
    let err = compile_error(&workspace_root(), "scalar", source);
    assert!(
        err.contains("shared") && err.contains("heap"),
        "expected a 'shared requires a heap type' error, got: {err}"
    );
}

/// Arrays of `shared T` copy the reference on insert: each holder retains
/// its own copy, and releasing every reference (array teardown included)
/// frees the box exactly once.
#[test]
fn shared_arrays_retain_on_insert_and_release_on_drop() {
    let source = r#"use arrays
fun main() {
    shared string s1 = "a";
    shared string s2 = "bb";
    var a = [s1, s2];
    arrays.push(a, s1);
    var b = arrays.remove_at(a, 0);
    var c = arrays.clone(b);
    var count = 0;
    for (var s in c) {
        count = count + 1;
    }
    say("count: " + count);
    say("a_len: " + arrays.length(a));
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "shared_arr", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(stdout.trim(), "count: 2\na_len: 3\ndone");
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}

/// Arrays of `option[T]` copy the cell on insert and reclaim it on
/// teardown: the array owns its own copy while the source keeps its own.
/// Overwriting, popping, and functional copies must not double-free or
/// corrupt the cells.
#[test]
fn option_arrays_copy_cells_in_on_insert() {
    let source = r#"use arrays
fun main() {
    var o1 = 5;
    var o2 = 7;
    var oa = [o1, o2];
    arrays.push(oa, o1);
    oa[1] = o2;
    var popped = arrays.pop(oa);
    var b = arrays.remove_at(oa, 0);
    var c = arrays.clone(b);
    var d = arrays.slice(c, 0, 1);
    var e = arrays.sort(d);
    var f = arrays.reverse(e);
    var total = 0;
    for (var x in f) {
        total = total + x;
    }
    say("popped: " + popped);
    say("total: " + total);
    say("o1 still: " + o1);
    say("o2 still: " + o2);
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run(&workspace_root(), "option_arr", source);
    assert!(ok, "binary must exit 0, stderr: {stderr}");
    assert_eq!(
        stdout.trim(),
        "popped: 5\ntotal: 7\no1 still: 5\no2 still: 7\ndone"
    );
    assert!(
        !stderr.contains("memory leak detected"),
        "no leaks expected: {stderr}"
    );
}
