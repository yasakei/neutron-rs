# Standard library reference

The standard library is exposed as modules callable directly from NTSC source;
every module name is predeclared. Functions follow the `module.func` naming
convention. Return types below are as seen by NTSC source; runtime `bool`
results are normalized to `bool` and scalar results to `int`/`float`.

Unless noted, a function that fails throws an exception whose message starts
with `module.func:`. Functions that previously returned a default value on
failure now throw instead.

## Arrays

The `arrays` module operates on the runtime heap array type produced by array
literals. Mutating operations take the array in place and return `void`;
functional operations take a view and return a new owned array. A `shared
array[T]` argument is borrowed for the operation.

| Function | Returns | Behavior |
| --- | --- | --- |
| `arrays.new()` | `array[any]` | Creates an empty untyped array. |
| `arrays.length(a)` | `int` | Number of elements. |
| `arrays.isEmpty(a)` | `bool` | `true` when the array is empty. |
| `arrays.get(a, i)` / `arrays.at(a, i)` | `T` | Element at index; bounds-checked. |
| `arrays.push(a, v)` | `void` | Appends `v` in place. |
| `arrays.pop(a)` | `T` | Removes and returns the last element. |
| `arrays.remove_at(a, i)` | `void` | Removes the element at `i`. |
| `arrays.remove(a, v)` | `void` | Removes the first element equal to `v`. |
| `arrays.clear(a)` | `void` | Removes all elements. |
| `arrays.reverse(a)` | `void` | Reverses in place. |
| `arrays.sort(a)` | `void` | Sorts in place. |
| `arrays.shuffle(a)` | `void` | Randomizes in place. |
| `arrays.contains(a, v)` | `bool` | Whether `v` is present. |
| `arrays.index_of(a, v)` | `int` | First index of `v`, or `-1`. |
| `arrays.slice(a, start, end)` | `array[T]` | New array of elements `start..end`. |
| `arrays.clone(a)` | `array[T]` | New array with the same elements. |
| `arrays.flat(a)` | `array[T]` | Nested arrays flattened one level. |
| `arrays.range(start, end)` | `array[int]` | `[start, start+1, ..., end-1]`. |
| `arrays.fill(v, count)` | `array[T]` | `count` copies of `v`. |

## Strings

Strings are owned UTF-8 values. String indexing is byte-based.

| Function | Returns | Behavior |
| --- | --- | --- |
| `strings.length(s)` | `int` | Length in bytes. |
| `strings.is_empty(s)` | `bool` | Whether the string is empty. |
| `strings.substring(s, start, end)` | `string` | Bytes `start..end`. |
| `strings.index_of(s, sub)` | `int` | First byte offset of `sub`, or `-1`. |
| `strings.last_index_of(s, sub)` | `int` | Last byte offset of `sub`, or `-1`. |
| `strings.contains(s, sub)` | `bool` | Whether `s` contains `sub`. |
| `strings.starts_with(s, prefix)` | `bool` | Prefix test. |
| `strings.ends_with(s, suffix)` | `bool` | Suffix test. |
| `strings.count(s, sub)` | `int` | Number of non-overlapping occurrences. |
| `strings.replace(s, from, to)` | `string` | Replace all occurrences. |
| `strings.replace_first(s, from, to)` | `string` | Replace the first occurrence. |
| `strings.trim(s)` / `trim_left` / `trim_right` | `string` | Strip whitespace. |
| `strings.upper(s)` / `strings.lower(s)` | `string` | Case conversion. |
| `strings.reverse(s)` | `string` | Reversed string. |
| `strings.repeat(s, n)` | `string` | `s` repeated `n` times. |
| `strings.split(s, delim)` | `string` | Split by `delim` (or per character when empty), joined by newlines. |
| `strings.join(a, b, delim)` | `string` | `a` + `delim` + `b`. |
| `strings.char_at(s, i)` | `string` | Single-character string at byte `i`. |
| `strings.char_code(s, i)` | `int` | Code point at byte `i`. |
| `strings.from_char_code(code)` | `string` | String for a code point. |
| `strings.is_alpha(s)` / `is_digit` / `is_alnum` | `bool` | Character class tests (first character). |

