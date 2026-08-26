//! NTSC standard library: `crypto` module.
//! String arguments are borrowed handles; returned handles are owned by the
//! caller.

use crate::registry;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_base64_encode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(s);
    registry::put_string(encoded)
}

/// `crypto.base64_decode(str)` — throws when the input is not valid base64
/// or does not decode to UTF-8.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_base64_decode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use base64::Engine;
    match base64::engine::general_purpose::STANDARD.decode(s) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(decoded) => registry::put_string(decoded),
            Err(_) => super::throw_str(
                "crypto.base64_decode: decoded data is not valid UTF-8".to_string(),
            ),
        },
        Err(e) => super::throw_str(format!("crypto.base64_decode: {e}")),
    }
}

/// `crypto.sha256(str)` — the digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_sha256(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    registry::put_string(format!("{:x}", result))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_hex_encode(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    let hex: String = s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
    registry::put_string(hex)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_hex_decode(hex: i64) -> i64 {
    let hex = registry::get_string(hex).unwrap_or_default();
    if !hex.len().is_multiple_of(2) {
        return super::throw_str("crypto.hex_decode: hex string must have even length".to_string());
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
                    "crypto.hex_decode: invalid hex byte '{byte_str}'"
                ));
            }
        }
    }
    match String::from_utf8(bytes) {
        Ok(decoded) => registry::put_string(decoded),
        Err(_) => {
            super::throw_str("crypto.hex_decode: decoded data is not valid UTF-8".to_string())
        }
    }
}

/// `crypto.random_bytes(count)` — hex-encoded bytes from the OS random
/// source, with a time-seeded PRNG fallback (not cryptographically secure).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_random_bytes(count: i64) -> i64 {
    let count = count.clamp(1, 1024 * 1024) as usize;
    use std::fs::File;
    use std::io::Read;
    let mut bytes = vec![0u8; count];
    if let Ok(mut f) = File::open("/dev/urandom") {
        let _ = f.read_exact(&mut bytes);
    } else {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut state = seed;
        for byte in bytes.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (state >> 33) as u8;
        }
    }
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    registry::put_string(hex)
}

/// `crypto.random_string(length, charset)` — `charset` defaults to
/// alphanumeric.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_random_string(length: i64, charset: i64) -> i64 {
    let length = length.clamp(1, 1024) as usize;
    let charset = registry::get_string(charset).unwrap_or_default();
    let chars = if charset.is_empty() {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
    } else {
        charset.as_str()
    };
    if chars.is_empty() {
        return registry::put_string(String::new());
    }
    use std::fs::File;
    use std::io::Read;
    let mut random_bytes = vec![0u8; length];
    if let Ok(mut f) = File::open("/dev/urandom") {
        let _ = f.read_exact(&mut random_bytes);
    } else {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut state = seed;
        for byte in random_bytes.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (state >> 33) as u8;
        }
    }
    let result: String = random_bytes
        .iter()
        .map(|&b| chars.as_bytes()[b as usize % chars.len()] as char)
        .collect();
    registry::put_string(result)
}

/// `crypto.xor_cipher(data, key)` — toy cipher for educational use only;
/// applying it twice with the same key recovers the input.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_xor_cipher(data: i64, key: i64) -> i64 {
    let data = registry::get_string(data).unwrap_or_default();
    let key = registry::get_string(key).unwrap_or_default();
    if key.is_empty() {
        return registry::put_string(data.to_string());
    }
    let result: String = data
        .bytes()
        .enumerate()
        .map(|(i, b)| (b ^ key.as_bytes()[i % key.len()]) as char)
        .collect();
    registry::put_string(result)
}

/// `crypto.sha512(str)` — the SHA-512 digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_sha512(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    registry::put_string(format!("{:x}", result))
}

/// `crypto.sha384(str)` — the SHA-384 digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_sha384(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use sha2::{Digest, Sha384};
    let mut hasher = Sha384::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    registry::put_string(format!("{:x}", result))
}

/// `crypto.sha224(str)` — the SHA-224 digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_sha224(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use sha2::{Digest, Sha224};
    let mut hasher = Sha224::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    registry::put_string(format!("{:x}", result))
}

/// `crypto.md5(str)` — the MD5 digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_md5(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use md5;
    let result = md5::compute(s.as_bytes());
    registry::put_string(format!("{:x}", result))
}

