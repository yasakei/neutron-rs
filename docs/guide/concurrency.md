# Concurrency

NTSC offers two concurrency models: OS threads with message-passing channels,
and cooperative `async`/`await` coroutines. `async` is what the name suggests;
threads use `process.spawn_thread` and the `collections` channel functions.

## Threads

`process.spawn_thread(body, arg)` starts a new OS thread. The body is a lambda
with a single parameter; the argument is passed to it. It returns a handle
that `process.thread_join` waits on:

```ntsc
var tx = collections.channel_sender(rx)
var producer = process.spawn_thread(fun(int tx) {
    collections.channel_send(tx, "ping")
    collections.channel_send(tx, "pong")
    collections.channel_close(tx)
}, tx)

process.thread_join(producer)
```

### Channels

Channels are message queues with a bounded capacity. Handles are integers.
`collections.channel(capacity)` creates a queue and returns the receiver;
`collections.channel_sender(rx)` creates the matching sender:

```ntsc
var rx = collections.channel(4)
var tx = collections.channel_sender(rx)
```

- `collections.channel_send(tx, msg)` blocks until space is available. The
  message is a string: the runtime copies its text into the queue, so the
  sender keeps its own value and the receiver gets independent data.
- `collections.channel_recv(rx)` blocks until a message arrives.
- `collections.channel_try_recv(rx)` checks without blocking: it returns the
  pending message, or the empty string when the queue is empty or every sender
  end is closed.
- `collections.channel_close(handle)` closes a queue end.

Every handle must eventually be closed.

### Threading rules

NTSC has no `Send`/`Sync` traits to implement. Instead the compiler classifies
every value it hands to another thread, and the classification is total — there
is nothing to opt into and no escape hatch. A value crosses a thread boundary
when it is an argument to `process.spawn_thread` (the payload) or to
`collections.channel_send` (the message).

| Value | Crosses a thread boundary? | Why |
| --- | --- | --- |
| `int`, `float`, `bool` and other scalars | Yes | Copied by value; the two threads share nothing. |
| Stdlib handles (channels, files, sockets, threads) | Yes | A handle is an `int`. The resource behind it lives in the runtime registry, which is mutex-guarded, so concurrent access is synchronized. |
| Function references | Yes | Code is immutable, and a lambda cannot capture anything, so a function value carries no state. |
| Owned heap values (`string`, `array`, `object`, class instances) | Through a channel only | `collections.channel_send` copies the text across, so each side owns independent data. `process.spawn_thread` does not: the payload would arrive as a raw handle that both threads alias without synchronization, and the caller's scope exit would free it while the thread is still running. |
| `shared` values | No | The reference count is not synchronized, so two threads holding copies would race on both the value and its count. Send the data through a channel instead. |
| `view` / `view mut` | No | A borrow lives only as long as the borrowing scope, which does not have to outlive the thread that receives it. |

So the supported shape is the one in the example above: create the channel in
the parent, pass only the channel handle to `process.spawn_thread`, and move
the data itself through `collections.channel_send`. Violations are ownership
errors (`NTSC-E0501`) reported at compile time:

```ntsc
fun main() {
    var xs = [1, 2]
    process.spawn_thread(worker, xs)
    // error[NTSC-E0501]: cannot pass `xs` to process.spawn_thread: an owned
    // heap value would cross as a raw handle that both threads then alias
    // without synchronization, ...
}
```

A user-defined type is thread-safe exactly when its values are: a class
instance is an owned heap value, so it follows that row of the table. There is
no way to declare otherwise, which is why the rules above are exhaustive.

`await` is not a thread boundary. The executor polls every future on one
thread, so a suspension never moves a value between threads and values that
cannot cross threads are still free to live across an `await`.

## Async / await

An `async fun` is a coroutine that can suspend at `await` points and resume
later. The compiler lowers it to a poll-based state machine driven by a
single-threaded runtime executor; a suspension yields the thread, so async
code is safe to use alongside threads.

```ntsc
async fun fetch(string url) -> string {
    await async.sleep(10)
    return "data for " + url
}

async fun main() -> int {
    var n = 1
    await async.sleep(1)
    n = n + 1
    var first = await fetch("a")
    var second = await fetch("b")
    say(first)
    say(second)
    return 0
}
```

### Rules

- `async fun main` is supported and drives the root future; its `int` result
  becomes the process exit code.
- Local variables survive suspension: values written before an `await` are
  intact afterwards.
- Await results flow back into the calling coroutine after the child future
  completes.
- `try`, `throw`, and `retry` are rejected inside async bodies: exceptions
  cannot unwind across a suspended state machine.

### The `async` module

`async.sleep(ms)` is the one suspending function. It pauses the coroutine for
approximately `ms` milliseconds: the future reports "pending" until the
deadline passes, and the executor re-polls it on a millisecond quantum.
