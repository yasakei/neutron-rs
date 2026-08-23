// Bubble sort benchmark in Rust
// Matches neutron/benchmarks/neutron/sorting.nt

fn bubble_sort(arr: &mut [i64]) {
    let n = arr.len();
    let mut i = 0;
    while i < n {
        let mut j = 0;
        while j < n - i - 1 {
            if arr[j] > arr[j + 1] {
                arr.swap(j, j + 1);
            }
            j += 1;
        }
        i += 1;
    }
}

fn main() {
    let mut arr = vec![64, 34, 25, 12, 22, 11, 90, 88, 45, 50, 33, 77, 99, 18, 7];
    bubble_sort(&mut arr);
    println!("{} {} {} {} {}", arr[0], arr[1], arr[2], arr[3], arr[4]);
}
