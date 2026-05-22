//! Deterministic, reproducible RNG for `socsim`.
//!
//! Built on [`rand_chacha::ChaCha20Rng`].  Seed a [`SimRng`] from a `u64`
//! root seed, then derive child RNGs per agent / phase / trial using
//! [`SimRng::derive`] or the free function [`derive_seed`].  Explicit IDs are
//! always passed through — never rely on "most recent" state.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Deterministic RNG wrapper.
///
/// Wraps [`ChaCha20Rng`] and delegates all [`rand::RngCore`] calls to it.
/// Use [`SimRng::from_seed`] to create the root RNG, then [`SimRng::derive`]
/// to produce child RNGs for each agent, phase, or trial without mutating the
/// parent.
pub struct SimRng {
    inner: ChaCha20Rng,
}

impl SimRng {
    /// Create a root [`SimRng`] from a 64-bit seed.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            inner: ChaCha20Rng::seed_from_u64(seed),
        }
    }

    /// Derive a child [`SimRng`] for a given label (e.g. `[trial_id, agent_id,
    /// phase_index]`) **without mutating** `self`.
    ///
    /// The parent's current seed is used as the root for the derivation, so
    /// child RNGs are fully determined by the label and the root seed.
    pub fn derive(&self, label: &[u64]) -> SimRng {
        // Extract the current stream word-position as part of the root.
        let root = self.inner.get_word_pos() as u64;
        let seed = derive_seed(root, label);
        SimRng::from_seed(seed)
    }
}

/// Deterministically mix a root seed with a slice of `u64` labels.
///
/// Uses a simple [FNV-1a](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function)-like
/// mix so that every distinct `(root, label)` pair produces a distinct seed
/// with good avalanche properties.
pub fn derive_seed(root: u64, parts: &[u64]) -> u64 {
    // FNV-1a 64-bit constants
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut h = FNV_OFFSET ^ root.wrapping_mul(FNV_PRIME);
    for &p in parts {
        h ^= p;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ── rand trait impls ────────────────────────────────────────────────────────

impl rand::RngCore for SimRng {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    #[inline]
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.inner.fill_bytes(dest)
    }

    #[inline]
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.inner.try_fill_bytes(dest)
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    #[test]
    fn same_seed_gives_same_output() {
        let mut a = SimRng::from_seed(42);
        let mut b = SimRng::from_seed(42);
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn different_seeds_give_different_output() {
        let mut a = SimRng::from_seed(1);
        let mut b = SimRng::from_seed(2);
        // With overwhelming probability these differ.
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn derive_is_deterministic() {
        let rng = SimRng::from_seed(0);
        let s1 = rng.derive(&[1, 2, 3]).next_u64();
        // Re-create from same root seed to verify.
        let rng2 = SimRng::from_seed(0);
        let s2 = rng2.derive(&[1, 2, 3]).next_u64();
        assert_eq!(s1, s2);
    }

    #[test]
    fn derive_with_different_label_differs() {
        let rng = SimRng::from_seed(0);
        let s1 = rng.derive(&[1]).next_u64();
        let rng2 = SimRng::from_seed(0);
        let s2 = rng2.derive(&[2]).next_u64();
        assert_ne!(s1, s2);
    }

    #[test]
    fn derive_seed_fn_is_deterministic() {
        assert_eq!(derive_seed(42, &[1, 2]), derive_seed(42, &[1, 2]));
    }
}
