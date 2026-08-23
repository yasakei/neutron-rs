# Error handling

NTSC uses exceptions. Any value can be thrown; a matching `catch` recovers
control. Uncaught exceptions terminate the program with a non-zero exit code
and a message on stderr.

## Throw

`throw expr` raises an exception:

```ntsc
throw "something went wrong"
```

## Try / catch / finally

`try` wraps a block. `catch (name)` binds the thrown value to a variable.
`finally` runs whether or not an exception was thrown:

```ntsc
try {
    var n = fmt.to_int("oops")
    say("unreached")
} catch (err) {
    say("caught: " + err)
}
```

Both `catch` and `finally` are optional, but a `try` with neither is unusual.
`catch` and `finally` blocks see variables declared in the enclosing scope.

## Retry

`retry <count> <body>` attempts a block up to `count` times, with an optional
`catch` after it:

```ntsc
retry 3 {
    network_call()
} catch (err) {
    say("gave up: " + err)
}
```

`retry` runs the block up to `count` times. If a run throws, the attempt is
counted and the block runs again while attempts remain; once exhausted, control
passes to the `catch` handler, or the exception is re-thrown outward when there
is no `catch`.

## Numeric and indexing faults

Integer arithmetic and indexing throw ordinary catchable exceptions when an
operation cannot produce a correct answer, so a fault is handled with the same
`try`/`catch` as anything else:

```ntsc
try {
    var x = a * b
    say("" + x)
} catch (err) {
    say("too big: " + err)     // "integer multiplication overflow"
}
```

What throws: `+`, `-`, `*` (including `x++`, `x--`, and `-x`) when the result
does not fit in a 64-bit signed integer; `/` and `%` on a zero divisor or the
one overflowing division; `<<` and `>>` with a shift amount below 0 or above 63;
and an array index that is negative or past the end, on writes as well as reads.

This behavior is the same in debug and release builds. NTSC does not wrap on
overflow in one build mode and fault in the other, so a program cannot compute a
different answer once it is optimized.

Float arithmetic follows IEEE-754 and never throws — an out-of-range result is
infinity and an undefined one is NaN.

## Unsafe blocks

`unsafe <body>` is parsed and type-checked. It currently lowers to a plain
block; exception-converting semantics are not yet implemented.

## Standard library error convention

Standard library functions report failures by throwing an exception whose
message identifies the failing call: `module.func: detail`. This lets callers
distinguish error kinds with `try`/`catch` instead of inspecting return
values.

```ntsc
try {
    var r = sys.read("/nonexistent")
} catch (err) {
    say(err)   // "sys.read: ..."
}
```

Functions that follow this convention include `fmt.to_int`, `fmt.to_float`,
`sys.read`, `sys.write`, `json.parse`, `regex.find`, `crypto.base64_decode`,
`crypto.hex_decode`, `math.sqrt` (negative input), `process.spawn`,
`http.get`, `io.open`, `net.tcp_connect`, `random.weighted` (empty weights),
and `random.int` (inverted bounds).

Functions that previously returned a default or empty value on failure now
throw instead, so a successful call can be trusted to have produced a real
result.

## Lint suppression

`quiet [name, ...] body` suppresses lint warnings inside a block. For example,
the unused-variable lint:

```ntsc
quiet [unused_variable] {
    var unused = compute()
}
```

With an empty list, `quiet { ... }` suppresses all suppresable lints in the
block.
