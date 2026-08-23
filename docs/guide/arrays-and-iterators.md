# Arrays and iterators

## Array types and literals

Arrays are heap values with a fixed element type. The type is written
`array[T]`; literals are written with brackets:

```ntsc
var array[int] xs = [1, 2, 3]
var floats = [1.5, 2.5]       // array[float]
var names = ["a", "b"]        // array[string]
```

The empty literal `[]` has no element type and must be annotated:

```ntsc
var array[int] empty = []
```

Untyped arrays can also be created with `arrays.new()`.

## Indexing

`a[i]` reads an element, `a[i] = v` writes one. Indices are zero-based and
bounds are checked at runtime; out-of-range access throws an exception.

```ntsc
var fs = [2.5, 1.5, 3.5]
say("" + fs[0])    // 2.5
fs[1] = 9.5
```

Arrays are resizable at the runtime level but the literal `[]` and fixed
indexing keep a homogeneous type; use `arrays.push` and `arrays.pop` to grow
or shrink.

## The `arrays` module

The `arrays` module provides the standard array operations. It is available
without an explicit `use`. Mutating operations take the array in place and
return `void`; functional operations take a view, leave the input untouched,
and return a new owned array.

### Mutating in place

```ntsc
var a = [1]
arrays.push(a, 2)          // append; a is now [1, 2]
var last = arrays.pop(a)   // remove and return the last element
arrays.remove_at(a, 0)     // remove the element at an index
arrays.clear(a)            // remove all elements
arrays.shuffle(a)          // randomize in place
arrays.reverse(a)          // reverse in place
arrays.sort(a)             // sort in place
```

`arrays.push` returns `void`, so the call site does not reassign `a`; the array
handle is mutated directly and is never moved.

Arrays of strings (including untyped `[]` arrays and `array[any]`) own their
elements: `arrays.push`, `arrays.pop`, and index-assignment hand values to and
from the container without aliasing, so a string pushed into an array is safe
to reassign or drop at the call site afterwards. `arrays.pop` transfers
ownership of the removed string to the caller.

### Returning new arrays

```ntsc
var nums = [3, 1, 2]
var sorted = arrays.sort(nums)   // [1, 2, 3]; nums is untouched
var s = arrays.slice(nums, 1, 3) // elements 1..3
var cl = arrays.clone(nums)      // copy; string elements are deep-copied
```

### Query and element operations

```ntsc
say("" + arrays.length(a))    // int
say("" + arrays.isEmpty(a))   // bool
var i = arrays.index_of(a, 2)   // first index, or -1
say("" + arrays.contains(a, 5)) // bool
var e = arrays.get(a, 0)     // element at index (bounds-checked)
var f = arrays.flat(a)       // flatten nested arrays one level
```

`arrays.at` is an alias of `get`. `arrays.range(start, end)` builds an
`array[int]` of consecutive values, and `arrays.fill(value, count)` builds an
array with `count` copies of `value`.

## The iterator protocol

`for-in` iterates any value that provides two methods:

- `length() -> int`
- `get(int) -> T`

Arrays implement these natively. Custom classes implement them to participate:

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

The loop variable type is inferred from `get`'s return type, so a
`Words` class whose `get` returns `string` iterates as strings. The iterator
may be a function call, a field, or any expression.

## Ownership of loop variables

The loop variable is bound from `get`'s result. For heap element types the
element is moved or copied out according to the usual ownership rules; scalar
element types are copied. An owned container iterated by value is moved, so it
cannot be used after the loop unless it was borrowed:

```ntsc
fun total(array[int] xs) -> int {
    var s = 0
    for (var x in xs) { s = s + x }
    return s
}
```

Iterating an array through a `view` parameter leaves the caller's array
owned and untouched.
