//! Public-ABI regression tests for runtime exceptions.

use ntsc_runtime::{
    ntsc_array_new, ntsc_array_push, ntsc_exception_clear, ntsc_exception_get_message,
    ntsc_exception_pending, ntsc_exception_take_message, ntsc_string_clone, ntsc_string_drop,
    ntsc_string_equals, ntsc_string_from_words, ntsc_throw,
};

fn string_handle(text: &str) -> i64 {
    let words = ntsc_array_new(8, text.len().div_ceil(8) as i64);
    for chunk in text.as_bytes().chunks(8) {
        let mut bytes = [0_u8; 8];
        bytes[..chunk.len()].copy_from_slice(chunk);
        ntsc_array_push(words, i64::from_ne_bytes(bytes));
    }
    ntsc_string_from_words(words, text.len() as i64)
}

#[test]
fn roundtrip() {
    ntsc_exception_clear();
    let message = string_handle("boom");

    assert_eq!(ntsc_throw(message), 0);
    assert_eq!(ntsc_exception_pending(), 1);

    let borrowed = ntsc_exception_get_message();
    let expected = string_handle("boom");
    assert_eq!(ntsc_string_equals(borrowed, expected), 1);

    let taken = ntsc_exception_take_message();
    assert_eq!(taken, borrowed);
    assert_eq!(ntsc_exception_pending(), 0);
    let clone = ntsc_string_clone(taken);
    assert_ne!(clone, 0);

    ntsc_string_drop(clone);
    ntsc_string_drop(taken);
    ntsc_string_drop(expected);
}

#[test]
fn clear_reclaims_the_pending_message() {
    ntsc_exception_clear();
    ntsc_throw(string_handle("discarded"));
    let borrowed = ntsc_exception_get_message();

    ntsc_exception_clear();

    assert_eq!(ntsc_exception_pending(), 0);
    assert_eq!(ntsc_string_clone(borrowed), 0);
}
