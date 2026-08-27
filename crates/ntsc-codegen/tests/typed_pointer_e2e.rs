//! End-to-end tests for typed pointers: `own T` allocations, `&T` / `&mut T`
//! references, and `*const T` / `*mut T` raw pointers reached through
//! `memory.raw_address` inside `unsafe`.
//!
//! Each program is compiled and run in a debug build, which enables leak
//! reporting, so an owning allocation that is not reclaimed shows up on
//! stderr.

use std::path::Path;

use ntsc_codegen::{compile_source, link_binary};

fn runtime_lib(rewrite_dir: &Path) -> std::path::PathBuf {
    let lib = rewrite_dir
        .join("target")
        .join("debug")
        .join(ntsc_codegen::runtime_lib_name());
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

fn compile_run(name: &str, source: &str) -> (bool, String, String) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let out_dir = rewrite_dir
        .join("target")
        .join("typed-pointer-e2e")
        .join(name);
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = compile_source(source, ntsc_codegen::host_triple(), name, &out_dir).unwrap();
    let binary = out_dir.join(name);
    link_binary(&object, &runtime_lib(rewrite_dir), &binary).unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn compile_error(name: &str, source: &str) -> String {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let out_dir = rewrite_dir
        .join("target")
        .join("typed-pointer-e2e")
        .join(name);
    std::fs::create_dir_all(&out_dir).unwrap();
    match compile_source(source, ntsc_codegen::host_triple(), name, &out_dir) {
        Ok(_) => panic!("expected `{name}` to be rejected"),
        Err(err) => err.to_string(),
    }
}

/// An owning allocation holds a class instance, a reference reads its field,
/// and a raw pointer taken from a `&mut` field reference writes through it.
#[test]
fn own_reference_and_raw_pointer_round_trip() {
    let source = r#"use memory
class Packet {
    var int id

    fun init(int id) {
        this.id = id
    }
}

fun main() {
    var own Packet packet = alloc(Packet(7))
    var &Packet read = &packet
    say("read: " + read.id)

    unsafe {
        var *mut int raw = memory.raw_address(&mut packet.id)
        *raw = 42
    }

    var &mut Packet write = &mut packet
    say("write: " + write.id)
}
"#;
    let (ok, stdout, stderr) = compile_run("own_ref_raw", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "read: 7\nwrite: 42\n");
    assert!(!stderr.contains("memory leak detected"), "{stderr}");
}

/// A reference to a scalar local is the address of its slot, so a raw write
/// through it is observable in the original variable.
#[test]
fn raw_write_through_a_scalar_reference_is_observable() {
    let source = r#"use memory
fun main() {
    var int value = 5
    unsafe {
        var *mut int raw = memory.raw_address(&mut value)
        say("before: " + *raw)
        *raw = 99
    }
    say("after: " + value)
}
"#;
    let (ok, stdout, stderr) = compile_run("raw_scalar", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "before: 5\nafter: 99\n");
    assert!(!stderr.contains("memory leak detected"), "{stderr}");
}

/// A boxed scalar allocation is reclaimed when its owner leaves scope.
#[test]
fn boxed_scalar_allocation_is_reclaimed() {
    let source = r#"use memory
fun main() {
    var own int boxed = alloc(11)
    unsafe {
        var *const int p = memory.raw_address(&boxed)
        say("boxed: " + *p)
    }
    say("done")
}
"#;
    let (ok, stdout, stderr) = compile_run("boxed_scalar", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "boxed: 11\ndone\n");
    assert!(!stderr.contains("memory leak detected"), "{stderr}");
}

/// Raw dereference outside `unsafe` is a compile error.
#[test]
fn raw_dereference_outside_unsafe_is_rejected() {
    let err = compile_error(
        "raw_outside_unsafe",
        "use memory\nfun main() {\n    var int v = 1\n    var *mut int p = memory.raw_address(&mut v)\n    *p = 2\n}\n",
    );
    assert!(
        err.contains("requires an `unsafe` block"),
        "expected an unsafe-required error, got: {err}"
    );
}

/// A raw pointer keeps its pointee type: a `&mut Packet` cannot be narrowed
/// to a `*mut int`.
#[test]
fn raw_pointer_pointee_type_is_enforced() {
    let err = compile_error(
        "raw_pointee_type",
        "use memory\nclass Packet {\n    var int id\n}\nfun main() {\n    var own Packet p = alloc(Packet())\n    unsafe {\n        var *mut int raw = memory.raw_address(&mut p)\n    }\n}\n",
    );
    assert!(
        err.contains("type mismatch"),
        "expected a pointee-type mismatch, got: {err}"
    );
}

/// Writing through a `*const` pointer is rejected.
#[test]
fn writing_through_a_const_raw_pointer_is_rejected() {
    let err = compile_error(
        "raw_const_write",
        "use memory\nfun main() {\n    var int v = 1\n    unsafe {\n        var *const int p = memory.raw_address(&v)\n        *p = 2\n    }\n}\n",
    );
    assert!(
        err.contains("cannot write through `*const` pointer"),
        "expected a const-write rejection, got: {err}"
    );
}

/// An integer is never silently an address: pointer types are not
/// constructible from arithmetic.
#[test]
fn an_integer_is_not_a_pointer() {
    let err = compile_error(
        "int_is_not_pointer",
        "fun main() {\n    var *mut int p = 4096\n}\n",
    );
    assert!(
        err.contains("type mismatch"),
        "expected an integer-to-pointer rejection, got: {err}"
    );
}

/// A reference may not outlive the value it points at, so it cannot be
/// returned.
#[test]
fn a_reference_cannot_be_returned() {
    let err = compile_error(
        "ref_escapes_return",
        "fun leak() -> &int {\n    var int local = 1\n    return &local\n}\nfun main() {\n    say(\"unused\")\n}\n",
    );
    assert!(
        err.contains("cannot return a borrow"),
        "expected a returned-borrow rejection, got: {err}"
    );
}

/// A reference may not be stored in an owned container, which outlives the
/// borrow.
#[test]
fn a_reference_cannot_be_stored_in_an_array() {
    let err = compile_error(
        "ref_escapes_array",
        "fun main() {\n    var int v = 1\n    var refs = [&v]\n}\n",
    );
    assert!(
        err.contains("cannot store a view in an array"),
        "expected a container-escape rejection, got: {err}"
    );
}

/// An exclusive borrow excludes every other live borrow of the same value.
#[test]
fn conflicting_reference_borrows_are_rejected() {
    let err = compile_error(
        "ref_conflict",
        "fun main() {\n    var xs = [1, 2]\n    var &array[int] r = &xs\n    var &mut array[int] w = &mut xs\n    say(\"\" + r[0] + w[0])\n}\n",
    );
    assert!(
        err.contains("already viewed"),
        "expected a borrow-exclusivity rejection, got: {err}"
    );
}

/// A borrow keeps its referent alive: the owner cannot be moved away while a
/// reference to it is still live.
#[test]
fn moving_a_referenced_owner_is_rejected() {
    let err = compile_error(
        "ref_owner_moved",
        "fun main() {\n    var xs = [1, 2]\n    var &array[int] r = &xs\n    var taken = xs\n    say(\"\" + r[0])\n}\n",
    );
    assert!(
        err.contains("while it is viewed"),
        "expected a move-while-borrowed rejection, got: {err}"
    );
}