## Formatting

| Function | Returns | Behavior |
| --- | --- | --- |
| `fmt.to_int(s)` | `int` | Parses an integer; throws on failure. |
| `fmt.to_float(s)` | `float` | Parses a float; throws on failure. |
| `fmt.i64_to_str(n)` | `string` | Integer to string. |
| `fmt.f64_to_str(f)` | `string` | Float to string. |
| `fmt.to_hex(n)` | `string` | Hexadecimal representation. |
| `fmt.to_oct(n)` | `string` | Octal representation. |
| `fmt.is_int(s)` | `bool` | Whether `s` parses as an integer. |
| `fmt.is_float(s)` | `bool` | Whether `s` parses as a float. |
| `fmt.type_name(tag)` | `string` | Name of the type for a runtime type tag. |
| `fmt.pad_left(s, width)` | `string` | Left-pad to `width`. |
| `fmt.pad_right(s, width)` | `string` | Right-pad to `width`. |

## Math

All `math` functions operate on and return `float`. `math.sqrt` throws on a
negative argument.

| Function | Behavior |
| --- | --- |
| `math.sqrt(x)` | Square root. |
| `math.pow(base, exp)` | Power. |
| `math.abs(x)` | Absolute value. |
| `math.ceil(x)` / `math.floor(x)` / `math.round(x)` | Rounding. |
| `math.sin(x)` / `math.cos(x)` / `math.tan(x)` | Trigonometry. |

## System

| Function | Returns | Behavior |
| --- | --- | --- |
| `sys.read(path)` | `string` | Reads a file; throws on failure. |
| `sys.write(path, content)` | `bool` | Writes a file; throws on failure. |
| `sys.append(path, content)` | `bool` | Appends to a file. |
| `sys.exists(path)` | `bool` | Whether the path exists. |
| `sys.mkdir(path)` | `bool` | Creates a directory. |
| `sys.listdir(path)` | `string` | Directory entries. |
| `sys.cwd()` | `string` | Current working directory. |
| `sys.env(name)` | `string` | Environment variable value. |
| `sys.args()` | `string` | Command-line arguments. |
| `sys.exit(code)` | `void` | Terminates the process. |
| `sys.sleep(ms)` | `void` | Sleeps for `ms` milliseconds. |
| `sys.exec(command)` | `int` | Runs a command; returns its exit status. |
| `sys.cp(src, dst)` | `bool` | Copies a file. |
| `sys.rm(path)` | `bool` | Removes a file. |

## Time

| Function | Returns | Behavior |
| --- | --- | --- |
| `time.now()` | `float` | Milliseconds since the Unix epoch. |
| `time.sleep(ms)` | `void` | Sleeps for `ms` milliseconds. |
| `time.format(timestamp_ms, fmt)` | `string` | Formats a timestamp. |

## JSON

`json` operates on JSON text strings and returns JSON text or plain strings.

| Function | Returns | Behavior |
| --- | --- | --- |
| `json.parse(s)` | `string` | Validates and normalizes; throws on invalid input. |
| `json.stringify(v)` | `string` | Serializes a value. |
| `json.is_valid(s)` | `bool` | Whether the text is valid JSON. |
| `json.get(json, key)` | `string` | Extracts a key's value. |
| `json.has(json, key)` | `bool` | Whether a key exists. |
| `json.keys(json)` | `string` | Key list. |
| `json.stringify_pretty(json)` | `string` | Pretty-prints. |
| `json.escape_string(s)` | `string` | Escapes a string for embedding in JSON. |

## HTTP

`http` performs blocking requests. On network or HTTP failure the request
functions throw. `http://` URLs are sent over a plain TCP connection;
`https://` URLs are encrypted with TLS (rustls) and verified against Mozilla's
bundled CA root certificates.

