// cpu_fanout.go — Go baseline for benchmarks/ntsc/cpu_fanout.nt: prime
// counting split across g goroutines.
package main

import (
	"fmt"
	"os"
	"strconv"
)

func countPrimes(begin, end int64) int64 {
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
	return count
}

func main() {
	g := int64(8)
	if v := os.Getenv("NTS_BENCH_N"); v != "" {
		g, _ = strconv.ParseInt(v, 10, 64)
	}
	const limit int64 = 400000
	step := limit / g
	ch := make(chan int64, g)
	for id := int64(0); id < g; id++ {
		go func(id int64) {
			ch <- countPrimes(id*step, (id+1)*step)
		}(id)
	}
	var total int64
	for i := int64(0); i < g; i++ {
		total += <-ch
	}
	fmt.Println(total)
}
