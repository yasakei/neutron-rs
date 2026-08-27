use std::env;
use std::fs;
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use inkwell::OptimizationLevel;
use ntsc_codegen::SUMMARY_MARKER;
use ntsc_diag::{DiagConfig, Diagnostic, SourceBuffer, SourceMap, Writer, diagnostics_to_json};

// Project version in the 26.0 scheme: <year>.<release>.<patch><stage>,
// where `a` = alpha, `b` = beta, and stable releases omit the suffix.
// Cargo cannot represent this, so it is defined here rather than derived
// from CARGO_PKG_VERSION (which carries the semver projection "26.0.0").
pub const NTSC_VERSION: &str = "26.0.0b";

// ANSI escape codes for the CLI's own output. Follows the same colour
// conventions as ntsc-diag: CLICOLOR_FORCE, NO_COLOR, CLICOLOR=0, TERM=dumb.
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[1;36m";
const GREEN: &str = "\x1b[1;32m";
const RESET: &str = "\x1b[0m";

/// Whether ANSI colours should be emitted for a given stream.
fn colour_on(stream_is_tty: bool) -> bool {
    let force = env::var("CLICOLOR_FORCE")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);
    let no_color = env::var("NO_COLOR").map(|v| !v.is_empty()).unwrap_or(false);
    let clicolor_off = env::var("CLICOLOR").map(|v| v == "0").unwrap_or(false);
    let dumb_term = env::var("TERM").map(|v| v == "dumb").unwrap_or(false);
    force || (!no_color && !clicolor_off && !dumb_term && stream_is_tty)
}

/// Wrap `text` in an ANSI escape code when colour is enabled.
fn paint(text: &str, code: &str, colour: bool) -> String {
    if colour {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

fn main() {
    let (command, opts) = match parse_args() {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("error: {msg}");
            print_usage();
            std::process::exit(1);
        }
    };

    let result = match command {
        Command::Help => {
            print_usage();
            Ok(())
        }
        Command::Version => {
            print_version();
            Ok(())
        }
        Command::Init(name) => cmd_init(name.as_deref()).map(|_| ()),
        Command::Build => cmd_build(opts.release, false, opts.json).map(|_| ()),
        Command::Test => cmd_test(opts.release, opts.json).map(|_| ()),
        Command::Run => cmd_run(opts.release, opts.json).map(|_| ()),
        Command::Clean => cmd_clean(),
        Command::Watch => cmd_watch(opts.release, opts.json),
        Command::Graph => cmd_graph().map(|_| ()),
        Command::Pkg(args) => cmd_pkg(&args),
    };

    if let Err(e) = result {
        report(e, opts.json);
        std::process::exit(1);
    }
}

/// Command-line options shared by every subcommand.
#[derive(Clone, Copy, Default)]
struct CliOptions {
    release: bool,
    json: bool,
}

/// An error that failed at the CLI level. Either a batch of renderable
/// diagnostics (with their source files) or a plain message for
/// infrastructure failures that have no source location.
enum CliError {
    Diagnostics {
        diags: Vec<Diagnostic>,
        sources: SourceMap,
    },
    Plain(String),
}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self::Plain(message)
    }
}

impl From<Box<dyn std::error::Error>> for CliError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self::Plain(error.to_string())
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::Plain(error.to_string())
    }
}

impl From<ntsc_codegen::CodegenError> for CliError {
    fn from(error: ntsc_codegen::CodegenError) -> Self {
        Self::Diagnostics {
            diags: error.into_diagnostics(),
            sources: SourceMap::new(),
        }
    }
}

/// Print a CLI error: diagnostics through the writer, plain messages as
/// text.
///
/// With `--json`, emit a machine-readable JSON document on stdout
/// instead.
fn report(error: CliError, json: bool) {
    match error {
        CliError::Diagnostics { diags, sources } => {
            if json {
                println!("{}", diagnostics_to_json(&diags));
            } else {
                let writer = Writer::new(DiagConfig::from_env());
                writer.emit_all(&diags, Some(&sources));
            }
        }
        CliError::Plain(msg) => {
            if json {
                println!("{}", diagnostics_to_json(&[Diagnostic::error(msg)]));
            } else {
                eprintln!("error: {msg}");
            }
        }
    }
}

