// Prime number generation in Rust
// Matches neutron/benchmarks/neutron/primes.nt

fn is_prime(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 {
        return true;
    }
    if n % 2 == 0 {
        return false;
    }
    let mut i = 3;
    while i * i <= n {
        if n % i == 0 {
            return false;
        }
        i += 2;
    }
    true
}

fn generate_primes(limit: i64) -> Vec<i64> {
    let mut primes = Vec::new();
    let mut i = 2;
    while i <= limit {
        if is_prime(i) {
            primes.push(i);
        }
        i += 1;
    }
    primes
}

fn main() {
    let primes = generate_primes(1000);
    println!("{}", primes.len());
}
