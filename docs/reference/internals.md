# Internals

This document describes how the compiler lowers NTSC to native code. It is
aimed at contributors and at users who need to understand the ABI.

## Pipeline

Compilation proceeds through a fixed set of stages, each owned by one crate:

```
source (.nt)
  -> ntsc-lexer     tokenize with automatic semicolon insertion
  -> ntsc-parser    build the AST
  -> ntsc-typeck    name resolution, type checking, ownership, lints
  -> ntsc-codegen   LLVM IR, optimization, object file
  -> link against libntsc_runtime.a  (via the system C compiler)
```

For a project, `ntsc-build` first loads the entry file and the transitive
closure of file imports, merging them into one program and recording per
span the file each token came from (for diagnostics). Modules are parsed in
parallel.

Name resolution runs before code generation so that undefined-name errors get
the `NTSC-E0101` diagnostic and "did you mean" suggestions. Lint warnings are
reported but do not fail the build.

## LLVM lowering

### Object model

No pointer crosses the ABI. Every owned heap value lives in the runtime's
handle registry under an opaque `i64` key, and generated code passes only those
keys. The registry is a `HashMap<i64, Handle>` behind a mutex; the runtime crate
contains no `unsafe` code at all. `Ty::View(T)` has the same representation as
`T` — a view is a borrowed handle, not a distinct value.

The handle kinds are:

- **Strings**: an owned `String`; `ntsc_string_drop` removes it.
- **Arrays**: an element vector plus its element size and a `string_elements`
  flag.

```
ArrayData
  elem_size        : bytes per element
  string_elements  : elements are owned strings
  elements         : Vec<i64> — raw bits for scalars, handles for strings
```

A handle names the entry, not its storage, so it stays valid across
`arrays.push` however the element vector reallocates, and remains valid when
passed between functions.

`string_elements` marks an array whose elements are owned strings. Such arrays
deep-copy a string on insert (`arrays.push`, index-assignment), deep-copy every
element in array-producing operations (`clone`, `deep_clone`, `slice`,
`reverse`, `fill`, `clear`), free each element exactly once when the array is
dropped or an element is removed, and hand ownership of the removed element to
the caller on `pop`. All other arrays store plain bytes and none of this
applies. Codegen sets the flag from the element type: string and `Any` element
arrays own their strings; an empty `[]` literal adopts the representation of
its declared destination type (`var array[int] x = []` stores raw scalars).

- **Class instances**: heap structures with one field slot per field, or stack
  slots when escape analysis decides the instance cannot escape.
- **Shared boxes**: a counted entry holding a single inner handle.
  `ntsc_shared_new` adopts an owned handle; `ntsc_shared_retain`/
  `ntsc_shared_release` adjust the count, and the last release removes the box
  and returns the inner handle for the caller to drop.
- **Futures**: the state machine of an `async.sleep(ms)` future.
- **Opaque module resources**: values a stdlib module owns (files, sockets,
  channels) stored as `Box<dyn Any + Send>` and readable only at the type the
  owning module registered.

### Handle validity and kind safety

Handles are issued by a monotonic counter and are **never reused**: once a
handle is dropped its id stays unknown for the life of the process, so a stale
handle can never name a value some later allocation registered. This is what
makes the guarantees below decidable rather than best-effort.

Every operation resolves its handle before doing anything, and every
*destructive* operation additionally checks the kind:

| Handle it is given | What happens |
| --- | --- |
| `0` (null) | "no value" — the operation is a no-op and reports its safe failure |
| unknown, or already consumed | same as null; no other entry is touched |
| a live handle of the wrong kind | left untouched; the operation reports its safe failure |
| a live handle of the right kind | the operation runs |

So `ntsc_string_drop` on an array handle, `ntsc_array_drop` on a string handle,
`ntsc_shared_release` on a plain string, and `ntsc_async_sleep_drop` on any
value handle are all no-ops that leave the value they were given intact. A
second drop of the same handle is a no-op for the same reason — the first drop
removed the entry, and nothing else was ever issued that id. Reads work the same
way: an array handle has no string length, a shared box is not its own contents,
and an opaque file resource cannot be read as either.

Because a refused operation cannot return an error out-of-band, each returns the
documented safe failure for its result type:

| Result type | Safe failure |
| --- | --- |
| a handle | `0` (null) |
| a count, length, or boolean | `0` |
| a search index or character code | `-1` |

Nothing panics and nothing treats an unknown nonzero handle as a license to
touch another entry. Faults that *are* the program's error rather than an
invalid handle still throw a catchable exception: indexing a real array out of
bounds throws `array index out of bounds` on reads and writes alike.

Two counters, `LIVE` and `PERMANENT`, track live and permanently-registered
(compile-time constant) entries; `LIVE + PERMANENT == map.len()` is asserted in
the registry's own tests, which is how a wrong-kind or duplicate drop silently
losing an entry would be caught. `crates/ntsc-runtime/tests/handle_validity.rs`
pins all of the above through the public ABI with no `unsafe` code.

### Moves and drops

The generated code decides at compile time who owns each value:

- A bare-variable argument to an owned parameter is a move: the source slot is
  nulled so it is not dropped at its own exit, and the callee drops it.
- A view parameter borrows; the caller keeps ownership. Taking a view of an
  instance, or of one of its fields, therefore leaves the instance's owned
  fields to be reclaimed at the owner's scope exit.