enum Command {
    Help,
    Version,
    Init(Option<String>),
    Build,
    Test,
    Run,
    Clean,
    Watch,
    Graph,
    Pkg(Vec<String>),
}

/// Parse the command line, accepting flags (`--release`, `--debug`,
/// `--json`, `--help`) before or after the subcommand.
fn parse_args() -> Result<(Command, CliOptions), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = CliOptions::default();
    let mut command = None;
    let mut init_name = None;
    let mut pkg_args = Vec::new();

    for arg in args {
        if command.as_deref() == Some("pkg") {
            pkg_args.push(arg);
            continue;
        }
        match arg.as_str() {
            "--release" => opts.release = true,
            "--debug" => opts.release = false,
            "--json" => opts.json = true,
            "--help" | "-h" => return Ok((Command::Help, opts)),
            "--version" | "-V" => return Ok((Command::Version, opts)),
            "version" => return Ok((Command::Version, opts)),
            "init" | "build" | "test" | "run" | "clean" | "watch" | "graph" | "pkg" => {
                if command.is_some() {
                    return Err(format!("unexpected argument `{arg}`"));
                }
                command = Some(arg);
            }
            _ if command.as_deref() == Some("init") && init_name.is_none() => {
                init_name = Some(arg);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }

    let command = match command {
        Some(c) => c,
        None => return Err("missing command".to_string()),
    };
    let command = match command.as_str() {
        "init" => Command::Init(init_name),
        "build" => Command::Build,
        "test" => Command::Test,
        "run" => Command::Run,
        "clean" => Command::Clean,
        "watch" => Command::Watch,
        "graph" => Command::Graph,
        "pkg" => Command::Pkg(pkg_args),
        _ => unreachable!("validated above"),
    };
    Ok((command, opts))
}

fn print_usage() {
    let colour = colour_on(std::io::stderr().is_terminal());
    let bold = |text: &str| paint(text, BOLD, colour);
    let cell = |name: &str| paint(&format!("{name:<16}"), CYAN, colour);
    let row = |name: &str, desc: &str| format!("  {}{desc}", cell(name));

    eprintln!("ntsc — Neutron Type-Safe Compiler {}", bold(NTSC_VERSION));
    eprintln!();
    eprintln!("A statically typed, memory-safe systems language compiled to native");
    eprintln!("binaries through LLVM.");
    eprintln!();
    eprintln!("Usage: ntsc <command> [options]");
    eprintln!();
    eprintln!("{}", bold("Commands:"));
    eprintln!("{}", row("init [name]", "Scaffold a new project"));
    eprintln!(
        "{}",
        row("build", "Compile to build/debug/ or build/release/")
    );
    eprintln!("{}", row("test", "Discover and run `test` blocks"));
    eprintln!("{}", row("run", "Build and execute"));
    eprintln!("{}", row("clean", "Remove the build/ directory"));
    eprintln!("{}", row("watch", "Rebuild when sources change"));
    eprintln!("{}", row("pkg [args]", "Run the package manager"));
    eprintln!(
        "{}",
        row("graph", "Print the module dependency graph as DOT")
    );
    eprintln!();
    eprintln!("{}", bold("Options:"));
    eprintln!("{}", row("--release", "Build in release mode"));
    eprintln!("{}", row("--debug", "Build in debug mode (default)"));
    eprintln!("{}", row("--json", "Emit diagnostics as JSON on stdout"));
    eprintln!(
        "{}",
        row("--version, -V", "Print detailed version info and exit")
    );
    eprintln!("{}", row("--help, -h", "Print this help and exit"));
}

/// Print a detailed version banner: version, LLVM revision, host triple,
/// build profile, and (when inside a checkout) the git commit.
fn print_version() {
    let colour = colour_on(std::io::stdout().is_terminal());
    println!(
        "ntsc {} — Neutron Type-Safe Compiler",
        paint(NTSC_VERSION, BOLD, colour)
    );
    println!();
    println!("LLVM   {}", ntsc_codegen::llvm_version());
    println!("Host   {}", ntsc_codegen::host_triple());
    println!(
        "Build  {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    if let Some(commit) = git_commit() {
        println!("Commit {commit}");
    }
}

/// The current commit hash of the workspace checkout, when the running
/// binary can be traced back to one. Best effort: `None` for released
/// binaries.
fn git_commit() -> Option<String> {
    let dir = find_rewrite_dir_from_exe()?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let hash = String::from_utf8(output.stdout).ok()?;
    let hash = hash.trim();
    if hash.is_empty() {
        None
    } else {
        Some(hash.to_string())
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BuildMode {
    Debug,
    Release,
}

impl BuildMode {
    fn new(release: bool) -> Self {
        if release { Self::Release } else { Self::Debug }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn opt_level(self) -> OptimizationLevel {
        match self {
            Self::Debug => OptimizationLevel::None,
            Self::Release => OptimizationLevel::Aggressive,
        }
    }
}

/// Scaffold a new project with exactly two files: `neutron.toml` and
/// `src/main.nt`.
fn cmd_init(project_name: Option<&str>) -> Result<(), CliError> {
    let name = match project_name {
        Some(n) => n.to_string(),
        None => {
            let cwd = env::current_dir()?;
            cwd.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("my-project")
                .to_string()
        }
    };

    let project_dir = Path::new(&name);
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    let neutron_toml = format!(
        "[package]\n\
         entry = \"src/main.nt\"\n\
         output = \"{}\"\n",
        ntsc_codegen::with_executable_extension(&name),
    );
    fs::write(project_dir.join("neutron.toml"), &neutron_toml)?;

    let main_nt = "fun main() {\n    say(\"Hello, World!\")\n}\n";
    fs::write(src_dir.join("main.nt"), main_nt)?;

    println!("Created project `{name}`:");
    println!("  {name}/neutron.toml");
    println!("  {name}/src/main.nt");

    Ok(())
}

/// Build the current project: read `neutron.toml`, load modules, compile,
/// link.
///
/// In test mode the user `main` is replaced by the test harness.
fn cmd_build(release: bool, test_mode: bool, json: bool) -> Result<PathBuf, CliError> {
    let (cwd, config) = load_project()?;
    let mode = BuildMode::new(release);
    let entry_path = cwd.join(&config.entry);

    // Resolve and parse the module closure (in parallel).
    let load_start = Instant::now();
    let loaded = ntsc_build::modules::load_program(&entry_path).map_err(load_error)?;
    let load_time = load_start.elapsed();

    // Run name resolution up front so undefined-name errors get the
    // `NTSC-E0101` code (and any "did you mean" suggestions).
    if let Err(errors) = ntsc_typeck::resolve_program(&loaded.program) {
        return Err(resolve_error(errors, &loaded));
    }

    // Lint warnings are non-fatal: report them, then continue the build.
    let warnings = ntsc_typeck::lint_program(&loaded.program);
    if !warnings.is_empty() {
        emit_warnings(warnings, &loaded, json);
    }

    // Create the output directory.
    let out_dir = cwd.join("build").join(mode.dir_name());
    fs::create_dir_all(&out_dir)?;

    if !json {
        let colour = colour_on(std::io::stdout().is_terminal());
        let count = loaded.modules.len();
        let noun = if count == 1 { "module" } else { "modules" };
        println!("  {} {count} {noun}:", paint("Compiling", CYAN, colour));
        for module in &loaded.modules {
            println!(
                "    {} ({:.1}ms)",
                display_rel(&cwd, &module.path),
                module.parse_duration.as_secs_f64() * 1000.0
            );
        }
    }

    // Compile source → object file.
    let codegen_start = Instant::now();
    let obj_path = ntsc_codegen::compile_program(
        &loaded.program,
        &config.target,
        mode.opt_level(),
        &config.output,
        &out_dir,
        test_mode,
    )
    .map_err(|e| codegen_error(e, &loaded))?;
    let codegen_time = codegen_start.elapsed();

    // Find the runtime library and link → binary.
    let runtime_lib = find_runtime_lib(&cwd)?;
    let output_path = out_dir.join(ntsc_codegen::with_executable_extension(&config.output));
    let link_start = Instant::now();
    ntsc_codegen::link_binary(&obj_path, &runtime_lib, &output_path)?;
    let link_time = link_start.elapsed();

    if !json {
        let colour = colour_on(std::io::stdout().is_terminal());
        println!(
            "  {} load {:.1}ms, codegen {:.1}ms, link {:.1}ms",
            paint("Timing", CYAN, colour),
            load_time.as_secs_f64() * 1000.0,
            codegen_time.as_secs_f64() * 1000.0,
            link_time.as_secs_f64() * 1000.0
        );
        println!(
            "  {} {mode:?} build at {}",
            paint("Finished", GREEN, colour),
            output_path.display()
        );
    }
    Ok(output_path)
}

/// Convert a module-load failure into diagnostics, attaching the source
/// text of any file that produced parse errors so snippets can be
/// rendered.
fn load_error(error: ntsc_build::modules::ModuleLoadError) -> CliError {
    let diags = error.into_diagnostics();
    let mut sources = SourceMap::new();
    for diag in &diags {
        if let Some(path) = diag.file_path.as_deref()
            && let Ok(text) = fs::read_to_string(path)
        {
            sources.add(SourceBuffer::new(&text, path));
        }
    }
    CliError::Diagnostics { diags, sources }
}

/// Render lint warnings, attributing each span to its source file via
/// the module provenance recorded during load.
fn emit_warnings(
    warnings: Vec<ntsc_typeck::Warning>,
    loaded: &ntsc_build::modules::ModuleLoadResult,
    json: bool,
) {
    let diags: Vec<Diagnostic> = warnings
        .into_iter()
        .map(|warning| {
            let mut diag = Diagnostic::from(&warning);
            if let Some((path, base)) = loaded.localize(warning.span) {
                diag.file_path = Some(path.display().to_string());
                for label in &mut diag.labels {
                    label.span.start = label.span.start.saturating_sub(base);
                    label.span.end = label.span.end.saturating_sub(base);
                }
            }
            diag
        })
        .collect();
    if json {
        println!("{}", diagnostics_to_json(&diags));
    } else {
        let writer = Writer::new(DiagConfig::from_env());
        writer.emit_all(&diags, Some(&loaded.sources));
    }
}

/// Convert a name-resolution failure into diagnostics, attributing each
/// error to its source file via the module provenance recorded during
/// load.
fn resolve_error(
    errors: Vec<ntsc_typeck::ResolveError>,
    loaded: &ntsc_build::modules::ModuleLoadResult,
) -> CliError {
    let diags = errors
        .into_iter()
        .map(|error| {
            let mut diag = Diagnostic::from(&error);
            if diag.file_path.is_none()
                && let Some((path, base)) = loaded.localize(error.span)
            {
                diag.file_path = Some(path.display().to_string());
                for label in &mut diag.labels {
                    label.span.start = label.span.start.saturating_sub(base);
                    label.span.end = label.span.end.saturating_sub(base);
                }
            }
            diag
        })
        .collect();
    CliError::Diagnostics {
        diags,
        sources: loaded.sources.clone(),
    }
}

/// Convert a codegen failure into diagnostics. Span-carrying diagnostics
/// (parse/type errors) are attributed to their source file via the module
/// provenance recorded during load, and their spans rebased to
/// file-local byte coordinates so source snippets can be rendered.
fn codegen_error(
    error: ntsc_codegen::CodegenError,
    loaded: &ntsc_build::modules::ModuleLoadResult,
) -> CliError {
    let diags = error
        .into_diagnostics()
        .into_iter()
        .map(|mut diag| {
            if diag.file_path.is_none()
                && let Some(span) = diag.labels.first().map(|l| l.span)
                && let Some((path, base)) = loaded.localize(span)
            {
                diag.file_path = Some(path.display().to_string());
                for label in &mut diag.labels {
                    label.span.start = label.span.start.saturating_sub(base);
                    label.span.end = label.span.end.saturating_sub(base);
                }
            }
            diag
        })
        .collect();
    CliError::Diagnostics {
        diags,
        sources: loaded.sources.clone(),
    }
}

/// Build in test mode, link, and run every `test` block. Exits non-zero
/// if any test fails.
fn cmd_test(release: bool, json: bool) -> Result<PathBuf, CliError> {
    let output_path = cmd_build(release, true, json)?;

    let mut child = std::process::Command::new(&output_path)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CliError::Plain(format!("failed to run `{}`: {e}", output_path.display())))?;

    // The generated harness reports its totals on a `__NTSC_SUMMARY__`
    // line (see `emit_test_harness`). That marker is an internal protocol
    // between codegen and this command, so it is consumed here and
    // re-rendered rather than shown to the user. Output is streamed line
    // by line so test progress still appears as it happens.
    let mut summary = None;
    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            let line =
                line.map_err(|e| CliError::Plain(format!("failed to read test output: {e}")))?;
            match line.strip_prefix(SUMMARY_MARKER) {
                Some(totals) => summary = Some(totals.trim().to_string()),
                None => println!("{line}"),
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| CliError::Plain(format!("failed to run `{}`: {e}", output_path.display())))?;

    if !json && let Some(totals) = summary {
        let colour = colour_on(std::io::stdout().is_terminal());
        let label = if status.success() { GREEN } else { BOLD };
        println!("  {} {totals}", paint("Summary", label, colour));
    }

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(output_path)
}

/// Build and run the project.
fn cmd_run(release: bool, json: bool) -> Result<PathBuf, CliError> {
    let output_path = cmd_build(release, false, json)?;

    if !json {
        let colour = colour_on(std::io::stdout().is_terminal());
        println!(
            "  {} {}...",
            paint("Running", CYAN, colour),
            output_path.display()
        );
    }
    let status = std::process::Command::new(&output_path)
        .status()
        .map_err(|e| CliError::Plain(format!("failed to run `{}`: {e}", output_path.display())))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(output_path)
}

/// Remove the `build/` directory.
fn cmd_clean() -> Result<(), CliError> {
    let cwd = env::current_dir()?;
    let build_dir = cwd.join("build");
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    println!("Removed {}", build_dir.display());
    Ok(())
}

/// Rebuild whenever the entry, an imported module, or `neutron.toml`
/// changes.
fn cmd_watch(release: bool, json: bool) -> Result<(), CliError> {
    let (cwd, config) = load_project()?;

    let rebuild = || {
        if let Err(e) = cmd_build(release, false, json) {
            report(e, json);
        }
    };

    let mut last_signature = project_signature(&cwd, &config)?;
    println!("Watching for changes (Ctrl-C to stop)...");
    rebuild();

    loop {
        std::thread::sleep(Duration::from_millis(400));
        match project_signature(&cwd, &config) {
            Ok(signature) if signature != last_signature => {
                println!("  Change detected, rebuilding...");
                last_signature = signature;
                rebuild();
            }
            Err(e) => {
                report(e, json);
                std::thread::sleep(Duration::from_secs(1));
            }
            Ok(_) => {}
        }
    }
}

/// Print the module dependency graph in DOT format.
fn cmd_graph() -> Result<(), CliError> {
    let (cwd, config) = load_project()?;
    let graph = ntsc_build::modules::discover(&cwd.join(&config.entry)).map_err(load_error)?;

    println!("digraph ntsc_modules {{");
    for (importer, importee) in &graph.edges {
        println!(
            "  \"{}\" -> \"{}\";",
            display_rel(&cwd, importer),
            display_rel(&cwd, importee)
        );
    }
    println!("}}");
    Ok(())
}

/// Run the package-manager executable installed beside `ntsc`.
fn cmd_pkg(args: &[String]) -> Result<(), CliError> {
    let current_exe = env::current_exe()?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| CliError::Plain("cannot locate the ntsc executable directory".into()))?;
    let pkg_name = ntsc_codegen::with_executable_extension("ntsc-pkg");
    let pkg = exe_dir.join(&pkg_name);

    if !pkg.is_file() {
        return Err(CliError::Plain(format!(
            "cannot find the package manager beside ntsc: {}",
            pkg.display()
        )));
    }

    let status = std::process::Command::new(&pkg).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Snapshot the modification times of every file in the module closure
/// plus `neutron.toml`. Used by watch to detect changes (including new
/// imports).
fn project_signature(
    cwd: &Path,
    config: &ntsc_build::BuildConfig,
) -> Result<Vec<(PathBuf, std::time::SystemTime)>, CliError> {
    let graph = ntsc_build::modules::discover(&cwd.join(&config.entry)).map_err(load_error)?;
    let mut signature = Vec::new();
    for path in &graph.files {
        let mtime = fs::metadata(path)?.modified()?;
        signature.push((path.clone(), mtime));
    }
    let neutron_toml = cwd.join("neutron.toml");
    let mtime = fs::metadata(&neutron_toml)?.modified()?;
    signature.push((neutron_toml, mtime));
    Ok(signature)
}

/// Read `neutron.toml` from the current directory.
fn load_project() -> Result<(PathBuf, ntsc_build::BuildConfig), CliError> {
    let cwd = env::current_dir()?;

    let build_path = cwd.join("neutron.toml");
    let build_src = fs::read_to_string(&build_path)
        .map_err(|e| CliError::Plain(format!("cannot read neutron.toml: {e}")))?;
    let config = ntsc_build::parse(&build_src).map_err(|errors| {
        let msgs: Vec<_> = errors.iter().map(|e| e.to_string()).collect();
        CliError::Plain(format!("neutron.toml errors:\n  {}", msgs.join("\n  ")))
    })?;

    Ok((cwd, config))
}

/// Display a canonical path relative to `base` when possible.
fn display_rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Locate the runtime static library.
///
/// Checks the build directory first, then the Cargo target directory.
/// The archive name follows the host toolchain (`libntsc_runtime.a` on
/// Unix and MinGW, `ntsc_runtime.lib` under MSVC).
fn find_runtime_lib(project_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let lib_name = ntsc_codegen::runtime_lib_name();

    // Installers stage the runtime archive relative to the executable
    // (beside it on Windows and in the macOS bundle, in a sibling
    // `lib/ntsc` on Unix prefixes), so an installed ntsc links NTSC
    // programs with no workspace checkout and no cargo toolchain.
    if let Some(runtime) = find_installed_runtime() {
        return Ok(runtime);
    }

    // Check standard cargo build output paths.
    let candidates = [
        project_dir.join("target").join("debug").join(lib_name),
        project_dir.join("target").join("release").join(lib_name),
    ];

    // Also check if we're inside the workspace (for development).
    let workspace_target = project_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("target").join("debug").join(lib_name));

    for candidate in candidates.iter().chain(workspace_target.iter()) {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // Try to build it ourselves. Locate the workspace from the running
    // binary so projects outside the checkout still build against the
    // runtime. An installed binary has no workspace, so this fails with a
    // message that points at where an installer is expected to have
    // staged the runtime.
    let rewrite_dir = match find_rewrite_dir(project_dir)?.or_else(find_rewrite_dir_from_exe) {
        Some(dir) if dir.exists() => dir,
        _ => return Err(runtime_not_found_error(lib_name).into()),
    };

    println!("  Building runtime...");
    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "ntsc-runtime"])
        .current_dir(&rewrite_dir)
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;

    if !status.success() {
        return Err("failed to build ntsc-runtime".into());
    }

    // Try again after building.
    let built = rewrite_dir.join("target").join("debug").join(lib_name);
    if built.exists() {
        return Ok(built);
    }

    Err(format!("cannot find {lib_name} after building").into())
}

/// Locate a runtime archive staged by an installer, relative to the
/// running executable. Returns the first candidate that exists.
///
/// `current_exe` resolves symlinks on Linux (via `/proc/self/exe`), so a
/// tarball whose `bin/ntsc` is symlinked onto `PATH` still resolves its
/// sibling `lib/ntsc` directory correctly.
fn find_installed_runtime() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let lib_name = ntsc_codegen::runtime_lib_name();
    exe_relative_runtime_candidates(exe_dir, lib_name)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// The runtime-archive locations an installed `ntsc` searches, relative
/// to its executable directory, in priority order:
///
/// 1. `<exe_dir>/<lib>` — Windows (`.msi` and tarball), the macOS `.app`
///    bundle, and a dev `target/{debug,release}` build.
/// 2. `<exe_dir>/../lib/ntsc/<lib>` — the `.deb`, Arch, and Unix tarball
///    prefix layout (`bin/` + `lib/ntsc/`).
/// 3. `<exe_dir>/../lib64/ntsc/<lib>` — the `.rpm` layout: `rpmbuild`
///    expands `%{_libdir}` to `/usr/lib64` on x86_64, so Fedora installs
///    land there.
///
/// Pure and filesystem-free so the ordering is unit-tested directly.
fn exe_relative_runtime_candidates(exe_dir: &Path, lib_name: &str) -> Vec<PathBuf> {
    vec![
        exe_dir.join(lib_name),
        exe_dir.join("..").join("lib").join("ntsc").join(lib_name),
        exe_dir.join("..").join("lib64").join("ntsc").join(lib_name),
    ]
}

/// Error shown when no runtime archive can be found and no workspace is
/// available to build one — i.e. an installed binary whose installer
/// failed to stage the runtime. Names the archive and the prefix layout
/// it is expected in rather than the misleading "cannot locate the NTSC
/// workspace".
fn runtime_not_found_error(lib_name: &str) -> String {
    let searched = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .map(|exe_dir| {
            exe_relative_runtime_candidates(&exe_dir, lib_name)
                .iter()
                .map(|candidate| format!("\n  {}", candidate.display()))
                .collect::<String>()
        })
        .unwrap_or_default();
    format!(
        "cannot find the NTSC runtime library ({lib_name}).\n\
         Searched next to the executable:{searched}\n\
         An installed ntsc expects the runtime at <prefix>/lib/ntsc/{lib_name}; \
         reinstall from a package that ships it, or run inside the NTSC workspace \
         so it can be built."
    )
}

/// Walk up from `from` to find the rewrite workspace root.
fn find_rewrite_dir(from: &Path) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut dir = from.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("crates").exists() {
            return Ok(Some(dir));
        }
        if !dir.pop() {
            break;
        }
    }
    Ok(None)
}