| Function | Returns | Behavior |
| --- | --- | --- |
| `http.get(url)` | `string` | GET request body. |
| `http.post(url, data)` | `string` | POST request. |
| `http.put(url, data)` | `string` | PUT request. |
| `http.delete(url)` | `string` | DELETE request. |
| `http.head(url)` | `string` | HEAD request. |
| `http.patch(url, data)` | `string` | PATCH request. |
| `http.request(method, url, data)` | `string` | Arbitrary method. |
| `http.status_code(response)` | `int` | Status code of a response string. |

## Collections

### Sets

| Function | Returns | Behavior |
| --- | --- | --- |
| `collections.set_new()` | `object` | New empty set. |
| `collections.set_add(set, v)` | `bool` | Adds a value. |
| `collections.set_has(set, v)` | `bool` | Membership test. |
| `collections.set_remove(set, v)` | `bool` | Removes a value. |
| `collections.set_size(set)` | `int` | Number of elements. |
| `collections.set_to_array(set)` | `string` | Elements as a list. |
| `collections.set_union(a, b)` | `object` | Union. |
| `collections.set_intersection(a, b)` | `object` | Intersection. |
| `collections.set_difference(a, b)` | `object` | Difference. |

### Stacks and queues

| Function | Returns | Behavior |
| --- | --- | --- |
| `collections.stack_new()` | `object` | New empty stack. |
| `collections.stack_push(s, v)` | `bool` | Push. |
| `collections.stack_pop(s)` | `bool` | Pop. |
| `collections.stack_peek(s)` | `bool` | Inspect top. |
| `collections.stack_size(s)` | `int` | Size. |
| `collections.stack_is_empty(s)` | `bool` | Empty test. |
| `collections.queue_new()` | `object` | New empty queue. |
| `collections.queue_enqueue(q, v)` | `bool` | Enqueue. |
| `collections.queue_dequeue(q)` | `bool` | Dequeue. |
| `collections.queue_peek(q)` | `bool` | Inspect front. |
| `collections.queue_size(q)` | `int` | Size. |
| `collections.queue_is_empty(q)` | `bool` | Empty test. |

### Channels

See [Concurrency](../guide/concurrency.md) for usage.

| Function | Returns | Behavior |
| --- | --- | --- |
| `collections.channel(capacity)` | `int` | Creates a queue; returns the receiver handle. |
| `collections.channel_sender(rx)` | `int` | Creates the sender for a receiver. |
| `collections.channel_send(tx, msg)` | `bool` | Copies the string `msg` into the queue, blocking when full. |
| `collections.channel_recv(rx)` | `string` | Receives, blocking when empty. |
| `collections.channel_try_recv(rx)` | `string` | Non-blocking receive; the empty string when nothing is pending. |
| `collections.channel_close(handle)` | `void` | Closes a queue end. |

## Regular expressions

| Function | Returns | Behavior |
| --- | --- | --- |
| `regex.test(text, pattern)` | `bool` | Whether the pattern matches. |
| `regex.search(text, pattern)` | `bool` | Whether the pattern appears. |
| `regex.find(text, pattern)` | `string` | First match; throws on an invalid pattern. |
| `regex.find_all(text, pattern)` | `string` | All matches. |
| `regex.replace(text, pattern, rep)` | `string` | Replace matches. |
| `regex.split(text, pattern)` | `string` | Split on matches. |
| `regex.is_valid(pattern)` | `bool` | Pattern validity. |
| `regex.escape(text)` | `string` | Escapes literal text. |

## I/O

File handles are integers. `io.open` throws on failure.

