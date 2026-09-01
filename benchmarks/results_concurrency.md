# NTSC concurrency scaling results (vs Go goroutines)

Run: 2026-09-01 11:59:13, iterations=5, warmups=2, host cores=4, host caps: threads-max=57213, rlimit-nproc=28606.

## goroutine spawn/throughput (worker pool stays at one thread per CPU)

| Benchmark | Lang | Size | Best | RSS | max threads | KB/worker |
|---|---|---|---|---|---|---|
| goroutine_spawn | NTSC | 10000 | 65ms | 19.1MB | 5 | 1 |
| goroutine_spawn | Go | 10000 | 16ms | 29.9MB | 6 | 3 |
| goroutine_spawn | NTSC | 100000 | 658ms | 40.9MB | 5 | 0 |
| goroutine_spawn | Go | 100000 | 259ms | 265.1MB | 6 | 2 |

## CPU-bound fan-out (does g = cores use every worker?)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| cpu_fanout | NTSC | 1 | 110ms | 19.1MB | - |
| cpu_fanout | Go | 1 | 191ms | 19.1MB | - |
| cpu_fanout | NTSC | 4 | 63ms | 19.1MB | 5 |
| cpu_fanout | Go | 4 | 96ms | 19.1MB | 5 |
| cpu_fanout | NTSC | 16 | 59ms | 19.1MB | - |
| cpu_fanout | Go | 16 | 87ms | 19.1MB | - |

## CPU-bound goroutines while n_io block on async.sleep(300 ms)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| io_mixed | NTSC | 4c+64io | 304ms | 19.1MB | - |
| io_mixed | Go | 4c+64io | 304ms | 19.1MB | - |
| io_mixed | NTSC | 4c+512io | 314ms | 19.1MB | 6 |
| io_mixed | Go | 4c+512io | 308ms | 19.1MB | 6 |
| io_mixed | NTSC | 16c+512io | 312ms | 19.1MB | - |
| io_mixed | Go | 16c+512io | 321ms | 19.1MB | - |


HTTP server: Go net/http on local 37353.

## concurrent awaited http.fetch fan-out

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| http_fanout | NTSC | 16 | 5ms | 19.7MB | - |
| http_fanout | Go | 16 | 4ms | 19.7MB | - |
| http_fanout | NTSC | 128 | 15ms | 19.7MB | - |
| http_fanout | Go | 128 | 15ms | 19.7MB | - |
| http_fanout | NTSC | 512 | 59ms | 19.8MB | 9 |
| http_fanout | Go | 512 | 64ms | 26.2MB | 9 |

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
