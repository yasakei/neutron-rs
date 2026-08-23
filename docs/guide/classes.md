# Classes and enums

## Classes

A class bundles fields and methods:

```ntsc
class Counter {
    var int n

    fun init(int start) {
        this.n = start
    }

    fun bump() {
        this.n = this.n + 1
    }
}
```

Field declarations use the same `var` syntax as local variables and may carry a
type annotation. Methods use `fun`. A method named `init` is the constructor;
the constructor is what allows a class to be instantiated with arguments.
Classes without an `init` are still instantiated, with each field at its zero
value:

```ntsc
class Point {
    var int x
    var int y
}

fun main() {
    var p = Point()
    p.x = 3
    p.y = 4
}
```

A field can declare its own starting value, which is applied every time an
instance is constructed — before `init` runs, so a constructor is free to
overwrite it:

```ntsc
class Bag {
    var name = "bag"
    var xs = [1, 2, 3]
    fun init(string label) {
        this.name = label
    }
}

fun main() {
    var b = Bag("crate")
    say(b.name + " " + arrays.length(b.xs))  // crate 3
}
```

### Instantiation and field access

Instances are created by calling the class name like a function. Fields are
read and assigned with `.`:

```ntsc
var c = Counter(40)
c.bump()
say("" + c.n)   // 41
```

### `this`

`this` refers to the current instance. It is required to disambiguate field
access from local variables inside methods:

```ntsc
fun init(int start) {
    this.n = start
}
```

### Reference semantics

Class instances have reference semantics: assigning one instance variable to
another aliases the same object rather than copying it. Mutations through
either name are visible through the other:

```ntsc
var p = Point()
var q = p
q.x = 10
say("" + p.x)   // 10
```

Escape analysis stack-allocates instances that neither escape the function nor
declare an `init`; classes with a constructor are always heap-allocated. The
behavioral contract (reference semantics, correct lifetime) is identical in
both cases.

### Inheritance

A class may extend a parent:

```ntsc
class Base {
    var int id

    fun init(int id) {
        this.id = id
    }
}

class Derived extends Base {
    // inherits fields and methods from Base
}
```

`extends` provides single inheritance: the child inherits the parent's fields
and methods. The `super` keyword is reserved for accessing the parent and is
not yet usable in expressions.

### Classes as values

Class instances are first-class values: they can be passed to functions,
returned from functions, stored in arrays, and used as the element type of
`for-in` when they implement the iterator protocol.

```ntsc
fun make() -> Counter {
    var c = Counter(40)
    c.bump()
    return c
}
```

## Enums

An enum declares a fixed set of named cases:

```ntsc
enum Direction {
    North,
    South,
    East,
    West
}
```

Enum cases are matched with `match`:

```ntsc
match (dir) {
    case North => say("up")
    case South => say("down")
    case _ => say("sideways")
}
```

See [Syntax and control flow](syntax-and-control-flow.md) for the full pattern
syntax available in `case` arms.

## The iterator protocol

Any class with a `length() -> int` method and a `get(int) -> T` method can be
used in `for-in`. The loop variable is typed from `get`'s return type:

```ntsc
class Range {
    var int count

    fun init(int n) {
        this.count = n
    }

    fun length() -> int {
        return this.count
    }

    fun get(int i) -> int {
        return i * 10
    }
}

fun main() {
    var total = 0
    for (var v in Range(3)) {
        total = total + v
    }
    say("" + total)   // 30
}
```

The loop is equivalent to iterating indices `0 .. length()` and calling
`get(i)` on each. See [Arrays and iterators](arrays-and-iterators.md).
