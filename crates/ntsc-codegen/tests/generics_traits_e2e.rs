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
fn generic_functions_and_trait_impls_run() {
    let source = r#"
trait Printable {
    fun format() -> string
}

class User {
    var int id
    fun init() {}
}

impl Printable for User {
    fun format() -> string {
        return "user"
    }
}

fun identity<T>(T value) -> T {
    return value
}

fun show<T: Printable>(view T value) {
    say(value.format())
}

fun main() {
    var int answer = identity(41)
    var User user = User()
    show(user)
    say("answer=" + answer)
}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("generics-traits-e2e");
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "generics_traits",
        &out_dir,
    )
    .expect("generic and trait program should compile");
    let binary = out_dir.join("generics_traits");
    ntsc_codegen::link_binary(&object, &runtime_lib(rewrite_dir), &binary)
        .expect("link should succeed");
    let output = std::process::Command::new(binary)
        .output()
        .expect("run should succeed");
    assert!(
        output.status.success(),
        "program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "user\nanswer=41\n");
}

#[test]
fn generic_trait_bound_reports_missing_impl() {
    let source = r#"
trait Printable { fun format() -> string }
class User {
    var int id
    fun init() {}
}
fun show<T: Printable>(view T value) { say(value.format()) }
fun main() {
    var User user = User()
    show(user)
}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("generics-traits-errors");
    std::fs::create_dir_all(&out_dir).unwrap();
    let error = ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "missing_impl",
        &out_dir,
    )
    .expect_err("missing trait implementation should fail");
    assert!(
        error
            .to_string()
            .contains("does not implement trait `Printable`")
    );
}

#[test]
fn associated_types_specialize_in_generic_returns() {
    let source = r#"
trait Producer {
    type Item
    fun item() -> Item
}

class User {
    var int id
    fun init() {}
}

impl Producer for User {
    type Item = int
    fun item() -> int { return 7 }
}

fun read<T: Producer>(view T value) -> T::Item {
    return value.item()
}

fun main() {
    var User user = User()
    var int total = read(user)
    say("total=" + total)
}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("associated-types-e2e");
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "associated_types",
        &out_dir,
    )
    .expect("associated type program should compile");
    let binary = out_dir.join("associated_types");
    ntsc_codegen::link_binary(&object, &runtime_lib(rewrite_dir), &binary)
        .expect("link should succeed");
    let output = std::process::Command::new(binary)
        .output()
        .expect("run should succeed");
    assert!(
        output.status.success(),
        "program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "total=7\n");
}

#[test]
fn dyn_dispatch_runs_and_drops() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Dog {
    fun init() {}
}

impl Speaker for Dog {
    fun speak() -> string {
        return "woof"
    }
}

class Cat {
    fun init() {}
}

impl Speaker for Cat {
    fun speak() -> string {
        return "meow"
    }
}

fun main() {
    var dyn Speaker voice = Dog()
    say(voice.speak())
    voice = Cat()
    say(voice.speak())
}
"#;
    run_program(source, "dyn_dispatch", &["woof", "meow"]);
}

#[test]
fn dyn_parameter_adopts_fresh_instance() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Robot {
    var int id
    fun init() {}
}

impl Speaker for Robot {
    fun speak() -> string {
        return "beep"
    }
}

fun perform(dyn Speaker actor) {
    say(actor.speak())
}

fun main() {
    perform(Robot())
}
"#;
    run_program(source, "dyn_param", &["beep"]);
}

#[test]
fn own_dyn_moves_through_return_and_slot() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Bird {
    var int id
    fun init() {}
}

impl Speaker for Bird {
    fun speak() -> string {
        return "chirp"
    }
}

fun make() -> own dyn Speaker {
    return Bird()
}

fun main() {
    var own dyn Speaker speaker = make()
    say(speaker.speak())
    speaker = Bird()
    say(speaker.speak())
}
"#;
    run_program(source, "own_dyn", &["chirp", "chirp"]);
}

#[test]
fn default_method_bodies_are_inherited() {
    let source = r#"
trait Greeter {
    fun greet() -> string {
        return "hello default"
    }
    fun name() -> string
}

class Formal {
    fun init() {}
}

impl Greeter for Formal {
    fun name() -> string {
        return "formal"
    }
}

class Casual {
    fun init() {}
}

impl Greeter for Casual {
    fun name() -> string {
        return "casual"
    }
    fun greet() -> string {
        return "hey"
    }
}

fun main() {
    var Formal f = Formal()
    var Casual c = Casual()
    say(f.greet())
    say(c.greet())
}
"#;
    run_program(source, "default_methods", &["hello default", "hey"]);
}

#[test]
fn supertraits_register_impls_for_ancestors() {
    let source = r#"
trait Named {
    fun name() -> string
}

trait Speaker: Named {
    fun speak() -> string
}

class Duck {
    fun init() {}
}

impl Speaker for Duck {
    fun name() -> string {
        return "duck"
    }
    fun speak() -> string {
        return "quack"
    }
}

fun main() {
    var dyn Named n = Duck()
    say(n.name())
    var dyn Speaker s = Duck()
    say(s.speak())
    say(s.name())
}
"#;
    run_program(source, "supertraits", &["duck", "quack", "duck"]);
}

#[test]
fn impl_trait_return_resolves_to_concrete_class() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Dog {
    fun init() {}
}

impl Speaker for Dog {
    fun speak() -> string {
        return "woof"
    }
}

class Cat {
    fun init() {}
}

impl Speaker for Cat {
    fun speak() -> string {
        return "meow"
    }
}

fun loud() -> impl Speaker {
    return Dog()
}

fun main() {
    var speaker = loud()
    say(speaker.speak())
}
"#;
    run_program(source, "rpit", &["woof"]);
}

#[test]
fn rpit_without_matching_impl_is_rejected() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Rock {
    fun init() {}
}

fun hush() -> impl Speaker {
    return Rock()
}

fun main() {}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("rpit-errors");
    std::fs::create_dir_all(&out_dir).unwrap();
    let error = ntsc_codegen::compile_source(
        source,
        ntsc_codegen::host_triple(),
        "rpit_no_impl",
        &out_dir,
    )
    .expect_err("returning a non-implementing class as `impl Trait` should fail");
    assert!(error.to_string().contains("does not implement"));
}

#[test]
fn dyn_coercion_without_impl_is_rejected() {
    let source = r#"
trait Speaker {
    fun speak() -> string
}

class Rock {
    fun init() {}
}

fun main() {
    var dyn Speaker silent = Rock()
}
"#;
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join("dyn-errors");
    std::fs::create_dir_all(&out_dir).unwrap();
    let error =
        ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), "dyn_no_impl", &out_dir)
            .expect_err("coercing a non-implementing class to `dyn` should fail");
    assert!(
        error.to_string().contains("dyn Speaker"),
        "unexpected error: {error}"
    );
}

/// Compile, link, and run a program; every printed line must match the
/// expected output in order.
fn run_program(source: &str, name: &str, expected_lines: &[&str]) {
    let rewrite_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out_dir = rewrite_dir.join("target").join(format!("{name}-e2e"));
    std::fs::create_dir_all(&out_dir).unwrap();
    let object = ntsc_codegen::compile_source(source, ntsc_codegen::host_triple(), name, &out_dir)
        .expect("program should compile");
    let binary = out_dir.join(name);
    ntsc_codegen::link_binary(&object, &runtime_lib(rewrite_dir), &binary)
        .expect("link should succeed");
    let output = std::process::Command::new(binary)
        .output()
        .expect("run should succeed");
    assert!(
        output.status.success(),
        "program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!("{}\n", expected_lines.join("\n"));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
