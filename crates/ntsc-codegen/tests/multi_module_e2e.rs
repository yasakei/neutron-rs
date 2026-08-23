//! End-to-end test: compile a multi-module NTSC program (`use "file.nt"`)
//! into a native binary and run it. Covers cross-module function calls,
//! forward references (a module using a function defined later in itself),
//! and a shared class passed across modules.

use inkwell::OptimizationLevel;

use std::path::Path;
fn build_runtime() -> std::path::PathBuf {
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
    runtime_lib
}

struct Project {
    dir: std::path::PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ntsc_modules_e2e_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        Self { dir }
    }

    fn write(&self, rel: &str, contents: &str) {
        std::fs::write(self.dir.join(rel), contents).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn multi_module_e2e() {
    let project = Project::new("multi_module");
    project.write(
        "src/main.nt",
        r#"use "lib.nt"

fun main() {
    var box = Box(5)
    say("answer: " + lib_value())
    say("box: " + box.get())
}
"#,
    );
    project.write(
        "src/lib.nt",
        r#"use "util.nt"

fun lib_value() -> int {
    return util_value() * 2 + later();
}

fun later() -> int {
    return 0;
}
"#,
    );
    project.write(
        "src/util.nt",
        r#"fun util_value() -> int {
    return 21;
}

class Box {
    var int value

    fun init(int value) {
        this.value = value;
    }

    fun get() -> int {
        return this.value;
    }
}
"#,
    );

    let runtime_lib = build_runtime();
    let obj_path = project
        .dir
        .join(format!("app.{}", ntsc_codegen::object_extension()));
    let bin_path = project.dir.join("app");

    // Load the module closure and compile the merged program.
    let loaded = ntsc_build::modules::load_program(&project.dir.join("src/main.nt"))
        .expect("load_program failed");
    assert_eq!(loaded.modules.len(), 3);
    ntsc_codegen::compile_program(
        &loaded.program,
        ntsc_codegen::host_triple(),
        OptimizationLevel::None,
        "app",
        &project.dir,
        false,
    )
    .expect("compile_program failed");
    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");
    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");
    assert!(output.status.success(), "binary exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "answer: 42\nbox: 5");
}

#[test]
fn multi_module_iterator_protocol() {
    let project = Project::new("iterator_module");
    project.write(
        "src/main.nt",
        r#"use "lib.nt"

fun main() {
    var total = 0;
    for (var n in Range(4)) {
        total = total + n;
    }
    say("range: " + total);
    say("done")
}
"#,
    );
    project.write(
        "src/lib.nt",
        r#"class Range {
    var int limit

    fun init(int n) {
        this.limit = n;
    }

    fun length() -> int {
        return this.limit;
    }

    fun get(int i) -> int {
        return i + 1;
    }
}
"#,
    );

    let runtime_lib = build_runtime();
    let obj_path = project
        .dir
        .join(format!("app.{}", ntsc_codegen::object_extension()));
    let bin_path = project.dir.join("app");

    // Load the module closure and compile the merged program.
    let loaded = ntsc_build::modules::load_program(&project.dir.join("src/main.nt"))
        .expect("load_program failed");
    assert_eq!(loaded.modules.len(), 2);
    ntsc_codegen::compile_program(
        &loaded.program,
        ntsc_codegen::host_triple(),
        OptimizationLevel::None,
        "app",
        &project.dir,
        false,
    )
    .expect("compile_program failed");
    assert!(obj_path.exists(), "object file not produced");

    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &bin_path).expect("link_binary failed");
    assert!(bin_path.exists(), "binary not produced");

    let output = std::process::Command::new(&bin_path)
        .output()
        .expect("failed to run binary");
    assert!(output.status.success(), "binary exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // 1 + 2 + 3 + 4 = 10 via the imported Range class's length()/get(i).
    assert_eq!(stdout.trim(), "range: 10\ndone");
}
