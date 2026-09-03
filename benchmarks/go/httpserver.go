// httpserver.go — local HTTP server for the http_fanout benchmark. A Go
// server is the fair peer for the Go client baseline; Python's
// ThreadingHTTPServer stalls Go's keep-alive connection pool.
package main

import (
	"flag"
	"fmt"
	"net/http"
)

func main() {
	port := flag.Int("port", 8080, "listen port")
	flag.Parse()
	body := make([]byte, 505)
	for i := range body {
		body[i] = 'x'
	}
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Length", fmt.Sprint(len(body)))
		w.Header().Set("Connection", "close")
		w.Write(body)
	})
	http.ListenAndServe(fmt.Sprintf("127.0.0.1:%d", *port), nil)
}
