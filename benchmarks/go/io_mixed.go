// io_mixed.go — Go baseline for benchmarks/ntsc/io_mixed.nt: n_cpu goroutines
// count primes while n_io goroutines sleep 300 ms.
package main

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

func countPrimes(begin, end int64, ch chan int64) {
	var count int64
	for i := begin; i < end; i++ {
		if i >= 2 {
			prime := true
			for j := int64(2); j*j <= i; j++ {
				if i%j == 0 {
					prime = false
					break
				}
			}
			if prime {
				count++
			}
		}
	}
	ch <- count
}

func blockIO(ch chan int64) {
	time.Sleep(300 * time.Millisecond)
	ch <- 1
}

func main() {
	nCPU := int64(4)
	nIO := int64(64)
	if v := os.Getenv("NTS_BENCH_N"); v != "" {
		nCPU, _ = strconv.ParseInt(v, 10, 64)
	}
	if v := os.Getenv("NTS_BENCH_IO"); v != "" {
		nIO, _ = strconv.ParseInt(v, 10, 64)
	}
	const limit int64 = 400000
	step := limit / nCPU
	ch := make(chan int64, 64)
	for id := int64(0); id < nCPU; id++ {
		go countPrimes(id*step, (id+1)*step, ch)
	}
	for i := int64(0); i < nIO; i++ {
		go blockIO(ch)
	}
	var total int64
	for i := int64(0); i < nCPU+nIO; i++ {
		total += <-ch
	}
	fmt.Println(total)
}