| Function | Returns | Behavior |
| --- | --- | --- |
| `io.input()` | `string` | Reads one line from standard input without its line ending; returns `""` at EOF. |
| `io.stdin()` | `int` | Stable standard-input handle for `io.read*`. |
| `io.stdout()` | `int` | Stable standard-output handle for `io.write*` and `io.flush`. |
| `io.stderr()` | `int` | Stable standard-error handle for `io.write*` and `io.flush`. |
| `io.open(path, mode)` | `int` | Opens a file (`"r"`, `"w"`, `"w+"`, ...). |
| `io.close(handle)` | `bool` | Closes a file. |
| `io.read(handle, count)` | `string` | Reads up to `count` bytes. |
| `io.read_line(handle)` | `string` | Reads one line. |
| `io.read_all(handle)` | `string` | Reads the rest of the file. |
| `io.write(handle, data)` | `int` | Writes bytes; returns the count written. |
| `io.write_line(handle, data)` | `int` | Writes a line. |
| `io.flush(handle)` | `bool` | Flushes buffers. |
| `io.eof(handle)` | `bool` | End-of-file test. |
| `io.seek(handle, offset, whence)` | `bool` | Seeks (`whence` 0 = start). |
| `io.tell(handle)` | `int` | Current position. |

### Reading standard input

`io.input()` blocks until it reads a line from the process's standard input or
reaches end-of-file. It removes the trailing line ending (`\n` or `\r\n`) and
returns the remaining text as a string:

```ntsc
fun main() {
    say("What is your name?")
    var name = io.input()
    say("Hello, " + name)
}
```

An empty string is returned for both a blank line and end-of-file; `io.input()`
does not distinguish between them. If standard input cannot be read, it throws
an exception whose message starts with `io.input:`:

```ntsc
try {
    var line = io.input()
    say("Read: " + line)
} catch (err) {
    say(err)
}
```

Each call reads one line. The returned string is an ordinary owned NTSC string
and requires no manual cleanup.

### Standard streams

`io.stdin()`, `io.stdout()`, and `io.stderr()` return process-owned stream
handles. They use the same operations as file handles:

```ntsc
fun main() {
    var input = io.stdin()
    var output = io.stdout()
    var errors = io.stderr()

    io.write(output, "Name: ")
    io.flush(output)
    var name = strings.trim(io.read_line(input))
    io.write_line(output, "Hello, " + name)
    io.write_line(errors, "This line is written to stderr")
}
```

Standard stream handles are stable, allocate no registry objects, and are
owned by the process. Calling `io.close` on one is a successful no-op. Standard
input supports `io.read`, `io.read_line`, and `io.read_all`; standard output
and standard error support `io.write`, `io.write_line`, and `io.flush`.
Operations unsupported by the stream direction throw an `io.*` exception.
Standard streams are not seekable, so `io.seek`, `io.tell`, and `io.eof` throw
when given one of these handles.

## Networking

Sockets are integer handles. `net.tcp_connect` throws on connection failure.

| Function | Returns | Behavior |
| --- | --- | --- |
| `net.tcp_connect(host, port)` | `int` | Connects; returns a socket. |
| `net.tcp_listen(port)` | `int` | Listens; port `0` picks an ephemeral port. |
| `net.local_port(handle)` | `int` | Bound port of a listener. |
| `net.tcp_accept(listener)` | `int` | Accepts a connection. |
| `net.send(handle, data)` | `int` | Sends bytes. |
| `net.send_line(handle, data)` | `int` | Sends a line. |
| `net.recv(handle, count)` | `string` | Receives up to `count` bytes. |
| `net.recv_line(handle)` | `string` | Receives a line. |
| `net.close(handle)` | `bool` | Closes a socket. |
| `net.udp_bind(port)` | `int` | Binds a UDP socket. |
| `net.udp_send(handle, host, port, data)` | `int` | Sends a datagram. |
| `net.udp_recv(handle, count)` | `string` | Receives a datagram. |

## Operating system

| Function | Returns | Behavior |
| --- | --- | --- |
| `os.getenv(name)` | `string` | Environment variable (empty when unset). |
| `os.setenv(name, value)` | `bool` | Sets an environment variable. |
| `os.unsetenv(name)` | `bool` | Removes an environment variable. |
| `os.has_env(name)` | `bool` | Environment variable test. |
| `os.path_join(a, b)` | `string` | Joins path components. |
| `os.path_dirname(p)` | `string` | Directory part. |
| `os.path_basename(p)` | `string` | Final component. |
| `os.path_ext(p)` | `string` | Extension without the dot. |
| `os.path_stem(p)` | `string` | Filename without extension. |
| `os.path_abs(p)` | `string` | Absolute path. |
| `os.is_abs(p)` | `bool` | Absolute path test. |
| `os.separator()` | `string` | Path separator. |
| `os.temp_dir()` | `string` | Temporary directory. |
| `os.temp_path(prefix)` | `string` | A unique temp path. |
| `os.temp_file(prefix)` | `string` | Creates and returns a temp file path. |