/// Walk up from the `ntsc` executable to find the rewrite workspace
/// root.
fn find_rewrite_dir_from_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    find_rewrite_dir(&exe).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_candidates_are_ordered_beside_then_lib_then_lib64() {
        let exe_dir = Path::new("/opt/ntsc/bin");
        let got = exe_relative_runtime_candidates(exe_dir, "libntsc_runtime.a");

        assert_eq!(
            got,
            vec![
                PathBuf::from("/opt/ntsc/bin/libntsc_runtime.a"),
                PathBuf::from("/opt/ntsc/bin/../lib/ntsc/libntsc_runtime.a"),
                PathBuf::from("/opt/ntsc/bin/../lib64/ntsc/libntsc_runtime.a"),
            ],
            "beside-exe must win (Windows/.msi/macOS), then the .deb/Arch \
             prefix, then the .rpm lib64 prefix"
        );
    }

    #[test]
    fn runtime_candidates_use_the_given_lib_name() {
        // The archive name follows the host toolchain (ntsc_runtime.lib
        // under MSVC), so the helper must never hardcode it.
        let got = exe_relative_runtime_candidates(Path::new("/p/bin"), "ntsc_runtime.lib");
        assert!(got.iter().all(|p| p.ends_with("ntsc_runtime.lib")));
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn runtime_not_found_error_names_the_library_and_prefix() {
        let msg = runtime_not_found_error("libntsc_runtime.a");
        assert!(msg.contains("libntsc_runtime.a"));
        assert!(msg.contains("<prefix>/lib/ntsc/"));
        // Must not resurrect the misleading workspace-only wording.
        assert!(!msg.contains("cannot locate the NTSC workspace"));
    }
}
