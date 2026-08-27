//! End-to-end test: aliased file imports (`use "file.nt" as arm`) namespace
//! the imported module's symbols. Callers reference them as `arm.symbol()`,
//! while a module's own functions call each other by their unprefixed names.
//! Two aliased modules may share symbol names without colliding, and classes
//! are constructible through the alias.

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
        let dir = std::env::temp_dir().join(format!("ntsc_alias_e2e_{name}"));
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
fn aliased_module_namespaces_symbols() {
    let project = Project::new("namespaces");
    project.write(
        "src/main.nt",
        r#"use "math.nt" as math
use "strings.nt" as str

fun main() {
    say("fact(5): " + math.fact(5))
    say("math.greet: " + math.greet("world"))
    say("str.greet: " + str.greet("world"))
    var box = math.Box(5)
    box.bump()
    say("box.count: " + box.count())
    say("done")
}
"#,
    );
    // Both modules define `greet` and both call an internal helper by its own
    // name; namespacing keeps them isolated while internal calls stay bare.
    project.write(
        "src/math.nt",
        r#"fun fact(int n) -> int {
    if (n <= 1) {
        return 1;
    }
    return n * fact(n - 1)
}

fun greet(string name) -> string {
    return "hello " + decorate(name)
}

fun decorate(string name) -> string {
    return name + "!"
}

class Box {
    var int n

    fun init(int start) {
        this.n = start
    }

    fun bump() {
        this.n = this.n + 1
    }

    fun count() -> int {
        return this.n
    }
}
"#,
    );
    project.write(
        "src/strings.nt",
        r#"fun greet(string name) -> string {
    return "hi " + decorate(name)
}

fun decorate(string name) -> string {
    return name + "?"
}
"#,
    );

    let runtime_lib = build_runtime();
    let obj_path = project
        .dir
        .join(format!("app.{}", ntsc_codegen::object_extension()));
    let bin_path = project.dir.join("app");

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
    assert_eq!(
        stdout.trim(),
        "fact(5): 120\nmath.greet: hello world!\nstr.greet: hi world?\nbox.count: 6\ndone"
    );
}
