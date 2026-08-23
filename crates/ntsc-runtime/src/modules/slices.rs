//! Bounds-checked windows over owned arrays, exposed as the `slices` module.
//!
//! A slice never holds a pointer: it stores the source array handle plus a
//! window, so every access re-validates both that the array is still
//! registered and that the index lies inside the window. Subslicing narrows
//! the window, which makes it impossible to widen a slice or reach another
//! allocation from one.

use crate::registry;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_of(array: i64, start: i64, end: i64) -> i64 {
    registry::slice_of(array, start, end)
        .unwrap_or_else(|| super::throw_str("slices.of: range is out of bounds".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_sub(slice: i64, start: i64, end: i64) -> i64 {
    registry::slice_sub(slice, start, end)
        .unwrap_or_else(|| super::throw_str("slices.sub: range is out of bounds".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_length(slice: i64) -> i64 {
    registry::slice_len(slice)
        .unwrap_or_else(|| super::throw_str("slices.length: not a slice".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_get(slice: i64, index: i64) -> i64 {
    registry::slice_get(slice, index)
        .unwrap_or_else(|| super::throw_str("slices.get: index out of bounds".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_set(slice: i64, index: i64, value: i64) -> i8 {
    if registry::slice_set(slice, index, value) {
        1
    } else {
        super::throw_str("slices.set: index out of bounds".into()) as i8
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_to_array(slice: i64) -> i64 {
    registry::slice_to_array(slice)
        .unwrap_or_else(|| super::throw_str("slices.to_array: not a slice".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_fill(slice: i64, value: i64) -> i8 {
    if registry::slice_fill(slice, value) {
        1
    } else {
        super::throw_str("slices.fill: not a slice".into()) as i8
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_copy_from(dst: i64, src: i64) -> i8 {
    if registry::slice_copy_from(dst, src) {
        1
    } else {
        super::throw_str("slices.copy_from: lengths differ or not a slice".into()) as i8
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_equal(a: i64, b: i64) -> i8 {
    match registry::slice_equal(a, b) {
        Some(true) => 1,
        Some(false) => 0,
        None => super::throw_str("slices.equal: not a slice".into()) as i8,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_slices_drop(slice: i64) {
    registry::slice_drop(slice);
}
