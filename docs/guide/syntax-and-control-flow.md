# Syntax and control flow

## Lexical structure

### Comments

Two comment forms exist:

```ntsc
// Line comment, runs to the end of the line.

#{ This is a block comment. It does not nest. }
```

### Semicolons

Statement terminators are optional. The lexer inserts a semicolon where a
statement clearly ends at a newline, following the usual automatic semicolon
insertion rules. Both of these are valid:

```ntsc
var a = 1;
var b = 2
```

### String literals

Strings are written with double or single quotes. Literals are raw: a
backslash is stored as a backslash, so `"line one\n"` contains the five
characters `line one\n` and not a newline. Use the `strings` module functions
to build strings with control characters when needed.

Interpolation is written as `${expression}` inside a double-quoted string:

```ntsc
var name = "Ada"
say("Hello, ${name}!")
```

A raw string literal `r"..."` stores its content verbatim, which is convenient
for regular expressions and Windows paths.

Numbers are written in decimal notation. `1`, `1.5`, and `0.25` are all valid
literals; hexadecimal, octal, and binary literals are not currently supported.

### Identifiers and keywords

Identifiers start with a letter or underscore and continue with letters,
digits, or underscores.

Reserved words cannot be used as identifiers:

```
and       as        async     await     break
case      catch     class     continue  copy
default   do        elif      else      enum
false     finally   for       from      fun
if        in        int       match     mut
nil       or        option    retry     return
unsafe    say      shared    static    string
super     test      this      throw     true
try       use       var       view
while     quiet     bool      float     array
object    any
```

The type names (`int`, `float`, `bool`, `string`, `array`, `object`, `option`,
`any`) double as reserved words because type annotations use the prefix form.

## Variables

Declarations use `var`. A type annotation may precede the name; it is required
only where the initializer is ambiguous (the empty array literal, for
example).

```ntsc
var count = 42
var int total = 0
var float ratio = 0.5
var bool ok = true
var string name = "ada"
var array[int] xs = [1, 2, 3]
var option[int] maybe = nil
```

Without an initializer the variable starts at its zero value (`0`, `0.0`,
`false`, an empty string, `nil` for an option, an empty array for an array).

`static var` declares a value that survives across calls:

```ntsc
static var int calls = 0
```

## Operators

Precedence from highest to lowest:

| Precedence | Operators |
| --- | --- |
| call | `f(x)` `a.b` `a[i]` `a?.b` `x++` `x--` |
| unary | `-x` `!x` `~x` `...x` |
| multiplicative | `*` `/` `%` |
| additive | `+` `-` |
| shift | `<<` `>>` |
| relational | `<` `<=` `>` `>=` |
| equality | `==` `!=` |
| bitwise and | `&` |
| bitwise xor | `^` |
| bitwise or | `\|` |
| logical and | `&&` |
| logical or | `\|\|` |
| ternary | `cond ? then : else` |
| assignment | `=` |

### Arithmetic

`+`, `-`, `*`, `/` and `%` operate on `int` and `float` values. `+` also
concatenates strings and arrays. The postfix `++` and `--` operators increment
or decrement a variable in place and are legal on assignment targets only:

```ntsc
var n = 1
n++
say("" + n)   // 2
```

### Comparison and logic

`==`, `!=`, `<`, `<=`, `>`, `>=` compare scalars; `==` on heap values compares
identity, not contents. `&&` and `||` short-circuit. `!` negates a boolean.
The words `and` and `or` are not operators; use the symbols.

### Bitwise

`&`, `|`, `^`, `<<`, `>>` operate on `int`. `~` is the bitwise complement.

### Member access

`.` reads or assigns a field or method. `?.` is the optional chain: it
evaluates to `nil` (instead of throwing) when the receiver is `nil`.

### Ternary

```ntsc
var max = a > b ? a : b
```

## Control flow

### If / elif / else

```ntsc
if (score >= 90) {
    say("A")
} elif (score >= 80) {
    say("B")
} else {
    say("C")
}
```

### While and do-while

```ntsc
var i = 0
while (i < 10) {
    i = i + 1
}

var j = 0
do {
    j = j + 1
} while (j < 5)
```

### For

The C-style `for` accepts an optional initializer, condition, and step:

```ntsc
for (var i = 0; i < 5; i = i + 1) {
    say("" + i)
}
```

### For-in

`for-in` iterates a sequence using the iterator protocol. The loop variable
type is inferred from the element type:

```ntsc
var xs = [1, 2, 3]
for (var x in xs) {
    say("" + x)
}
```

See [Arrays and iterators](arrays-and-iterators.md) for the full protocol,
including how custom classes participate.

### Break and continue

`break` exits the innermost loop; `continue` starts its next iteration.

### Match

`match` dispatches on a value against a sequence of `case` arms, with an
optional `default`:

```ntsc
match (direction) {
    case "north" => say("up")
    case "south" => say("down")
    default => say("unknown")
}
```

Cases support patterns: literals, a wildcard `_`, variable bindings, and
destructuring patterns for arrays and objects:

```ntsc
match (value) {
    case 0 => say("zero")
    case 1 => say("one")
    case _ => say("many")
}
```

A `case` with a guard places `if <condition>` between the pattern and the
arrow. The first matching case wins; the match value is consumed once.

See [Classes and enums](classes.md) for matching on enum values.
