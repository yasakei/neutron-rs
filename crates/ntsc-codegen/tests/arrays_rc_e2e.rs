//! End-to-end regression test for `arrays.*` operations on heap arrays (the
//! runtime backing of array literals).
//!
//! Regression: the legacy `arrays` module used a newline-delimited string ABI
//! incompatible with heap arrays, so `arrays.length` returned 1 and
//! `arrays.push` was a no-op. These operations now route through the array
//! runtime functions. `arrays.push` is an in-place `view mut` operation that
//! returns `void`; the handle is stable (only the element buffer may be
//! relocated internally), so the caller's variable never needs updating.

use std::path::Path;
#[test]
fn arrays_rc_e2e() {
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

    let source = r#"fun fill(view array[int] arr, int n) -> int {
    for (var i = 0; i < n; i = i + 1) {
        arrays.push(arr, i * 10);
    }
    return arrays.length(arr);
}

fun main() {
    var arr = [1];
    say("len0: " + arrays.length(arr));
    say("empty0: " + arrays.isEmpty(arr));
    arrays.push(arr, 2);
    say("len1: " + arrays.length(arr));
    say("at1: " + arrays.at(arr, 1));
    var total = fill(arr, 5);
    say("total: " + total);
    say("first: " + arrays.at(arr, 0));
    say("last: " + arrays.at(arr, 6));
    var sum = 0;
    for (var x in arr) {
        sum = sum + x;
    }
    say("sum: " + sum);
    var names = [];
    arrays.push(names, "a");
    arrays.push(names, "b");
    arrays.push(names, "c");
    say("names: " + arrays.length(names));
    say("name2: " + arrays.at(names, 2));
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_arrays_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("arrays.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("arrays_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "arrays", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "len0: 1\nempty0: false\nlen1: 2\nat1: 2\ntotal: 7\nfirst: 1\nlast: 40\nsum: 103\nnames: 3\nname2: c\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: the remaining `arrays.*` operations (`range`, `fill`, `slice`,
/// `reverse`, `clone`, `sort`, `remove`, `remove_at`, `index_of`, `contains`,
/// `pop`, `join`, `clear`, `shuffle`, `every`, `some`) run against RC heap
/// arrays. Array-returning ops are functional: they return a new array and
/// never mutate the input, so callers reassign (`arr = arrays.sort(arr, 0)`).
/// Also covers:
/// - `every`/`some` with real predicate callbacks whose parameter must be
///   coerced to the array element type (untyped lambdas previously emitted
///   mismatched `ptr`/`i64` calls that failed LLVM verification).
/// - `shuffle` with `j == i` (a swap of an element with itself previously hit
///   `ptr::copy_nonoverlapping` overlap UB and aborted).
#[test]
fn arrays_rc_full_ops_e2e() {
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

    let source = r#"fun main() {
    var a = arrays.new();
    say("a0: " + arrays.length(a));
    arrays.push(a, 10);
    arrays.push(a, 20);
    arrays.push(a, 30);
    say("a1: " + arrays.length(a));

    var r = arrays.range(1, 4);
    say("range: " + r[0] + "," + r[1] + "," + r[2]);

    var f = arrays.fill(7, 3);
    say("fill: " + arrays.length(f) + "," + f[1]);

    var s = arrays.slice(r, 1, 3);
    say("slice: " + arrays.length(s) + "," + s[0]);

    var rev = arrays.reverse(r);
    say("reverse: " + arrays.length(rev) + "," + rev[0]);

    var cl = arrays.clone(r);
    say("clone: " + arrays.length(cl));

    var nums = [3, 1, 2];
    nums = arrays.sort(nums, 0);
    say("sort: " + nums[0] + nums[1] + nums[2]);

    var strarr = ["b", "a", "c"];
    strarr = arrays.sort(strarr, 2);
    say("strsort: " + strarr[0] + strarr[1] + strarr[2]);

    var rm = [1, 2, 3, 4];
    rm = arrays.remove_at(rm, 1);
    say("rm1: " + rm[0] + rm[1] + rm[2]);
    rm = arrays.remove(rm, 3);
    say("rm2: " + rm[0] + rm[1]);
    say("idx: " + arrays.index_of(rm, 2));
    say("has2: " + arrays.contains(rm, 2));
    say("has9: " + arrays.contains(rm, 9));

    var p = [5, 6];
    var last = arrays.pop(p);
    say("pop: " + last + "," + arrays.length(p));

    var j = ["x", "y", "z"];
    say("join: " + arrays.join(j, "-"));

    var ev = [2, 4, 6];
    say("every1: " + arrays.every(ev, fun(int elem) -> bool { return elem > 1; }));
    say("every3: " + arrays.every(ev, fun(int elem) -> bool { return elem > 3; }));
    say("some5: " + arrays.some(ev, fun(int elem) -> bool { return elem > 5; }));
    say("some9: " + arrays.some(ev, fun(int elem) -> bool { return elem > 9; }));

    rm = arrays.clear(rm);
    say("clear: " + arrays.length(rm));

    var sh = [1, 2, 3, 4, 5];
    sh = arrays.shuffle(sh);
    say("shuffle: " + arrays.length(sh));
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_arrays_full_ops_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("arrays.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("arrays_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "arrays", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "a0: 0\na1: 3\nrange: 1,2,3\nfill: 3,7\nslice: 2,2\nreverse: 3,3\nclone: 3\nsort: 123\nstrsort: abc\nrm1: 134\nrm2: 14\nidx: -1\nhas2: false\nhas9: false\npop: 6,1\njoin: x-y-z\nevery1: true\nevery3: false\nsome5: true\nsome9: false\nclear: 0\nshuffle: 5\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
/// type so `for (var x in arr)` over a parameter array types `x` as `int`
/// (previously every `array[...]` annotation became `array[any]`, and `x * 2`
/// failed with "Star on string"). Also covers untyped (`[]`) arrays: scalar
/// elements are coerced to strings at push time, so mixed int/float/bool
/// arrays can be read back, iterated, and concatenated without crashing.
#[test]
fn arrays_rc_untyped_and_param_e2e() {
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

    let source = r#"fun double_all(array[int] xs) -> array[int] {
    // The declared `-> array[int]` return type is honored at the call site, so
    // the output array must actually store raw int elements. An untyped `[]`
    // array stores string pointers and would not match the declared type.
    var array[int] out = [];
    for (var x in xs) {
        arrays.push(out, x * 2);
    }
    return out;
}

fun main() {
    var nums = [1, 2, 3];
    var doubled = double_all(nums);
    say("len: " + arrays.length(doubled));
    say("at1: " + arrays.at(doubled, 1));

    var dyn = [];
    arrays.push(dyn, 7);
    arrays.push(dyn, 2.5);
    arrays.push(dyn, true);
    arrays.push(dyn, "hi");
    say("dyn0: " + dyn[0]);
    say("dyn1: " + arrays.at(dyn, 1));
    say("dyn2: " + arrays.at(dyn, 2));
    say("dyn3: " + arrays.at(dyn, 3));
    var n = 0;
    for (var x in dyn) {
        say("elem: " + x);
        n = n + 1;
    }
    say("count: " + n);
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_arrays_untyped_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("arrays.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("arrays_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "arrays", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "len: 3\nat1: 4\ndyn0: 7\ndyn1: 2.5\ndyn2: true\ndyn3: hi\nelem: 7\nelem: 2.5\nelem: true\nelem: hi\ncount: 4\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: string-elements arrays own their contents. `arrays.push` and
/// index-assignment deep-copy the incoming string, so overwriting or dropping
/// the source variable afterwards must not corrupt or free the element (the
/// previous behavior stored the caller's raw pointer — a use-after-free). All
/// array-producing operations (`clone`, `reverse`, `slice`, `fill`) must
/// propagate the ownership flag and deep-copy elements; dropping the array
/// frees its strings once. `pop` hands ownership of the removed string to the
/// caller.
#[test]
fn string_array_ownership_e2e() {
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

    let source = r#"fun main() {
    // Push a runtime-built string, then overwrite the source: the array must
    // hold its own deep copy (the old source string is freed by reassignment).
    var a = [];
    var s = strings.repeat("x", 3);
    arrays.push(a, s);
    s = "changed";
    say("a0: " + arrays.at(a, 0));
    arrays.push(a, strings.trim("  hi  "));
    arrays.push(a, "lit");
    say("len: " + arrays.length(a));
    say("a1: [" + arrays.at(a, 1) + "]");
    say("a2: " + arrays.at(a, 2));

    var c = arrays.clone(a);
    var r = arrays.reverse(a);
    var sl = arrays.slice(a, 0, 2);
    var f = arrays.fill("z", 2);
    var cp = copy(a);
    say("c0: " + arrays.at(c, 0));
    say("r0: " + arrays.at(r, 0));
    say("sl1: [" + arrays.at(sl, 1) + "]");
    say("f1: " + arrays.at(f, 1));
    say("cp2: " + arrays.at(cp, 2));

    // Index-assign a runtime string, then overwrite the source.
    var g = ["x", "y"];
    var fresh = strings.repeat("q", 2);
    g[1] = fresh;
    fresh = "zz";
    say("g1: " + arrays.at(g, 1));

    // pop transfers ownership of the removed string to the caller.
    var p = ["m", "n"];
    var popped = arrays.pop(p);
    say("popped: " + popped + " len: " + arrays.length(p));

    // Scalar coercion into an untyped array still round-trips.
    var nums = [];
    arrays.push(nums, 5);
    arrays.push(nums, 2.5);
    say("nums0: " + nums[0]);
    say("nums1: " + arrays.at(nums, 1));

    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_string_array_ownership_e2e_test");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("arrays.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("arrays_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "arrays", &dir)
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "a0: xxx\nlen: 3\na1: [hi]\na2: lit\nc0: xxx\nr0: lit\nsl1: [hi]\nf1: z\ncp2: lit\ng1: qq\npopped: n len: 1\nnums0: 5\nnums1: 2.5\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
