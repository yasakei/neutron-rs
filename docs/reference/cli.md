# CLI reference

`ntsc` is the compiler command line. It operates on a project rooted at the
current working directory.

## Synopsis

```
ntsc [--release | --debug] [--json] <command>
```

Flags may appear before or after the command.

## Commands

| Command | Description |
| --- | --- |
| `ntsc init [name]` | Scaffold a new project in `./name` (defaults to the current directory name). |
| `ntsc build` | Compile the project to `build/debug/`. |
| `ntsc test` | Compile in test mode and run every `test` block. |
| `ntsc run` | Build, then execute the resulting binary. |
| `ntsc clean` | Remove the `build/` directory. |
| `ntsc watch` | Rebuild whenever a source file or `build.ntbl` changes. |
| `ntsc graph` | Print the module dependency graph as DOT. |
| `ntsc version` / `--version` / `-V` | Print detailed version info (version, LLVM revision, host triple, build profile, git commit) and exit. |
| `ntsc --help` / `-h` | Print usage. |

`ntsc init` creates exactly two files:

```
name/
  build.ntbl      target/entry/output for the host platform
  src/main.nt     fun main() { say("Hello, World!") }
```

## Flags

| Flag | Effect |
| --- | --- |
| `--release` | Build with LLVM aggressive optimization into `build/release/`. |
| `--debug` | Build with no optimization into `build/debug/` (the default). |
| `--json` | Emit diagnostics as JSON on stdout. |

## Build output

A successful build prints, for each module, its parse time:

```
  Modules (1):
    src/main.nt (0.1ms)
  load 0.2ms, codegen 3.4ms, link 1.1ms
Build complete (Debug): /path/to/build/debug/hello
```

`ntsc run` additionally prints `Running /path/to/build/debug/hello...`.

## The build manifest

`build.ntbl` is a line-based manifest with exactly three keys, each appearing
once:

```
target "x86_64-unknown-linux-gnu"
entry "src/main.nt"
output "hello"
```

- `target` is an LLVM target triple.
- `entry` is the entry source file, relative to the project root.
- `output` is the output binary name (without extension; the platform
  extension is added automatically).

Missing or duplicate keys are reported as `build.ntbl errors`.

## Diagnostics

Errors are formatted with annotated source snippets, spans, and diagnostic
codes (see the [language reference](language.md)). Render behavior is
controlled by the environment:

- `CLICOLOR_FORCE` (non-empty, not `0`): force color on.
- `NO_COLOR` (non-empty): force color off.
- `CLICOLOR=0`: color off.
- `TERM=dumb`: color off.
- Otherwise color is used only when stderr is a terminal.

The error limit defaults to 20 and can be changed with `NTSC_MAX_ERRORS`
(`0` means unlimited). With `--json`, diagnostics are emitted as structured
JSON on stdout instead.

## Environment variables

| Variable | Meaning |
| --- | --- |
| `LLVM_SYS_221_PREFIX` | Location of an LLVM 22 installation. |
| `NTSC_MAX_ERRORS` | Maximum diagnostics shown per run; `0` is unlimited. |
| `CLICOLOR_FORCE`, `NO_COLOR`, `CLICOLOR`, `TERM` | Diagnostic color control. |

## The runtime library

The compiler links every binary against the NTSC runtime static library.
`find_runtime_lib` looks in the project's `target/` directory, then in the
workspace `target/debug/`, and builds `ntsc-runtime` with Cargo if it cannot
find it. The archive is named `libntsc_runtime.a` on Unix and MinGW and
`ntsc_runtime.lib` under MSVC.

## Test mode

`ntsc test` builds with the test harness substituted for `main`, then runs the
resulting binary. Each test prints `PASS name` or `FAIL name: message`, ending
with a `Summary N passed, M failed` line. The process exits non-zero if any
test failed. See [Testing](../guide/testing.md).

## Watch mode

`ntsc watch` snapshots the modification times of every file in the module
closure plus `build.ntbl`, rebuilds, and polls every 400 ms. When any file
changes it prints `Change detected, rebuilding...` and rebuilds. New imports
are detected because the snapshot includes the discovery result.