## Crypto

`crypto.base64_decode`, `crypto.hex_decode`, and the hash functions throw on
invalid input.

| Function | Returns | Behavior |
| --- | --- | --- |
| `crypto.base64_encode(s)` | `string` | Base64 encode. |
| `crypto.base64_decode(s)` | `string` | Base64 decode. |
| `crypto.hex_encode(s)` | `string` | Hex encode. |
| `crypto.hex_decode(hex)` | `string` | Hex decode. |
| `crypto.sha256(s)` | `string` | SHA-256 digest (hex). |
| `crypto.random_bytes(count)` | `string` | Random bytes. |
| `crypto.random_string(length, alphabet)` | `string` | Random string. |
| `crypto.xor_cipher(data, key)` | `string` | XOR cipher. |

## Encoding

| Function | Returns | Behavior |
| --- | --- | --- |
| `encoding.base64_encode(s)` | `string` | Base64 encode. |
| `encoding.base64_decode(s)` | `string` | Base64 decode. |
| `encoding.hex_encode(s)` | `string` | Hex encode. |
| `encoding.hex_decode(hex)` | `string` | Hex decode. |
| `encoding.utf8_valid(s)` | `bool` | Whether the string is valid UTF-8. |

## Hash

| Function | Returns | Behavior |
| --- | --- | --- |
| `hash.sha256(s)` | `string` | SHA-256 digest (hex). |
| `hash.crc32(s)` | `int` | CRC-32 checksum. |

## Random

| Function | Returns | Behavior |
| --- | --- | --- |
| `random.seed(seed)` | `bool` | Seeds the generator. |
| `random.int(min, max)` | `int` | Integer in `[min, max)`; throws on inverted bounds. |
| `random.float()` | `float` | Float in `[0.0, 1.0)`. |
| `random.bool()` | `bool` | Random boolean. |
| `random.shuffle(a)` | `array[T]` | New array, elements shuffled. |
| `random.weighted(weights)` | `int` | Weighted index; throws when weights are all zero. |

## Sorting

`sort` clones its input into a new array; the input is never consumed.

| Function | Returns | Behavior |
| --- | --- | --- |
| `sort.stable_sort(a)` | `array[T]` | Stable ascending sort. |
| `sort.sort_by(a, cmp)` | `array[T]` | Sort with a comparator lambda. |
| `sort.binary_search(a, v)` | `int` | Index of `v`, or `-1`. |

`sort.sort_by` takes a lambda `fun(T a, T b) -> bool` returning whether `a`
should precede `b`.

## Slices

A `slice[T]` is a bounds-checked window over an `array[T]`. It holds the source
array handle plus the window rather than a pointer, so every operation
re-validates that the array is still registered and that the index lies inside
the window. Ranges are half-open (`start..end`), and every function throws when
its range or index is out of bounds.

| Function | Returns | Behavior |
| --- | --- | --- |
| `slices.of(a, start, end)` | `slice[T]` | Window over `a`; throws when the range leaves the array. |
| `slices.sub(s, start, end)` | `slice[T]` | Narrows `s`; bounds are relative to `s`, so it cannot widen. |
| `slices.length(s)` | `int` | Number of elements in the window. |
| `slices.get(s, i)` | `T` | Element at `i` within the window. `s[i]` is equivalent. |
| `slices.set(s, i, v)` | `bool` | Writes `v` through to the underlying array. |
| `slices.to_array(s)` | `array[T]` | Fresh owned array of the spanned elements. |
| `slices.fill(s, v)` | `bool` | Sets every element in the window to `v`. |
| `slices.copy_from(dst, src)` | `bool` | Element-wise copy; requires equal lengths. |
| `slices.equal(a, b)` | `bool` | Whether both windows hold equal elements. |

