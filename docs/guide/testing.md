# Testing

Tests are declared with `test` blocks inside source files. `ntsc test`
discovers them, compiles a harness in place of `main`, and runs each block.

## Writing tests

```ntsc
use testing

fun add(int a, int b) -> int {
    return a + b
}

test add_works {
    testing.assert_eq(add(2, 3), 5)
    testing.assert_ne(add(2, 3), 6)
}

test strings_compare {
    testing.assert_eq("hello", "hello")
}

fun main() {
    say("user main must not run in test mode")
}
```

A `test` block is a body of statements; a thrown exception or a failed
assertion fails the test. `fun main` is replaced by the test harness in test
mode, so the user `main` never runs under `ntsc test`.

## Running

```sh
ntsc test
```

Each test prints a result line:

```
PASS add_works
PASS strings_compare
  Summary 2 passed, 0 failed
```

A failing test prints its message:

```
FAIL failing_one: testing.assert_true: expected true, got false
  Summary 1 passed, 1 failed
```

The process exits non-zero when any test fails. Tests run in the order they
are declared.

## Assertions

The `testing` module provides these assertions:

| Function | Passes when |
| --- | --- |
| `testing.assert_true(bool)` | the argument is `true` |
| `testing.assert_false(bool)` | the argument is `false` |
| `testing.assert_eq(a, b)` | `a == b` |
| `testing.assert_ne(a, b)` | `a != b` |

`assert_eq` and `assert_ne` accept `int`, `float`, `string`, and `bool`
operands. On failure they throw an exception whose message names the failing
assertion, so the failure can also be caught and inspected:

```ntsc
try {
    testing.assert_eq(1, 2)
} catch (err) {
    say(err)   // "testing.assert_eq: ..."
}
```

## Release builds

`ntsc test --release` runs the test harness against the optimized build. The
`--json` flag applies to test output as it does to builds.
