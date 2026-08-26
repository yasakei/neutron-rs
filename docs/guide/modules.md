# Modules and imports

A program is made of one or more `.nt` source files. The entry file is named
in `neutron.toml`; every other file is pulled in explicitly.

## The standard library

The standard library is exposed as a set of builtin modules. Every module name
is predeclared, so modules can be called without an import:

```ntsc
var n = strings.length("hello")
var sorted = sort.stable_sort([3, 1, 2])
say(fmt.i64_to_str(n))
```

The available modules are:

```
arrays    async     collections  crypto  csv
encoding  fmt       hash         http    io
json      math      net          os      process
random    regex     sort         strings sys
testing   time      toml         yaml
```

`use module` is still accepted and can serve as documentation of intent:

```ntsc
use strings
use sort as sorter
```

The `testing` module is routed through codegen helpers and is also available
without an import; the test runner examples use `use testing` for clarity.

See the [standard library reference](../reference/stdlib.md) for the full
function list.

## Selective imports

Symbols can be imported selectively from a module with
`use (a, b) = from module`. The same form works with file paths.

## Loading other source files

`use` pulls another `.nt` file into the program when its argument is a
quoted path; an identifier after `use` (like `use strings`) refers to a
stdlib module instead. The path is resolved relative to the file that
contains the import, and the `.nt` extension is inferred when omitted:

```ntsc
use "lib"

fun main() {
    say("" + lib_value())
}
```

`lib.nt` may itself import other files:

```ntsc
// lib.nt
use "util.nt"

fun lib_value() -> int {
    return util_value() * 2
}
```

File imports cannot escape the project root: a path resolving outside the
entry file's directory (for example through `../`) is rejected.

The compiler loads the whole closure of files, resolves names across the
merged program, and compiles it as one unit. Declaration order does not
matter: `lib.nt` can call a function defined later in the same file, and
`main.nt` can use classes defined in an imported file.

Modules are loaded in parallel during the build, and the build output prints
the parse duration of each one.

## Dependency graph

`ntsc graph` prints the module dependency graph as DOT:

```sh
ntsc graph
```

Each node is a source file; each edge is a file import.

## Build manifest

The project manifest `neutron.toml` declares three keys:

```
target "x86_64-unknown-linux-gnu"
entry "src/main.nt"
output "my-project"
```

- `target`: the LLVM target triple.
- `entry`: the entry source file, relative to the project root.
- `output`: the name of the produced binary.

All three keys are required and each may appear only once. `ntsc init`
generates a correct manifest for the host platform automatically. See the
[CLI reference](../reference/cli.md).
