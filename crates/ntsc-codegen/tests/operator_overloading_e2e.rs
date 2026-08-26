//! End-to-end tests for operator overloading on custom types.

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
    let dir = std::env::temp_dir().join(format!("ntsc_op_overload_{test_id}"));
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join(format!("op.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("op_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "op", &dir)
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Memory leak"),
        "registry objects leaked:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    stdout.trim().to_string()
}

#[test]
fn vec_addition() {
    let source = r#"class Vec {
    var float x
    var float y

    fun init(float x, float y) {
        this.x = x;
        this.y = y;
    }

    fun +(view Vec other) -> Vec {
        return Vec(this.x + other.x, this.y + other.y);
    }
}

fun main() {
    var a = Vec(1.0, 2.0);
    var b = Vec(3.0, 4.0);
    var c = a + b;
    say("" + c.x);
    say("" + c.y);
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "vec_add"), "4\n6\ndone");
}

#[test]
fn vec_multiple_operators() {
    let source = r#"class Vec {
    var float x
    var float y

    fun init(float x, float y) {
        this.x = x;
        this.y = y;
    }

    fun +(view Vec other) -> Vec {
        return Vec(this.x + other.x, this.y + other.y);
    }

    fun -(view Vec other) -> Vec {
        return Vec(this.x - other.x, this.y - other.y);
    }

    fun *(float scalar) -> Vec {
        return Vec(this.x * scalar, this.y * scalar);
    }
}

fun main() {
    var a = Vec(10.0, 20.0);
    var b = Vec(3.0, 4.0);
    var c = a + b;
    var d = a - b;
    var e = a * 2.0;
    say("" + c.x + "," + c.y);
    say("" + d.x + "," + d.y);
    say("" + e.x + "," + e.y);
    say("done")
}
"#;
    assert_eq!(
        compile_and_run(source, "vec_multi"),
        "13,24\n7,16\n20,40\ndone"
    );
}

#[test]
fn class_equality() {
    let source = r#"class Point {
    var int x
    var int y

    fun init(int x, int y) {
        this.x = x;
        this.y = y;
    }

    fun ==(view Point other) -> bool {
        return this.x == other.x && this.y == other.y;
    }

    fun !=(view Point other) -> bool {
        return this.x != other.x || this.y != other.y;
    }
}

fun main() {
    var a = Point(1, 2);
    var b = Point(1, 2);
    var c = Point(3, 4);
    say("" + (a == b));
    say("" + (a == c));
    say("" + (a != c));
    say("" + (a != b));
    say("done")
}
"#;
    assert_eq!(
        compile_and_run(source, "class_eq"),
        "true\nfalse\ntrue\nfalse\ndone"
    );
}

#[test]
fn unary_negate() {
    let source = r#"class Vec {
    var float x
    var float y

    fun init(float x, float y) {
        this.x = x;
        this.y = y;
    }

    fun -() -> Vec {
        return Vec(-this.x, -this.y);
    }
}

fun main() {
    var a = Vec(3.0, -4.0);
    var b = -a;
    say("" + b.x);
    say("" + b.y);
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "unary_neg"), "-3\n4\ndone");
}

#[test]
fn class_comparison_for_sorting() {
    let source = r#"class Score {
    var int value

    fun init(int v) {
        this.value = v;
    }

    fun <(view Score other) -> bool {
        return this.value < other.value;
    }

    fun >(view Score other) -> bool {
        return this.value > other.value;
    }

    fun ==(view Score other) -> bool {
        return this.value == other.value;
    }
}

fun main() {
    var a = Score(5);
    var b = Score(3);
    var c = Score(7);
    say("" + (a > b));
    say("" + (a < c));
    say("" + (a == Score(5)));
    say("" + (a == b));
    say("done")
}
"#;
    assert_eq!(
        compile_and_run(source, "class_cmp"),
        "true\ntrue\ntrue\nfalse\ndone"
    );
}

#[test]
fn chaining_operators() {
    let source = r#"class Vec {
    var float x
    var float y

    fun init(float x, float y) {
        this.x = x;
        this.y = y;
    }

    fun +(view Vec other) -> Vec {
        return Vec(this.x + other.x, this.y + other.y);
    }

    fun *(float scalar) -> Vec {
        return Vec(this.x * scalar, this.y * scalar);
    }
}

fun main() {
    var a = Vec(1.0, 2.0);
    var b = Vec(3.0, 4.0);
    var c = a + b * 2.0;
    say("" + c.x);
    say("" + c.y);
    say("done")
}
"#;
    assert_eq!(compile_and_run(source, "chaining"), "7\n10\ndone");
}

#[test]
fn sort_with_custom_comparison() {
    let source = r#"class Score {
    var int value

    fun init(int v) {
        this.value = v;
    }

    fun <(view Score other) -> bool {
        return this.value < other.value;
    }
}

fun main() {
    var a = Score(5);
    var b = Score(2);
    var c = Score(8);
    var d = Score(1);
    say("" + (a < b));
    say("" + (b < a));
    say("" + (a < c));
    say("" + (c < a));
    say("done")
}
"#;
    assert_eq!(
        compile_and_run(source, "sort_custom"),
        "false\ntrue\ntrue\nfalse\ndone"
    );
}
