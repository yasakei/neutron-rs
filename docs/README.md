# NTSC Documentation

NTSC (the Neutron Type-Safe Compiler) is a compiled, statically typed
programming language with a Rust implementation that lowers directly to native
code through LLVM. It combines the ergonomics of a managed language with
deterministic memory management: values are owned and moved by default, views
provide borrowing, and a `shared` escape hatch covers the cases where aliasing
is genuinely needed.

This directory is the complete reference for the language, its standard
library, and the `ntsc` toolchain. It is split into a user guide and a
reference section.

## Guide

The guide walks through the language feature by feature, with runnable
examples. Read it in order if you are new to NTSC.

| Document | Contents |
| --- | --- |
| [Getting started](guide/getting-started.md) | Requirements, installation, the first program, the build/test/run workflow. |
| [Syntax and control flow](guide/syntax-and-control-flow.md) | Lexical structure, variables, operators, and all control-flow statements. |
| [Functions](guide/functions.md) | Function declarations, parameters, return values, and lambdas. |
| [Classes and enums](guide/classes.md) | Classes, constructors, methods, inheritance, enums, and `match`. |
| [Ownership](guide/ownership.md) | The Own - Move - View model: moves, views, `view mut`, `copy`, and escape analysis. |
| [Shared values](guide/shared-values.md) | The `shared T` escape hatch for explicit aliasing. |
| [Arrays and iterators](guide/arrays-and-iterators.md) | Arrays, indexing, `for-in`, the iterator protocol, and the `arrays` module. |
| [Error handling](guide/error-handling.md) | Exceptions, `try`/`catch`/`finally`, `retry`, results with `?` propagation, and the standard library error convention. |
| [Modules and imports](guide/modules.md) | Multi-file projects and the file-import system. |
| [Concurrency](guide/concurrency.md) | Threads, channels, and `async`/`await`. |
| [Testing](guide/testing.md) | `test` blocks and the `ntsc test` runner. |

## Reference

The reference is exhaustive and aimed at lookup rather than learning.

| Document | Contents |
| --- | --- |
| [Language reference](reference/language.md) | Every type, statement, expression, and grammar rule. |
| [Standard library](reference/stdlib.md) | Every module and function exposed to NTSC source. |
| [CLI reference](reference/cli.md) | The `ntsc` command, the `build.ntbl` manifest, and environment variables. |
| [Internals](reference/internals.md) | ABI, memory model, LLVM pipeline, leak detection, and platform support. |
| [Contributing](reference/contributing.md) | Building, testing, layout, conventions, and end-to-end change workflows. |

## Compiler layout

The compiler lives in the workspace root as a set of crates:

| Crate | Responsibility |
| --- | --- |
| `ntsc-lexer` | Tokenizer with automatic semicolon insertion. |
| `ntsc-parser` | Recursive-descent parser producing the AST. |
| `ntsc-ast` | AST types and spans. |
| `ntsc-typeck` | Name resolution, type checking, ownership checking, and linting. |
| `ntsc-codegen` | LLVM IR generation, optimization, and linking. |
| `ntsc-runtime` | The runtime static library linked into every binary. |
| `ntsc-build` | `build.ntbl` parsing and multi-file module loading. |
| `ntsc-diag` | Diagnostic rendering and JSON error output. |
| `ntsc-cli` | The `ntsc` command-line binary. |

## Feature summary

The language currently provides:

- Primitive, array, option, object, and class types with prefix type
  annotations.
- Ownership with move semantics, block-scoped views (`view`, `view mut`),
  explicit deep copies (`copy`), and a `shared T` alias escape hatch.
- Escape analysis that stack-allocates non-escaping class instances.
- Classes with constructors and methods, single inheritance, enums, and
  `match` with guards and destructuring patterns.
- Lambdas and a first-class function type.
- Exceptions (`throw`/`try`/`catch`/`finally`), `retry`, and unsafe blocks.
- The iterator protocol, letting any class with `length()` and `get(i)`
  participate in `for-in`.
- Threads, message-passing channels, and `async`/`await` coroutines lowered
  to state machines.
- A standard library of over twenty modules, from `strings` and `arrays` to
  `net`, `json`, and `crypto`.
- A test runner driven by `test` blocks.
- Debug-mode leak detection and a release-mode LLVM optimization pipeline.
- Cross-platform support for Linux, macOS, and Windows.
