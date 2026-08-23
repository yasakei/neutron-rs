//! NTSC standard library: `arrays` module.
//! Shims over the registry-backed array handles used by codegen; string
//! arrays deep-copy their elements on insert and reclaim them on drop.

use crate::registry;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_new() -> i64 {
    registry::array_new(registry::PTR_SIZE as i64, 0, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_length(arr: i64) -> i64 {
    registry::array_len(arr)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_push(arr: i64, val: i64) -> i64 {
    if registry::array_push(arr, val) {
        arr
    } else {
        crate::ntsc_throw(registry::put_string(
            "arrays.push: invalid array or value".to_string(),
        ))
    }
}

/// `arrays.pop(arr)` — removes and returns the last element; for string
/// arrays the element's ownership transfers to the caller.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_pop(arr: i64) -> i64 {
    registry::array_pop(arr).unwrap_or(0)
}

/// `arrays.at(arr, index)` — negative indices count from the end.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_at(arr: i64, index: i64) -> i64 {
    let len = registry::array_len(arr);
    let idx = if index < 0 { len + index } else { index };
    registry::array_get(arr, idx).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_contains(arr: i64, val: i64) -> i8 {
    let string_elements = registry::with_array(arr, |a| a.string_elements).unwrap_or(false);
    let len = registry::array_len(arr);
    let found = if string_elements {
        (0..len)
            .any(|i| registry::array_get(arr, i).is_some_and(|h| registry::string_equals(h, val)))
    } else {
        (0..len).any(|i| registry::array_get(arr, i) == Some(val))
    };
    i8::from(found)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_index_of(arr: i64, val: i64) -> i64 {
    let string_elements = registry::with_array(arr, |a| a.string_elements).unwrap_or(false);
    let len = registry::array_len(arr);
    if string_elements {
        for i in 0..len {
            if let Some(h) = registry::array_get(arr, i)
                && registry::string_equals(h, val)
            {
                return i;
            }
        }
    } else {
        for i in 0..len {
            if registry::array_get(arr, i) == Some(val) {
                return i;
            }
        }
    }
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_join(arr: i64, delim: i64) -> i64 {
    let delim = registry::get_string(delim).unwrap_or_default();
    let string_elements = registry::with_array(arr, |a| a.string_elements).unwrap_or(false);
    let len = registry::array_len(arr);
    let mut parts = Vec::with_capacity(len as usize);
    for i in 0..len {
        let elem = registry::array_get(arr, i).unwrap_or(0);
        let text = if string_elements {
            registry::get_string(elem).unwrap_or_default()
        } else {
            elem.to_string()
        };
        parts.push(text);
    }
    registry::put_string(parts.join(&delim))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_reverse(arr: i64) -> i64 {
    registry::array_reverse(arr)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_sort(arr: i64) -> i64 {
    registry::array_sort(arr, 2)
}

/// `arrays.remove(arr, val)` — removes the first occurrence; returns the
/// array unchanged (a copy) when `val` is absent.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_remove(arr: i64, val: i64) -> i64 {
    let idx = ntsc_arrays_index_of(arr, val);
    if idx >= 0 {
        registry::array_remove_at(arr, idx)
    } else {
        registry::array_clone(arr, 0)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_remove_at(arr: i64, index: i64) -> i64 {
    registry::array_remove_at(arr, index)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_slice(arr: i64, start: i64, end: i64) -> i64 {
    registry::array_slice(arr, start, end)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_clear(arr: i64) -> i64 {
    registry::array_clear(arr)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_clone(arr: i64) -> i64 {
    registry::array_clone(arr, 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_fill(val: i64, count: i64) -> i64 {
    registry::array_fill(val, count, registry::PTR_SIZE as i64, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_range(start: i64, end: i64) -> i64 {
    registry::array_range(start, end)
}

/// `arrays.every(arr, cond)` — stub: returns 1 iff the array is non-empty;
/// `cond` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_every(arr: i64, _cond: i64) -> i8 {
    i8::from(registry::array_len(arr) > 0)
}

/// `arrays.some(arr, cond)` — stub: returns 1 iff the array is non-empty;
/// `cond` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_some(arr: i64, _cond: i64) -> i8 {
    i8::from(registry::array_len(arr) > 0)
}

/// `arrays.flat(arr)` — flatten one level (clone with the flatten flag;
/// a no-op for one-dimensional arrays).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_flat(arr: i64) -> i64 {
    registry::array_clone(arr, 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_arrays_shuffle(arr: i64) -> i64 {
    registry::array_shuffle(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    #[test]
    fn test_length() {
        let arr = registry::array_new(8, 0, 1);
        for s in ["a", "b", "c"] {
            registry::array_push(arr, put(s));
        }
        assert_eq!(ntsc_arrays_length(arr), 3);
        registry::array_drop(arr);
    }

    #[test]
    fn test_join() {
        let arr = registry::array_new(8, 0, 1);
        for s in ["a", "b", "c"] {
            registry::array_push(arr, put(s));
        }
        let joined = ntsc_arrays_join(arr, put(","));
        assert_eq!(registry::get_string(joined).unwrap(), "a,b,c");
        registry::take_string(joined);
        registry::array_drop(arr);
    }

    #[test]
    fn test_range() {
        let r = ntsc_arrays_range(1, 4);
        assert_eq!(registry::array_len(r), 3);
        assert_eq!(registry::array_get(r, 0), Some(1));
        registry::array_drop(r);
    }
}
