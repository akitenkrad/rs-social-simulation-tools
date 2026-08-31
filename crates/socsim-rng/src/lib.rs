//! Deterministic, reproducible RNG for `socsim`.
//!
//! Built on [`rand_chacha::ChaCha20Rng`].  Seed a [`SimRng`] from a `u64`
//! root seed, then derive child RNGs per agent / phase / trial using
//! [`SimRng::derive`] or the free function [`derive_seed`].  Explicit IDs are
//! always passed through — never rely on "most recent" state.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};

/// Deterministic RNG wrapper.
///
/// Wraps [`ChaCha20Rng`] and delegates all [`rand::RngCore`] calls to it.
/// Use [`SimRng::from_seed`] to create the root RNG, then [`SimRng::derive`]
/// to produce child RNGs for each agent, phase, or trial without mutating the
/// parent.
///
/// Implements [`Clone`] and serde so the **exact** stream state (seed + word
/// position) can be captured in a snapshot and resumed bit-identically — see
/// [`Simulation::snapshot`](../socsim_engine/struct.Simulation.html#method.snapshot).
#[derive(Clone, Debug, Serialize, Deserialize)]
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
/// Distinct `(root, label)` pairs produce distinct, well-separated seeds: a
/// one-bit change anywhere in the input changes about half the output bits.
///
/// The accumulation is FNV-1a, which alone does **not** have that property —
/// XOR and an odd multiply both carry low bits upward only, so the low `k`
/// bits of the result would depend on nothing but the low `k` bits of the last
/// label, and consecutive labels would come out a fixed small multiple of the
/// prime apart. The finalizer below is what supplies the avalanche; the label
/// count enters the mix so that `[a]` and `[a, b]` cannot meet.
///
/// # Compatibility
///
/// The value returned changed in the commit that added the finalizer. Runs
/// recorded before it cannot be reproduced bit-for-bit afterwards, and a test
/// pinning a number produced by the old function is *supposed* to fail.
pub fn derive_seed(root: u64, parts: &[u64]) -> u64 {
    // FNV-1a 64-bit constants
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut h = FNV_OFFSET ^ root.wrapping_mul(FNV_PRIME);
    for &p in parts {
        h ^= p;
        h = h.wrapping_mul(FNV_PRIME);
    }
    // Without the count, a label list is only mixed through its elements, and
    // one list can reach the state another leaves behind.
    h ^= parts.len() as u64;
    h = h.wrapping_mul(FNV_PRIME);
    fmix64(h)
}

/// The MurmurHash3 finalizer: the step that makes high bits depend on low ones.
#[inline]
fn fmix64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
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

    /// Flipping one input bit must move about half the output bits.
    ///
    /// Without the finalizer this fails on the first bit tried: the mix carries
    /// low bits upward only, so flipping bit 0 of the label moves a handful of
    /// low bits and leaves the rest of the word alone.
    #[test]
    fn one_input_bit_moves_about_half_the_output_bits() {
        let base = derive_seed(42, &[0]);
        for bit in 0..64 {
            let moved = (derive_seed(42, &[1u64 << bit]) ^ base).count_ones();
            assert!(
                (16..=48).contains(&moved),
                "flipping label bit {bit} moved {moved} of 64 output bits"
            );
        }
    }

    /// The low bits must not be a function of the low bits of the input alone.
    ///
    /// `(x ^ i) * PRIME` keeps `seed mod 2^k` determined by `i mod 2^k`, so
    /// consecutive replicate indices used to share their low byte pattern.
    #[test]
    fn the_low_bits_do_not_come_from_the_low_bits_of_the_label() {
        // Labels agreeing on their low byte and differing above it.
        let a = derive_seed(42, &[0x0000_00ff]);
        let b = derive_seed(42, &[0x1234_00ff]);
        assert_ne!(
            a & 0xff,
            b & 0xff,
            "the low byte tracked the label's low byte"
        );
    }

    /// Neighbouring labels must not land a small multiple of the prime apart.
    ///
    /// The unfinalized mix put consecutive replicate indices at gaps of
    /// `±k * FNV_PRIME` for tiny `k`, which is what "no avalanche" looks like
    /// from the caller's side.
    #[test]
    fn consecutive_labels_are_not_a_multiple_of_the_prime_apart() {
        const FNV_PRIME: u64 = 0x100000001b3;
        let seeds: Vec<u64> = (0..8).map(|i| derive_seed(42, &[i])).collect();
        for (i, w) in seeds.windows(2).enumerate() {
            let gap = w[1].wrapping_sub(w[0]);
            for k in 1..=16u64 {
                assert_ne!(
                    gap,
                    FNV_PRIME.wrapping_mul(k),
                    "gap {i} is +{k} * FNV_PRIME"
                );
                assert_ne!(
                    gap,
                    0u64.wrapping_sub(FNV_PRIME.wrapping_mul(k)),
                    "gap {i} is -{k} * FNV_PRIME"
                );
            }
        }
    }

    /// A label list is identified by its contents *and* its length.
    ///
    /// Every step of the accumulation is invertible, so without the count in
    /// the mix a one-element list can land exactly where the empty list left
    /// off. This label is the solution of that equation for root 42 under the
    /// old function: it collided with `[]`.
    #[test]
    fn a_shorter_label_cannot_reach_a_longer_one() {
        assert_ne!(
            derive_seed(42, &[]),
            derive_seed(42, &[0x72d1_44d9_a011_ba62])
        );
        assert_ne!(derive_seed(42, &[1]), derive_seed(42, &[1, 0]));
    }

    #[test]
    fn serde_round_trip_preserves_stream_position() {
        // Advance the stream, snapshot it, then keep drawing.
        let mut rng = SimRng::from_seed(123);
        for _ in 0..5 {
            let _ = rng.next_u64();
        }
        // Restore from a serialised copy and compare the *continuation*.
        let json = serde_json::to_string(&rng).unwrap();
        let mut restored: SimRng = serde_json::from_str(&json).unwrap();
        assert_eq!(rng.next_u64(), restored.next_u64());
        assert_eq!(rng.next_u64(), restored.next_u64());
    }
}
