//! NTSC standard library: `random` module.
//! Splitmix64 PRNG with per-thread state, seeded from the OS entropy source.

use std::cell::Cell;

use crate::registry;

thread_local! {
    // Per-thread PRNG state; SEEDED tracks whether STATE has been initialized.
    static STATE: Cell<u64> = const { Cell::new(0) };
    static SEEDED: Cell<bool> = const { Cell::new(false) };
}

/// Seed from the OS entropy source, falling back to the clock.
fn initial_seed() -> u64 {
    use std::io::Read;
    let mut bytes = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom")
        && f.read_exact(&mut bytes).is_ok()
    {
        return u64::from_le_bytes(bytes);
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(std::process::id() as u64)
}

/// Mix `STATE` and return the next 64-bit output (splitmix64).
fn next_u64() -> u64 {
    STATE.with(|state| {
        SEEDED.with(|seeded| {
            if !seeded.get() {
                state.set(initial_seed());
                seeded.set(true);
            }
        });
        let mut x = state.get();
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        state.set(x);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_seed(seed: i64) -> i8 {
    STATE.with(|state| {
        SEEDED.with(|seeded| {
            state.set(seed as u64);
            seeded.set(true);
        });
    });
    1
}

/// `random.int(min, max)` — inclusive range `[min, max]`; throws when
/// `max < min`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_int(min: i64, max: i64) -> i64 {
    let range = (max as i128) - (min as i128) + 1;
    if range <= 0 {
        super::throw_str(format!(
            "random.int: max ({max}) must be greater than or equal to min ({min})"
        ));
        return 0;
    }
    let offset = (next_u64() as u128) % (range as u128);
    (min as i128 + offset as i128) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_float() -> f64 {
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_bool() -> i8 {
    (next_u64() & 1) as i8
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_shuffle(handle: i64) -> i64 {
    let shuffled = crate::ntsc_array_shuffle(handle);
    if shuffled == registry::NULL {
        super::throw_str("random.shuffle: cannot shuffle array".to_string());
    }
    shuffled
}

/// `random.weighted(arr, mode)` — index picked by weight; `mode` selects the
/// element type: 0 = i64, 1 = f64 raw bits. Throws on empty, negative
/// weights, or a non-positive total.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_random_weighted(handle: i64, mode: i8) -> i64 {
    let len = registry::array_len(handle);
    if len == 0 {
        super::throw_str("random.weighted: weights array is empty".to_string());
    }
    let mut total_int: i128 = 0;
    let mut total_float: f64 = 0.0;
    match mode {
        0 => {
            for i in 0..len {
                let weight = registry::array_get(handle, i).unwrap_or(0);
                if weight < 0 {
                    super::throw_str("random.weighted: weights must be non-negative".to_string());
                }
                total_int += weight as i128;
            }
        }
        1 => {
            for i in 0..len {
                let weight = f64::from_bits(registry::array_get(handle, i).unwrap_or(0) as u64);
                if weight < 0.0 {
                    super::throw_str("random.weighted: weights must be non-negative".to_string());
                }
                total_float += weight;
            }
        }
        _ => {
            super::throw_str("random.weighted: unsupported element type".to_string());
        }
    }
    match mode {
        0 => {
            if total_int <= 0 {
                super::throw_str("random.weighted: sum of weights must be positive".to_string());
                return -1;
            }
            let pick = (next_u64() as u128) % (total_int as u128);
            let mut acc: u128 = 0;
            for i in 0..len {
                let weight = registry::array_get(handle, i).unwrap_or(0) as u128;
                acc += weight;
                if acc > pick {
                    return i;
                }
            }
        }
        _ => {
            if total_float <= 0.0 {
                super::throw_str("random.weighted: sum of weights must be positive".to_string());
                return -1;
            }
            let pick = next_u64() as f64 / (u64::MAX as f64) * total_float;
            let mut acc: f64 = 0.0;
            for i in 0..len {
                let weight = f64::from_bits(registry::array_get(handle, i).unwrap_or(0) as u64);
                acc += weight;
                if acc > pick {
                    return i;
                }
            }
        }
    }
    len - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_int_array(values: &[i64]) -> i64 {
        let arr = registry::array_new(8, 0, 0);
        for &v in values {
            registry::array_push(arr, v);
        }
        arr
    }

    #[test]
    fn test_seeded_deterministic() {
        ntsc_random_seed(42);
        let a = next_u64();
        ntsc_random_seed(42);
        let b = next_u64();
        assert_eq!(a, b);
    }

    #[test]
    fn test_int_in_range() {
        for _ in 0..1000 {
            let v = ntsc_random_int(5, 10);
            assert!((5..=10).contains(&v));
        }
        let _ = ntsc_random_int(i64::MIN, i64::MAX);
    }

    #[test]
    fn test_int_range_error_throws() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_random_int(10, 5);
        });
        let msg = err.unwrap();
        assert!(msg.contains("random.int"), "unexpected message: {msg}");
    }

    #[test]
    fn test_float_in_unit_interval() {
        for _ in 0..1000 {
            let v = ntsc_random_float();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn test_bool() {
        let mut saw_zero = false;
        let mut saw_one = false;
        for _ in 0..2000 {
            match ntsc_random_bool() {
                0 => saw_zero = true,
                1 => saw_one = true,
                other => panic!("unexpected bool value: {other}"),
            }
        }
        assert!(saw_zero && saw_one);
    }

    #[test]
    fn test_shuffle_keeps_elements() {
        let arr = make_int_array(&(0..10).collect::<Vec<i64>>());
        let shuffled = ntsc_random_shuffle(arr);
        let mut original: Vec<i64> = (0..10).collect();
        let mut values: Vec<i64> = (0..10)
            .map(|i| registry::array_get(shuffled, i).unwrap_or(0))
            .collect();
        original.sort_unstable();
        values.sort_unstable();
        assert_eq!(original, values);
        registry::array_drop(arr);
        registry::array_drop(shuffled);
    }

    #[test]
    fn test_weighted_int() {
        let arr = make_int_array(&[1, 1, 98]);
        let mut counts = [0; 3];
        for _ in 0..2000 {
            let idx = ntsc_random_weighted(arr, 0);
            assert!((0..3).contains(&idx));
            counts[idx as usize] += 1;
        }
        assert!(counts[2] > counts[0] * 10, "counts: {counts:?}");
        registry::array_drop(arr);
    }

    #[test]
    fn test_weighted_empty_throws() {
        use crate::modules::test_util::catch_throw;
        let arr = registry::array_new(8, 0, 0);
        let err = catch_throw(|| {
            let _ = ntsc_random_weighted(arr, 0);
        });
        let msg = err.unwrap();
        assert!(msg.contains("random.weighted"), "unexpected message: {msg}");
        registry::array_drop(arr);
    }
}
