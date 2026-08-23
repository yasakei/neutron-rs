//! Bounds-checked memory capabilities exposed as the `memory` module.

use crate::registry;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_alloc(size: i64) -> i64 {
    if !(0..=16_777_216).contains(&size) {
        return super::throw_str("memory.alloc: size must be between 0 and 16777216".into());
    }
    registry::memory_alloc(size as usize)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_offset(pointer: i64, delta: i64) -> i64 {
    registry::memory_offset(pointer, delta).unwrap_or_else(|| {
        super::throw_str("memory.offset: out of bounds or invalid pointer".into())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_clone(pointer: i64) -> i64 {
    registry::memory_clone(pointer)
        .unwrap_or_else(|| super::throw_str("memory.clone: invalid pointer".into()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_drop(pointer: i64) {
    registry::memory_drop(pointer);
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_load8(pointer: i64) -> i64 {
    registry::memory_load(pointer, 1)
        .map(|value| value & 0xff)
        .unwrap_or_else(|| {
            super::throw_str("memory.load8: out of bounds or invalid pointer".into())
        })
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_load64(pointer: i64) -> i64 {
    registry::memory_load(pointer, 8).unwrap_or_else(|| {
        super::throw_str("memory.load64: out of bounds or invalid pointer".into())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_store8(pointer: i64, value: i64) -> i8 {
    if !(0..=255).contains(&value) {
        return super::throw_str("memory.store8: value must be between 0 and 255".into()) as i8;
    }
    if registry::memory_store(pointer, 1, value) {
        1
    } else {
        super::throw_str("memory.store8: out of bounds or invalid pointer".into()) as i8
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_memory_store64(pointer: i64, value: i64) -> i8 {
    if registry::memory_store(pointer, 8, value) {
        1
    } else {
        super::throw_str("memory.store64: out of bounds or invalid pointer".into()) as i8
    }
}
