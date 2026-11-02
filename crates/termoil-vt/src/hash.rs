//! A small fast hasher for the page maps, whose keys are cell indexes and styles.
//!
//! The standard library's SipHash defends against hash flooding, which matters for maps keyed by
//! untrusted strings. The page maps are keyed by numbers the terminal chose, so the Fx hash
//! (the one rustc uses) is enough and several times faster.

use core::hash::{BuildHasherDefault, Hasher};

/// The Fx hasher: multiply and rotate per word.
#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    /// Mixes one word into the hash.
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    /// Mixes bytes in eight at a time.
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        for &b in chunks.remainder() {
            self.add(b as u64);
        }
    }

    /// Mixes a u8.
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    /// Mixes a u16.
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64);
    }

    /// Mixes a u32.
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    /// Mixes a u64.
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    /// Mixes a usize.
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    /// The hash value.
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// A `HashMap` using [`FxHasher`].
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
