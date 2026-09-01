# NTSC concurrency scaling results (vs Go goroutines)

Run: 2026-09-01 13:55:59, iterations=5, warmups=2, host cores=4, host caps: threads-max=57213, rlimit-nproc=28606.

## goroutine spawn/throughput (worker pool stays at one thread per CPU)

| Benchmark | Lang | Size | Best | RSS | max threads | KB/worker |
|---|---|---|---|---|---|---|
| goroutine_spawn | NTSC | 10000 | 62ms | 19.3MB | 5 | 1 |
| goroutine_spawn | Go | 10000 | 16ms | 29.9MB | 6 | 3 |
| goroutine_spawn | NTSC | 100000 | 640ms | 30.7MB | 5 | 0 |
| goroutine_spawn | Go | 100000 | 306ms | 265.4MB | 6 | 2 |

## CPU-bound fan-out (does g = cores use every worker?)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| cpu_fanout | NTSC | 1 | 75ms | 19.3MB | - |
| cpu_fanout | Go | 1 | 194ms | 19.3MB | - |
| cpu_fanout | NTSC | 4 | 37ms | 19.3MB | 5 |
| cpu_fanout | Go | 4 | 91ms | 19.3MB | 5 |
| cpu_fanout | NTSC | 16 | 36ms | 19.3MB | - |
| cpu_fanout | Go | 16 | 86ms | 19.3MB | - |

## CPU-bound goroutines while n_io block on async.sleep(300 ms)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| io_mixed | NTSC | 4c+64io | 309ms | 19.3MB | - |
| io_mixed | Go | 4c+64io | 304ms | 19.3MB | - |
| io_mixed | NTSC | 4c+512io | 317ms | 19.3MB | 6 |
| io_mixed | Go | 4c+512io | 307ms | 19.3MB | 6 |
| io_mixed | NTSC | 16c+512io | 345ms | 19.3MB | - |
| io_mixed | Go | 16c+512io | 376ms | 19.3MB | - |


HTTP server: Go net/http on local 49051.

## concurrent awaited http.fetch fan-out

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| http_fanout | NTSC | 16 | 14ms | 19.9MB | - |
| http_fanout | Go | 16 | 11ms | 19.9MB | - |
| http_fanout | NTSC | 128 | 15ms | 19.9MB | - |
| http_fanout | Go | 128 | 14ms | 19.9MB | - |
| http_fanout | NTSC | 512 | 54ms | 19.9MB | 9 |
| http_fanout | Go | 512 | 72ms | 28.2MB | 9 |

## Notes

- Both runtimes multiplex goroutines onto a fixed OS-thread pool: the "max
  threads" column stays near one per CPU at any goroutine count. This host
  (cores=4, kernel allows 57213 OS threads) runs 100k goroutines on the
  pool; "KB/worker" is peak RSS / goroutine count.
- cpu_fanout: g=1 -> g=cores shows the fan-out uses every core; at
  g = 4*cores work stealing keeps all workers even.
- io_mixed: wall time stays ~300 ms at any n_io — blocked goroutines park on
  timers and free the worker instead of pinning it.
- http_fanout: in-flight requests are decoupled from OS thread count; the
  blocking request is offloaded to a bounded pool while the goroutine parks.
