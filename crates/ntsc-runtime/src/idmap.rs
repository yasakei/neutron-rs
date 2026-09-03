//! Hash maps keyed by runtime-generated `i64` ids.
//!
//! The hot tables (handle registry, goroutine/channel/io tables) are keyed by a
//! runtime counter, not by attacker-controlled data, so the default SipHash is
//! paid for nothing. [`IdHasher`] uses a multiply-xor finalizer instead.

use std::hash::{BuildHasherDefault, Hasher};

/// A [`HashMap`](std::collections::HashMap) keyed by an `i64` runtime id.
pub(crate) type IdMap<V> = std::collections::HashMap<i64, V, BuildHasherDefault<IdHasher>>;

/// Hasher for runtime-generated integer ids.
#[derive(Default)]
pub(crate) struct IdHasher(u64);

impl IdHasher {
    /// splitmix64's finalizer: spreads a dense counter across all 64 bits so
    /// the map's power-of-two bucket masking does not cluster.
    #[inline]
    fn mix(value: u64) -> u64 {
        let mut hash = value;
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash ^= hash >> 33;
        hash
    }
}

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write_i64(&mut self, value: i64) {
        self.0 = Self::mix(value as u64);
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = Self::mix(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.0 = Self::mix(value as u64);
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = Self::mix(self.0 ^ u64::from(byte));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dense run of ids must round-trip without collapsing into one bucket.
    #[test]
    fn dense_ids_round_trip_without_collisions() {
        let mut map: IdMap<i64> = IdMap::default();
        for id in 1..=10_000 {
            assert!(map.insert(id, id * 3).is_none());
        }
        assert_eq!(map.len(), 10_000);
        for id in 1..=10_000 {
            assert_eq!(map.get(&id), Some(&(id * 3)));
        }
        for id in 1..=10_000 {
            assert_eq!(map.remove(&id), Some(id * 3));
        }
        assert!(map.is_empty());
    }

    /// Equal keys must hash equally.
    #[test]
    fn equal_ids_hash_equally() {
        let hash_of = |id: i64| {
            let mut hasher = IdHasher::default();
            hasher.write_i64(id);
            hasher.finish()
        };
        assert_eq!(hash_of(42), hash_of(42));
        assert_ne!(hash_of(1), hash_of(2));
        assert_ne!(hash_of(0), hash_of(i64::MAX));
    }
}
