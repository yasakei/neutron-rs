//! End-to-end regression test for user functions and methods that return a
//! class. The call site must type the result from the declared return type
//! (`-> Counter`) rather than from the LLVM pointer return type, which would
//! otherwise mis-type the instance as a string.

use std::path::Path;
fn build_runtime() -> std::path::PathBuf {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
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

fn compile_and_run(source: &str, test_id: &str) -> String {
    let dir = std::env::temp_dir().join(format!("ntsc_class_return_{test_id}"));
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join(format!("cls.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("cls_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "cls", &dir)
        .expect("compile_source failed");
    assert!(obj_path.exists(), "object file not produced");

    let runtime_lib = build_runtime();
    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");
    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");
    assert!(output.status.success(), "binary exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    stdout.trim().to_string()
}

#[test]
fn function_returning_class_types_call_result() {
    let source = r#"class Counter {
    var int n

    fun init(int start) {
        this.n = start;
    }

    fun bump() {
        this.n = this.n + 1;
    }
}

fun make() -> Counter {
    var c = Counter(40);
    c.bump();
    return c;
}

fun main() {
    var c = make();
    say("count: " + c.n);
    c.bump();
    say("count: " + c.n);
    say("done")
}
"#;
    assert_eq!(
        compile_and_run(source, "make"),
        "count: 41\ncount: 42\ndone"
    );
}

#[test]
fn method_returning_class_types_call_result() {
    let source = r#"class Point {
    var int x

    fun init(int v) {
        this.x = v;
    }
}

class Factory {
    var int tag

    fun make() -> Point {
        return Point(7);
    }
}

fun main() {
    var f = Factory();
    var p = f.make();
    say("x: " + p.x);
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "factory"), "x: 7\ndone");
}

#[test]
fn returned_instance_supports_iterator_protocol() {
    let source = r#"class Range {
    var int count

    fun init(int n) {
        this.count = n;
    }

    fun length() -> int {
        return this.count;
    }

    fun get(int i) -> int {
        return i * 10;
    }
}

fun build() -> Range {
    return Range(3);
}

fun main() {
    var total = 0;
    for (var v in build()) {
        total = total + v;
    }
    say("total: " + total);
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "forin"), "total: 30\ndone");
}

#[test]
fn primitive_return_types_unaffected() {
    let source = r#"fun add(int a, int b) -> int {
    return a + b;
}

fun greet() -> string {
    return "hi";
}

fun main() {
    say("sum: " + add(2, 3));
    say(greet());
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "primitive"), "sum: 5\nhi\ndone");
}