A slice owns its own registry entry and is reclaimed at scope exit; it never
frees the array it borrows. `copy(s)` is equivalent to `slices.to_array(s)`.
Slices cannot cross a thread boundary.

## Memory

The `memory` module provides bounds-checked byte-buffer capabilities. A
`pointer` value is an opaque capability into a runtime-managed byte region, not
a machine address: it cannot be converted to an integer, dereferenced with `*`,
or used to reach outside the allocation it points into. All access goes through
the functions below, each of which bounds-checks its pointer and throws on a
stale or invalid capability.

| Function | Returns | Behavior |
| --- | --- | --- |
| `memory.alloc(size)` | `pointer` | Allocates a zeroed region of `size` bytes (`0..=16777216`). |
| `memory.offset(p, delta)` | `pointer` | A derived capability at `p + delta`; throws when out of bounds. |
| `memory.clone(p)` | `pointer` | A second capability into the same allocation. |
| `memory.drop(p)` | `void` | Releases one capability; the allocation lives until all capabilities are dropped. |
| `memory.load8(p)` | `int` | Reads one byte (0..255). |
| `memory.load64(p)` | `int` | Reads eight bytes, little-endian. |
| `memory.store8(p, v)` | `bool` | Writes one byte; `v` must be in 0..255. |
| `memory.store64(p, v)` | `bool` | Writes eight bytes, little-endian. |

Capabilities follow ordinary ownership: assignment moves them, `copy(p)`
clones a capability, and the value is dropped at scope exit. The allocation's
storage is freed only when its last capability is dropped, so an offset
capability keeps the whole region alive.

Capability access resolves the region through a per-thread cache, so a load or
store in a loop does not take the registry lock on every access; any handle
removal invalidates the cache, so a dropped capability is never served from it.
Typed pointers (`own T`, `&T`, `*T`) bypass the registry entirely and compile to
a plain load or store.

`memory.raw_address(reference)` is the typed-pointer escape hatch: it converts
`&T` to `*const T` and `&mut T` to `*mut T`. It is only allowed inside an
`unsafe` block, and the resulting pointer keeps its pointee type. See
[Pointers and references](language.md#pointers-and-references).

## Process

| Function | Returns | Behavior |
| --- | --- | --- |
| `process.exec(command)` | `int` | Runs a command; returns its exit status. |
| `process.exec_output(command)` | `string` | Runs a command; returns its output. |
| `process.spawn(command, args)` | `string` | Spawns a process. |
| `process.pid()` | `int` | Current process id. |
| `process.spawn_thread(body, arg)` | `int` | Starts an OS thread; returns its handle. `arg` must be thread-safe. |
| `process.thread_join(id)` | `bool` | Waits for a thread to finish. |

`process.spawn_thread` takes a lambda with one `int` parameter. Its payload
must be a scalar or a stdlib handle: owned heap values, `shared` values, and
views are rejected at compile time. See
[Concurrency](../guide/concurrency.md#threading-rules) for the full
classification and the channel-handle pattern it implies.

## Testing

Assertions throw on failure. The test runner also uses them.

| Function | Returns | Behavior |
| --- | --- | --- |
| `testing.assert_true(b)` | `bool` | Passes when `b` is `true`. |
| `testing.assert_false(b)` | `bool` | Passes when `b` is `false`. |
| `testing.assert_eq(a, b)` | `bool` | Passes when `a == b` (`int`, `float`, `bool`, or `string`). |
| `testing.assert_ne(a, b)` | `bool` | Passes when `a != b`. |

## Async

| Function | Returns | Behavior |
| --- | --- | --- |
| `async.sleep(ms)` | `int` | Suspends the coroutine for approximately `ms` milliseconds. |

## Unused legacy `arrays` module

A separate newline-delimited string implementation of the `arrays` module
exists in the runtime for backwards compatibility, but it is not used: every
`arrays.*` call in NTSC source is routed to the heap array operations above.
