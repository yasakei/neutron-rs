//! Unit test fuzzing suite for the NTSC parser.
//!
//! Generates pseudo-random byte sequences and UTF-8 strings to verify
//! that the lexer and parser never panic or crash on arbitrary inputs.

use ntsc_lexer::tokenize;
use ntsc_parser::parse;

#[test]
fn test_parser_fuzz_random_bytes() {
    let mut state: u64 = 0x123456789ABCDEF0;
    let mut rand_u8 = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };

    for _ in 0..1000 {
        let len = (rand_u8() % 128) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rand_u8()).collect();
        if let Ok(source) = std::str::from_utf8(&bytes) {
            let tokens = tokenize(source);
            let _ = parse(&tokens);
        }
    }
}

#[test]
fn test_parser_fuzz_malformed_syntax() {
    let inputs = vec![
        "fun ((((",
        "var int x = (((1 + 2) * ",
        "class A extends extends {",
        "match (x) { case => => =>",
        "if elif else if (((",
        "say(\"${${${",
        "var [a, b, ...] = [1, 2, 3]",
        "try { throw 1 } catch catch finally",
        "do { } while (",
        "enum E { A =, B = }",
    ];

    for input in inputs {
        let tokens = tokenize(input);
        let _ = parse(&tokens);
    }
}
