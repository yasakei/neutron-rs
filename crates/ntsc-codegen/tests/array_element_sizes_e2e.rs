//! End-to-end regression test for 4.2: `ntsc_array_get` element sizes for
//! elements that are not pointers. The runtime stores raw element bytes
//! (`elem_size` bytes each) and codegen must load the correct typed width.
//! `array[int]`/`array[float]` store 8-byte elements and `array[bool]` stores
//! 1-byte elements, exercised through index access, `arrays.at`, and for-in.

use std::path::Path;
#[test]
fn typed_float_and_bool_array_elements_e2e() {
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
    var fs = [1.5, 2.5, 3.5];
    say("f0: " + fs[0]);
    say("f1: " + arrays.at(fs, 1));
    var fsum = 0.0;
    for (var x in fs) {
        fsum = fsum + x;
    }
    say("fsum: " + fsum);
    fs[2] = 9.5;
    say("fset: " + fs[2]);

    var bs = [true, false, true];
    say("b0: " + bs[0]);
    say("b1: " + arrays.at(bs, 1));
    var bcount = 0;
    for (var x in bs) {
        if (x) {
            bcount = bcount + 1;
        }
    }
    say("bcount: " + bcount);
    bs[1] = true;
    say("bset: " + bs[1]);

    var is = [10, 20, 30];
    var itotal = 0;
    for (var x in is) {
        itotal = itotal + x;
    }
    say("isum: " + itotal);
    say("idiff: " + (is[2] - is[0]));
    say("done")
}
"#;

    let dir = std::env::temp_dir().join("ntsc_array_elem_size_e2e");
    std::fs::create_dir_all(&dir).unwrap();

    let obj_path = dir.join(format!("elem.{}", ntsc_codegen::object_extension()));
    let bin_path = dir.join("elem_ntsc_test");

    ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "elem", &dir)
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
        "f0: 1.5\nf1: 2.5\nfsum: 7.5\nfset: 9.5\nb0: true\nb1: false\nbcount: 2\nbset: true\nisum: 60\nidiff: 20\ndone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
