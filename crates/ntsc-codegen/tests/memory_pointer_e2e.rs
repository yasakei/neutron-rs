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
        .join("memory-pointer-e2e")
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

#[test]
fn pointer_capabilities_are_bounds_checked_and_reclaimed() {
    let source = r#"use memory
fun main() {
    var pointer base = memory.alloc(16)
    memory.store64(base, 72623859790382856)
    var pointer next = memory.offset(base, 8)
    memory.store8(next, 255)
    var pointer alias = copy(next)
    say("word=" + memory.load64(base))
    say("byte=" + memory.load8(alias))
}
"#;
    let (ok, stdout, stderr) = compile_run("pointer_capabilities", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "word=72623859790382856\nbyte=255\n");
    assert!(!stderr.contains("memory leak detected"), "{stderr}");
}

#[test]
fn out_of_bounds_pointer_access_throws_without_touching_memory() {
    let source = r#"use memory
fun main() {
    var pointer base = memory.alloc(4)
    try {
        var pointer outside = memory.offset(base, 5)
        say("unreached " + memory.load8(outside))
    } catch (err) {
        say(err)
    }
}
"#;
    let (ok, stdout, stderr) = compile_run("pointer_bounds", source);
    assert!(ok, "program failed: {stderr}");
    assert!(stdout.contains("memory.offset: out of bounds or invalid pointer"));
    assert!(!stderr.contains("memory leak detected"), "{stderr}");
}
