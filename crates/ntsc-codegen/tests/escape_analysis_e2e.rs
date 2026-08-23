//! End-to-end regression test for 3.6: escape analysis. Class instances that
//! provably never escape their function are stack-allocated (into a slot with
//! a memset) instead of heap-allocated (`ntsc_alloc`). This test verifies the
//! stack-slot path behaves identically to the heap path: field reads/writes,
//! method calls, variable copies, and constructions inside loops all work.

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

#[test]
fn escape_analysis_stack_slots_are_behaviorally_correct() {
    let source = r#"class Point {
    var int x
    var int y

    fun sum() -> int {
        return this.x + this.y;
    }
}

fun main() {
    var p = Point();
    p.x = 3;
    p.y = 4;
    say("p: " + p.sum());

    var q = p;
    q.x = 10;
    say("q: " + q.x);
    say("p: " + p.x);

    for (var i = 0; i < 5; i = i + 1) {
        var r = Point();
        r.x = i;
        r.y = i * 2;
        say("r" + i + ": " + r.sum());
    }
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_escape_analysis_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join(format!("esc.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("esc_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "esc", &dir)
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
    // Classes use reference semantics, so `q = p` aliases `p` and `q.x = 10`
    // is visible through `p`.
    assert_eq!(
        stdout.trim(),
        "p: 7\nq: 10\np: 10\nr0: 0\nr1: 3\nr2: 6\nr3: 9\nr4: 12\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn escape_analysis_keeps_init_classes_on_the_heap() {
    // A class with an `init` is never stack-allocated (its `init` observes
    // `this`), so it must keep working through the heap path.
    let source = r#"class Counter {
    var int n

    fun init(int start) {
        this.n = start;
    }

    fun bump() {
        this.n = this.n + 1;
    }
}

fun main() {
    var c = Counter(40);
    c.bump();
    say("count: " + c.n);
    c.bump();
    say("count: " + c.n);
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_escape_analysis_heap_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join(format!("esc_heap.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("esc_heap_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "esc_heap", &dir)
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
    assert_eq!(stdout.trim(), "count: 41\ncount: 42\ndone");

    let _ = std::fs::remove_dir_all(&dir);
}
