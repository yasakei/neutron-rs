# NTSC concurrency scaling results (vs Go goroutines)

Run: 2026-09-02 13:31:08, iterations=7, warmups=3, host cores=4, host caps: threads-max=57213, rlimit-nproc=28606.

## goroutine spawn/throughput (worker pool stays at one thread per CPU)

| Benchmark | Lang | Size | Best | RSS | max threads | KB/worker |
|---|---|---|---|---|---|---|
| goroutine_spawn | NTSC | 10000 | 47ms | 19.2MB | 5 | 1 |
| goroutine_spawn | Go | 10000 | 27ms | 27.0MB | 6 | 2 |
| goroutine_spawn | NTSC | 100000 | 435ms | 31.0MB | 5 | 0 |
| goroutine_spawn | Go | 100000 | 295ms | 264.9MB | 6 | 2 |

## CPU-bound fan-out (does g = cores use every worker?)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| cpu_fanout | NTSC | 1 | 111ms | 19.3MB | - |
| cpu_fanout | Go | 1 | 179ms | 19.3MB | - |
| cpu_fanout | NTSC | 4 | 52ms | 19.3MB | 5 |
| cpu_fanout | Go | 4 | 87ms | 19.3MB | 5 |
| cpu_fanout | NTSC | 16 | 65ms | 19.3MB | - |
| cpu_fanout | Go | 16 | 89ms | 19.3MB | - |

## CPU-bound goroutines while n_io block on async.sleep(300 ms)

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| io_mixed | NTSC | 4c+64io | 307ms | 19.3MB | - |
| io_mixed | Go | 4c+64io | 303ms | 19.3MB | - |
| io_mixed | NTSC | 4c+512io | 343ms | 19.3MB | 6 |
| io_mixed | Go | 4c+512io | 309ms | 19.3MB | 6 |
| io_mixed | NTSC | 16c+512io | 310ms | 19.3MB | - |
| io_mixed | Go | 16c+512io | 303ms | 19.3MB | - |


HTTP server: Go net/http on local 55153.

## concurrent awaited http.fetch fan-out

| Benchmark | Lang | Size | Best | RSS | max threads |
|---|---|---|---|---|---|
| http_fanout | NTSC | 16 | 13ms | 19.7MB | - |
| http_fanout | Go | 16 | 6ms | 19.7MB | - |
| http_fanout | NTSC | 128 | 13ms | 19.7MB | - |
| http_fanout | Go | 128 | 17ms | 19.7MB | - |
| http_fanout | NTSC | 512 | 54ms | 19.7MB | 9 |
| http_fanout | Go | 512 | 65ms | 27.4MB | 8 |

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
