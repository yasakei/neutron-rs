# Getting started

## Requirements

NTSC requires:

- Linux, macOS, or Windows.
- The LLVM 22 toolchain (specifically the libraries used by the `llvm-sys`
  crate). On Linux and Windows the LLVM shared libraries must be discoverable;
  on macOS they are linked into the binary at compile time.
- A C toolchain and linker (gcc or clang on Linux, clang and the Xcode command
  line tools on macOS, or the MSVC toolchain on Windows).
- Rust 1.85 or newer (stable), if building from source.

### Finding LLVM

The compiler locates LLVM through the `LLVM_SYS_221_PREFIX` environment
variable, which must point at an LLVM 22 installation. For example:

```sh
export LLVM_SYS_221_PREFIX=/usr/local/opt/llvm@22
```

Without this variable, the build looks in the usual system locations. On
Windows, either set `LLVM_SYS_221_PREFIX` to the LLVM install directory or add
`llvm-config` to the `PATH`.

## Building from source

```sh
cargo build --release
```

This produces the `ntsc` binary under `target/release/`. The runtime library is
built as part of the workspace and linked into every generated binary, so no
separate install step is needed.

Verify the install:

```sh
ntsc --version
```

prints a detailed version banner:

```
ntsc 26.0.0b — Neutron Type-Safe Compiler

LLVM   22.1.8
Host   x86_64-unknown-linux-gnu
Build  debug
Commit 33fe798
```

The `LLVM` line reports the linked LLVM revision, `Host` the target triple,
`Build` the profile of the `ntsc` binary itself, and `Commit` the git commit
it was built from (omitted for released binaries).

NTSC versions follow a year-based scheme, `<year>.<release>.<patch><stage>`:
`a` is an alpha, `b` a beta, and stable releases drop the suffix (e.g.
`26.0.0`).

## Your first project

```sh
ntsc init hello
cd hello
ntsc run
```

`ntsc init` scaffolds a project with exactly two files:

```
hello/
  neutron.toml
  src/main.nt
```

`src/main.nt` contains a minimal program:

```ntsc
fun main() {
    say("Hello, World!")
}
```

`ntsc run` compiles the project and executes the resulting binary, printing
`Hello, World!`.

## The project layout

```
project/
  neutron.toml      Manifest: target triple, entry point, output name
  src/            Source files (.nt)
  build/          Generated output; debug/ and release/ subdirectories
```

Source files are plain UTF-8 text with the `.nt` extension. The entry file is
declared in `neutron.toml`; additional files are pulled in with `use` (see
[Modules and imports](modules.md)).

## Build, test, run, watch

| Command | Effect |
| --- | --- |
| `ntsc build` | Compile the project to `build/debug/`. |
| `ntsc build --release` | Compile with LLVM aggressive optimization to `build/release/`. |
| `ntsc test` | Compile and run every `test` block in the project. |
| `ntsc run` | Build and execute immediately. |
| `ntsc clean` | Remove the `build/` directory. |
| `ntsc watch` | Rebuild whenever a source file changes. |

Common flags:

- `--release` / `--debug`: select the build mode. Debug is the default and uses
  no optimization.
- `--json`: emit diagnostics as JSON on stdout instead of formatted text.

## Debug versus release

Debug builds:

- Apply no LLVM optimization.
- Enable leak detection, so programs that drop owned heap values report them on
  exit.
- Give the runtime its own `backtrace()` function for dumping the call stack.

Release builds:

- Apply LLVM's aggressive optimization pipeline.
- Skip leak detection.
- Remove runtime debugging helpers.

See [Internals](../reference/internals.md) for details of what each mode changes.

## A slightly larger example

A program that uses a function, an array, and the iterator protocol:

```ntsc
fun square(int n) -> int {
    return n * n
}

fun main() {
    var array[int] values = [1, 2, 3, 4]
    var int total = 0
    for (var v in values) {
        total = total + square(v)
    }
    say("sum of squares: ${total}")
}
```

Note the string interpolation `${expr}` and the prefix type annotations
(`array[int]` for the array element type, `int` before each variable name).

## Next steps

- [Syntax and control flow](syntax-and-control-flow.md)
- [Functions](functions.md)
- [The language reference](../reference/language.md)
