//! NTSC standard library: `hash` module.
//! SHA-256 and CRC-32; MD5 is intentionally omitted.

use crate::registry;

/// `hash.sha256(str)` — the digest as a lowercase hex string.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_hash_sha256(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    registry::put_string(format!("{:x}", result))
}

/// `hash.crc32(str)` — standard reflected CRC-32 (IEEE 802.3, polynomial
/// 0xEDB88320), as an integer.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_hash_crc32(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    crc32(s.as_bytes()) as i64
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
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
    fn test_sha256() {
        let hash = read(ntsc_hash_sha256(put("hello")));
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_crc32() {
        assert_eq!(ntsc_hash_crc32(put("hello")), 0x3610a686);
        assert_eq!(ntsc_hash_crc32(put("")), 0);
        assert_eq!(ntsc_hash_crc32(put("abc")), 0x352441c2);
    }

    #[test]
    fn test_crc32_deterministic_and_hand_computed() {
        assert_eq!(ntsc_hash_crc32(put("123456789")), 0xcbf43926);
    }
}
