//! Public-ABI regression tests for memory-safety fixes.

use ntsc_runtime::modules::{arrays, crypto, encoding, strings};
use ntsc_runtime::{
    ntsc_array_drop, ntsc_array_get, ntsc_array_len, ntsc_array_new, ntsc_array_new_typed,
    ntsc_array_pop, ntsc_array_push, ntsc_exception_clear, ntsc_exception_get_message,
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

fn assert_string_eq(actual: i64, expected: &str) {
    let expected = string_handle(expected);
    assert_eq!(ntsc_string_equals(actual, expected), 1);
    ntsc_string_drop(expected);
}

fn take_exception() -> i64 {
    assert_eq!(ntsc_exception_pending(), 1);
    let message = ntsc_exception_take_message();
    assert_ne!(message, 0);
    assert_eq!(ntsc_exception_pending(), 0);
    message
}

#[test]
fn array_get_out_of_bounds_throws() {
    ntsc_exception_clear();
    let array = ntsc_array_new(8, 2);
    ntsc_array_push(array, 10);
    ntsc_array_push(array, 20);

    assert_eq!(ntsc_array_get(array, 0), 10);
    assert_eq!(ntsc_array_get(array, 1), 20);

    assert_eq!(ntsc_array_get(array, -1), 0);
    let message = take_exception();
    assert_string_eq(message, "array index out of bounds");
    ntsc_string_drop(message);

    assert_eq!(ntsc_array_get(array, 5), 0);
    let message = take_exception();
    assert_string_eq(message, "array index out of bounds");
    ntsc_string_drop(message);
    ntsc_array_drop(array);
}

#[test]
fn empty_string_array_element_is_a_valid_owned_handle() {
    let array = ntsc_array_new_typed(8, 1, 1);
    let empty = string_handle("");
    assert_ne!(empty, 0);
    assert_eq!(ntsc_array_push(array, empty), array);

    let popped = ntsc_array_pop(array);
    assert_ne!(popped, 0);
    assert_string_eq(popped, "");
    assert_eq!(ntsc_array_len(array), 0);

    ntsc_string_drop(popped);
    ntsc_string_drop(empty);
    ntsc_array_drop(array);
}

#[test]
fn substring_and_char_at_utf8_boundaries_do_not_panic() {
    let text = string_handle("abçd");
    let substring = strings::ntsc_strings_substring(text, 3, 4);
    assert_string_eq(substring, "ç");
    ntsc_string_drop(substring);

    let reversed = strings::ntsc_strings_substring(text, 4, 3);
    assert_string_eq(reversed, "");
    ntsc_string_drop(reversed);

    let character = strings::ntsc_strings_char_at(text, 3);
    assert_string_eq(character, "ç");
    ntsc_string_drop(character);
    ntsc_string_drop(text);
}

#[test]
fn non_ascii_hex_throws_instead_of_panicking() {
    ntsc_exception_clear();
    let input = string_handle("aÄb");
    assert_eq!(encoding::ntsc_encoding_hex_decode(input), 0);
    let message = take_exception();
    let clone = ntsc_string_clone(message);
    assert_ne!(clone, 0);
    ntsc_string_drop(clone);
    ntsc_string_drop(message);

    assert_eq!(crypto::ntsc_crypto_hex_decode(input), 0);
    let message = take_exception();
    let clone = ntsc_string_clone(message);
    assert_ne!(clone, 0);
    ntsc_string_drop(clone);
    ntsc_string_drop(message);
    ntsc_string_drop(input);
}

#[test]
fn valid_hex_decode_roundtrips() {
    let input = string_handle("68656c6c6f");
    let decoded = encoding::ntsc_encoding_hex_decode(input);
    assert_eq!(ntsc_exception_pending(), 0);
    assert_string_eq(decoded, "hello");

    ntsc_string_drop(decoded);
    ntsc_string_drop(input);
}

#[test]
fn taken_exception_message_remains_owned_after_pending_state_is_cleared() {
    ntsc_exception_clear();
    ntsc_throw(string_handle("boom"));
    let borrowed = ntsc_exception_get_message();
    let taken = ntsc_exception_take_message();

    assert_eq!(borrowed, taken);
    assert_eq!(ntsc_exception_pending(), 0);
    assert_string_eq(taken, "boom");
    let clone = ntsc_string_clone(taken);
    assert_ne!(clone, 0);

    ntsc_string_drop(clone);
    ntsc_string_drop(taken);
}

#[test]
fn reversed_array_slice_is_empty_and_does_not_mutate_source() {
    let source = arrays::ntsc_arrays_range(1, 4);
    let sliced = arrays::ntsc_arrays_slice(source, 3, 1);

    assert_eq!(ntsc_array_len(sliced), 0);
    assert_eq!(ntsc_array_len(source), 3);
    assert_eq!(ntsc_array_get(source, 0), 1);

    ntsc_array_drop(sliced);
    ntsc_array_drop(source);
}