/// `crypto.hmac_sha256(key, message)` — HMAC-SHA256 as a lowercase hex
/// string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_hmac_sha256(key: i64, message: i64) -> i64 {
    let key = registry::get_string(key).unwrap_or_default();
    let message = registry::get_string(message).unwrap_or_default();
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mac =
        HmacSha256::new_from_slice(key.as_bytes()).map_err(|e| format!("HMAC key error: {e}"));
    match mac {
        Ok(mut m) => {
            m.update(message.as_bytes());
            let result = m.finalize().into_bytes();
            registry::put_string(format!("{:x}", result))
        }
        Err(e) => super::throw_str(format!("crypto.hmac_sha256: {e}")),
    }
}

/// `crypto.hmac_sha512(key, message)` — HMAC-SHA512 as a lowercase hex
/// string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_hmac_sha512(key: i64, message: i64) -> i64 {
    let key = registry::get_string(key).unwrap_or_default();
    let message = registry::get_string(message).unwrap_or_default();
    use hmac::{Hmac, Mac};
    use sha2::Sha512;
    type HmacSha512 = Hmac<Sha512>;
    let mac =
        HmacSha512::new_from_slice(key.as_bytes()).map_err(|e| format!("HMAC key error: {e}"));
    match mac {
        Ok(mut m) => {
            m.update(message.as_bytes());
            let result = m.finalize().into_bytes();
            registry::put_string(format!("{:x}", result))
        }
        Err(e) => super::throw_str(format!("crypto.hmac_sha512: {e}")),
    }
}

/// `crypto.verify_sha256(data, expected_hash)` — returns 1 if the SHA-256
/// hash of `data` matches `expected_hash` (case-insensitive).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_verify_sha256(data: i64, expected_hash: i64) -> i8 {
    let data = registry::get_string(data).unwrap_or_default();
    let expected = registry::get_string(expected_hash).unwrap_or_default();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(&expected) {
        1
    } else {
        0
    }
}

/// `crypto.verify_sha512(data, expected_hash)` — returns 1 if the SHA-512
/// hash of `data` matches `expected_hash` (case-insensitive).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_verify_sha512(data: i64, expected_hash: i64) -> i8 {
    let data = registry::get_string(data).unwrap_or_default();
    let expected = registry::get_string(expected_hash).unwrap_or_default();
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(data.as_bytes());
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(&expected) {
        1
    } else {
        0
    }
}

/// `crypto.file_sha256(path)` — SHA-256 of a file's contents as hex.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_file_sha256(path: i64) -> i64 {
    let path = registry::get_string(path).unwrap_or_default();
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            return super::throw_str(format!("crypto.file_sha256: cannot read '{path}': {e}"));
        }
    };
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&data);
    registry::put_string(format!("{:x}", hasher.finalize()))
}

/// `crypto.file_sha512(path)` — SHA-512 of a file's contents as hex.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_crypto_file_sha512(path: i64) -> i64 {
    let path = registry::get_string(path).unwrap_or_default();
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            return super::throw_str(format!("crypto.file_sha512: cannot read '{path}': {e}"));
        }
    };
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(&data);
    registry::put_string(format!("{:x}", hasher.finalize()))
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
        let encoded = ntsc_crypto_base64_encode(put(original));
        let decoded = ntsc_crypto_base64_decode(encoded);
        assert_eq!(read(decoded), original);
        let _ = registry::take_string(encoded);
    }

    #[test]
    fn test_sha256() {
        let hash = read(ntsc_crypto_sha256(put("hello")));
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = "test123";
        let hex = read(ntsc_crypto_hex_encode(put(original)));
        assert_eq!(hex, "74657374313233");
        let decoded = ntsc_crypto_hex_decode(put(&hex));
        assert_eq!(read(decoded), original);
    }

    #[test]
    fn test_xor() {
        let data = "hello";
        let key = "x";
        let enc = ntsc_crypto_xor_cipher(put(data), put(key));
        let enc_text = read(enc);
        let dec = ntsc_crypto_xor_cipher(put(&enc_text), put(key));
        assert_eq!(read(dec), data);
    }

    #[test]
    fn test_decode_errors_throw() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_crypto_base64_decode(put("!!!not-base64!!!"));
        });
        let msg = err.unwrap();
        assert!(
            msg.contains("crypto.base64_decode"),
            "unexpected message: {msg}"
        );

        let err = catch_throw(|| {
            let _ = ntsc_crypto_hex_decode(put("abc"));
        });
        let msg = err.unwrap();
        assert!(
            msg.contains("crypto.hex_decode"),
            "unexpected message: {msg}"
        );

        let err = catch_throw(|| {
            let _ = ntsc_crypto_hex_decode(put("zz"));
        });
        let msg = err.unwrap();
        assert!(
            msg.contains("crypto.hex_decode"),
            "unexpected message: {msg}"
        );
    }
}
