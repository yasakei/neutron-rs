# Tuples

A tuple is a fixed-size, heterogeneous collection of values. Unlike arrays,
each element can have a different type, and the length is part of the type.

## Literal syntax

Tuples are written as a comma-separated list in parentheses:

```ntsc
var t = (10, 20)
var mixed = (42, "hello", true)
```

The type of a tuple is written with the element types in parentheses:

```ntsc
var t: (int, int) = (10, 20)
var mixed: (int, string, bool) = (42, "hello", true)
```

## Indexing

Elements are accessed by zero-based numeric index using dot notation:

```ntsc
var t = (10, 20)
say("" + t.0)   // 10
say("" + t.1)   // 20
```

Indexing is bounds-checked: an out-of-range index throws a catchable exception.

## Destructuring

Tuples can be unpacked into individual variables:

```ntsc
var t = (42, "hello")
var (a, b) = t
say("" + a)   // 42
say(b)        // hello
```

The number of binding names must match the tuple length.

## Returning tuples from functions

Functions can return tuples, which is useful for returning multiple values:

```ntsc
fun bounds() -> (int, int) {
    return (100, 200)
}

fun main() {
    var (w, h) = bounds()
    say("" + w)   // 100
    say("" + h)   // 200
}
```

The returned tuple can also be stored and indexed later:

```ntsc
fun pair() -> (int, int) {
    return (7, 14)
}

fun main() {
    var t = pair()
    say("" + t.0)   // 7
    say("" + t.1)   // 14
}
```

## Tuples with different types

Tuples shine when grouping values of different types without defining a class:

```ntsc
fun main() {
    var t = ("alice", "bob")
    var (a, b) = t
    say(a + " and " + b)   // alice and bob
}
```

## Use cases

- **Multiple return values**: Return several results from a function without
  wrapping them in a class.
- **Swap**: `(a, b) = (b, a)` swaps two variables.
- **Grouping**: Pair related values that don't warrant a class definition.
