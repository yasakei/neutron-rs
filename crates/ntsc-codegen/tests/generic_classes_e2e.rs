use std::path::Path;

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
            .expect("failed to build ntsc-runtime");
        assert!(status.success(), "failed to build ntsc-runtime");
    }
    lib
}

#[test]
fn generic_classes_and_nested_types_run() {
    let source = r#"
class Box<T> {
    var T value
}

class Pair<T, U> {
    var T first
    var U second
    fun init(T first, U second) {
        this.first = first
        this.second = second
    }
}

fun main() {
    var Pair<int, string> pair = Pair<int, string>(7, "seven")
    var Box<Pair<int, string> > boxed = Box<Pair<int, string> >()
    boxed.value = pair
    say("ok")
}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("generic-classes-e2e");
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "generic_classes",
        &out_dir,
    )
    .expect("generic classes should compile");
    let binary = out_dir.join("generic_classes");
    ntsc_codegen::link_binary(&object, &runtime_lib(rewrite_dir), &binary)
        .expect("generic classes should link");
    let output = std::process::Command::new(binary)
        .output()
        .expect("generic classes should run");
    assert!(
        output.status.success(),
        "program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
}
