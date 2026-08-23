//! NTSC standard library: `encoding` module.
//! String arguments are borrowed handles; returned handles are owned by the
//! caller.

use crate::registry;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_encoding_base64_encode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(s);
    registry::put_string(encoded)
}

/// `encoding.base64_decode(str)` — throws when the input is not valid
/// base64 or does not decode to UTF-8.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_encoding_base64_decode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use base64::Engine;
    match base64::engine::general_purpose::STANDARD.decode(s) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(decoded) => registry::put_string(decoded),
            Err(_) => super::throw_str(
                "encoding.base64_decode: decoded data is not valid UTF-8".to_string(),
            ),
        },
        Err(e) => super::throw_str(format!("encoding.base64_decode: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_encoding_hex_encode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    let hex: String = s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    registry::put_string(hex)
}

/// `encoding.hex_decode(hex)` — throws when the input is not valid hex or
/// does not decode to UTF-8.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_encoding_hex_decode(hex: i64) -> i64 {
    let hex = registry::get_string(hex).unwrap_or_default();
    if !hex.len().is_multiple_of(2) {
        return super::throw_str(
            "encoding.hex_decode: hex string must have even length".to_string(),
        );
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    // Slice the byte buffer, never the &str, so hex whose bytes straddle a
    // UTF-8 boundary cannot panic.
    let hex_bytes = hex.as_bytes();
    for i in (0..hex.len()).step_by(2) {
        let byte_str = std::str::from_utf8(&hex_bytes[i..i + 2]).unwrap_or("");
        match u8::from_str_radix(byte_str, 16) {
            Ok(byte) => bytes.push(byte),
            Err(_) => {
                return super::throw_str(format!(
                    "encoding.hex_decode: invalid hex byte '{byte_str}'"
                ));
            }
        }
    }
    match String::from_utf8(bytes) {
        Ok(decoded) => registry::put_string(decoded),
        Err(_) => {
            super::throw_str("encoding.hex_decode: decoded data is not valid UTF-8".to_string())
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_encoding_utf8_valid(s: i64) -> i8 {
    let s = registry::get_string(s).unwrap_or_default();
    i8::from(std::str::from_utf8(s.as_bytes()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = "Hello, World!";
        let encoded = read(ntsc_encoding_base64_encode(put(original)));
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");
        let decoded = read(ntsc_encoding_base64_decode(put(&encoded)));
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = "test123";
        let hex = read(ntsc_encoding_hex_encode(put(original)));
        assert_eq!(hex, "74657374313233");
        let decoded = read(ntsc_encoding_hex_decode(put(&hex)));
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_decode_errors_throw() {
        use crate::modules::test_util::catch_throw;
        let msg = catch_throw(|| {
            let _ = ntsc_encoding_base64_decode(put("!!!not-base64!!!"));
        })
        .unwrap();
        assert!(
            msg.contains("encoding.base64_decode"),
            "unexpected message: {msg}"
        );

        let msg = catch_throw(|| {
            let _ = ntsc_encoding_hex_decode(put("abc"));
        })
        .unwrap();
        assert!(
            msg.contains("encoding.hex_decode"),
            "unexpected message: {msg}"
        );

        let msg = catch_throw(|| {
            let _ = ntsc_encoding_hex_decode(put("zz"));
        })
        .unwrap();
        assert!(
            msg.contains("encoding.hex_decode"),
            "unexpected message: {msg}"
        );

        let msg = catch_throw(|| {
            let _ = ntsc_encoding_hex_decode(put("aÄb"));
        })
        .unwrap();
        assert!(
            msg.contains("encoding.hex_decode"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn test_utf8_valid() {
        assert_eq!(ntsc_encoding_utf8_valid(put("hello")), 1);

        assert_eq!(ntsc_encoding_utf8_valid(0), 1);
    }
}
