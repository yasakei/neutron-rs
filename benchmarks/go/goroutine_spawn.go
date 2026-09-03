// goroutine_spawn.go — Go baseline for benchmarks/ntsc/goroutine_spawn.nt.
// Same workload: n goroutines, each sending a fixed sum over a 512-buffered
// channel (goroutines park when the buffer is full, like the NTSC version).
package main

import (
	"fmt"
	"os"
	"strconv"
)

func worker(ch chan int64) {
	var acc int64
	for i := int64(0); i < 64; i++ {
		acc += i
	}
	ch <- acc
}

func main() {
	n := 100000
	if v := os.Getenv("NTS_BENCH_N"); v != "" {
		n, _ = strconv.Atoi(v)
	}
	ch := make(chan int64, 512)
	for i := 0; i < n; i++ {
		go worker(ch)
	}
	var total int64
	for i := 0; i < n; i++ {
		total += <-ch
	}
	fmt.Println(total)
}
