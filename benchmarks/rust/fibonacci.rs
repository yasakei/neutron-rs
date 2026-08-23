// Fibonacci benchmark in Rust (iterative version)
// Matches neutron/benchmarks/neutron/fibonacci.nt

fn fibonacci(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    let mut a = 0;
    let mut b = 1;
    let mut i = 2;
    while i <= n {
        let temp = a + b;
        a = b;
        b = temp;
        i += 1;
    }
    b
}

fn main() {
    let result = fibonacci(35);
    println!("{result}");
}
