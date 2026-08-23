// Matrix multiplication benchmark in Rust
// 20x20 integer matrix multiplication - cumulative sum of all products
// Matches rewrite/benchmarks/ntsc/matrix.nt exactly

fn main() {
    let size = 20;
    let mut total: i64 = 0;
    let mut i = 0;
    while i < size {
        let mut j = 0;
        while j < size {
            let mut k = 0;
            while k < size {
                total += (i * size + k + 1) * (k * size + j + 1);
                k += 1;
            }
            j += 1;
        }
        i += 1;
    }
    println!("{total}");
}
