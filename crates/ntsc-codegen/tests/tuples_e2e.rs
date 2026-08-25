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
            .expect("failed to run cargo");
        assert!(status.success(), "failed to build ntsc-runtime");
    }
    assert!(lib.exists(), "runtime lib not found at {lib:?}");
    lib
}

fn compile_run(name: &str, source: &str) -> (bool, String, String) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rewrite_dir = workspace.parent().unwrap().parent().unwrap();
    let out_dir = rewrite_dir.join("target").join("tuples-e2e").join(name);
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), name, &out_dir)
        .expect("compile failed");
    let binary = out_dir.join(name);
    ntsc_codegen::link_binary(&object, &runtime_lib(rewrite_dir), &binary).unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn tuple_literal_and_index() {
    let source = r#"fun main() {
    var t = (10, 20)
    say("" + t.0)
    say("" + t.1)
}
"#;
    let (ok, stdout, stderr) = compile_run("tuple_literal_index", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "10\n20\n");
}

#[test]
fn tuple_destructure() {
    let source = r#"fun main() {
    var t = (42, "hello")
    var (a, b) = t
    say("" + a)
    say(b)
}
"#;
    let (ok, stdout, stderr) = compile_run("tuple_destructure", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "42\nhello\n");
}

#[test]
fn tuple_multi_return() {
    let source = r#"fun bounds() -> (int, int) {
    return (100, 200)
}

fun main() {
    var (w, h) = bounds()
    say("" + w)
    say("" + h)
}
"#;
    let (ok, stdout, stderr) = compile_run("tuple_multi_return", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "100\n200\n");
}

#[test]
fn tuple_index_into_result_of_function() {
    let source = r#"fun pair() -> (int, int) {
    return (7, 14)
}

fun main() {
    var t = pair()
    say("" + t.0)
    say("" + t.1)
}
"#;
    let (ok, stdout, stderr) = compile_run("tuple_index_func", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "7\n14\n");
}

#[test]
fn tuple_with_strings() {
    let source = r#"fun main() {
    var t = ("alice", "bob")
    var (a, b) = t
    say(a + " and " + b)
}
"#;
    let (ok, stdout, stderr) = compile_run("tuple_strings", source);
    assert!(ok, "program failed: {stderr}");
    assert_eq!(stdout, "alice and bob\n");
}