- A field's declared initializer is stored at construction under the same
  ownership rules as `this.field = value`, so the instance owns it and the
  class drop thunk reclaims it.
- `copy(expr)` emits a deep copy and the result is a fresh owned value.
- "Fresh" results (literals, `copy`, string concatenation, `arrays.*` calls)
  are dropped by the statement that produced them; owned values stored into a
  variable are dropped when the variable's scope exits.
- `emit_drop_all_owned` runs the drop thunk for every owned slot at function
  exit (on the normal path and on the exception path).

### Exceptions

Exceptions use a return-check model, not stack unwinding. `ntsc_throw` stores
the message handle in a thread-local pending slot and returns `0`; generated
code checks `ntsc_exception_pending()` after every call that can throw and
branches to its exception-return path when the flag is set, running the drops
for everything live on that path on the way out. A `catch` handler takes the
message with `ntsc_exception_take_message`, which clears the flag;
`ntsc_rethrow` re-arms it (used after a `finally` that ran with a clean flag).
An exception still pending when `main` returns is reported by
`ntsc_runtime_shutdown` and aborts.

Standard library functions report failures by calling `throw_str`, which
constructs a `module.func: detail` message and calls `ntsc_throw`.

### Async lowering

An `async fun` is lowered to a poll-based state machine. The function body is
split into segments at each `await`; each suspension splits the body and
stores the continuation state plus live locals in a future struct. The runtime
executor (`ntsc_async_run`) polls the root future; a poll that reports
"pending" yields the thread and is re-polled later. Child futures are
scheduled with `ntsc_async_push`. `async.sleep(ms)` is implemented as a future
that arms a deadline on its first poll and completes once the clock passes it.

### Exception and async interaction

`try`, `throw`, and `retry` work inside async bodies. A `throw` sets the
thread-local pending-exception flag and propagates to the caller after
`wait_any`/`wait_all` returns.

## Optimization

Debug builds skip optimization and emit no optimization passes. Release builds
run the module through LLVM's `opt` with a fixed pass pipeline:

```
mem2reg, instcombine, simplifycfg, sccp, dce, gvn
```

Verification is enabled for each pass. Loop unrolling and vectorization are
disabled to keep IR generation deterministic. In particular, `mem2reg`
promotes the alloca-based variable slots into SSA registers; without the pass
every loop induction variable stays on the stack (a measured roughly 3% versus
70% relative speedup).

Every emitted module is verified before the object file is written, and the
compiler dumps the IR if verification fails.

## Escape analysis

Before emitting a class instantiation, the compiler analyzes whether the
instance can escape the function. If it cannot (and the class has no `init`,
since a constructor observes `this`), the instance is placed in an alloca
instead of calling the allocator. The escape test is conservative:

- returning the instance escapes it,
- storing it where the analysis cannot track it escapes it,
- a single member read or index through a bare base is safe; chaining further
  is only rejected for the stack-allocation question, since a deeper read still
  hands out no ownership,
- a grouped object base is treated as an escape.

The decision is invisible to program behavior.

The same walk answers a second question with the opposite default: whether the
scope owns an instance's fields and must run its drop thunk at exit. Refusing a
candidate there leaks, so a read that only borrows — a view, or a chained
member/index access — keeps the instance in the field-drop set.

## Test harness

`test name { ... }` compiles to a function `test_<name>`. In test mode the
user `main` is replaced by a generated harness that runs each `test_<name>`
inside an exception frame, prints `PASS name` or `FAIL name: message`, and
exits non-zero if any test fails.

## Linking

`link_binary` invokes the system C compiler on Unix hosts (`cc` with the
object file, the runtime archive, `-lm`, and on non-macOS hosts `-lpthread`
and `-ldl`). On Windows it prefers the `ld.lld` bundled next to the ntsc
executable (MSI installs; links via the MinGW emulation with the bundled
import libraries and the GNU-flavoured runtime), then falls back to MSVC
`link.exe` and MinGW `gcc`. Object and archive names are host-dependent:
`.o`/`.obj`, and `libntsc_runtime.a` versus `ntsc_runtime.lib`, chosen to
match the linker in use.

`host_triple()` and `with_executable_extension()` let the build produce
objects and binaries the local linker can consume, so tests and `ntsc init`
work on Linux, macOS, and Windows.

## Leak detection

Leak detection falls out of the registry: an entry that is never removed is a
leak. `ntsc_runtime_shutdown(report)` runs after `main` returns, and when
`report != 0` (debug builds) and any non-permanent entries are still live it
prints:

```
NTSC WARNING: Memory leak detected! N registry object(s) leaked.
```

String literals and other compile-time constants are registered as permanent
and excluded from the count. Release builds pass `report = 0` and stay silent.
Runtime assertions and panics write `NTSC PANIC: message` to stderr and abort.

## Deterministic IR

The code generator is written to produce deterministic IR:

- stdlib calls are routed through fixed ABI function names
  (`ntsc_<module>_<func>`), forward-declared up front;
- module-specific operations that need type knowledge the runtime cannot infer
  (arrays, sort, testing, `random.shuffle`/`weighted`, `process.spawn_thread`)
  are emitted by codegen helpers instead of the runtime;
- a fixed optimization pipeline with verification keeps output predictable.

## Platform support

Supported hosts are Linux, macOS, and Windows on x86-64 and aarch64, on stable
Rust with LLVM 22. The CI matrix builds and tests all three platforms.
