#![no_main]

use libfuzzer_sys::fuzz_target;
use ntsc_lexer::tokenize;
use ntsc_parser::parse;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let tokens = tokenize(source);
        let _ = parse(&tokens);
    }
});
