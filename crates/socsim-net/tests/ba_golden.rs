//! Golden outputs of the Barabási–Albert generators, recorded from the
//! original linear-scan implementation. The Fenwick-tree implementation must
//! add the same edges in the same order and leave the RNG in the same state.
//!
//! Each row of `data/ba_golden.txt` is `kind n m seed dup edges hash`, where
//! `hash` is FNV-1a over the edge list followed by the next `u64` drawn from
//! the RNG after construction.

use rand::Rng;
use socsim_core::AgentId;
use socsim_net::{DiSocialNetwork, SimRng, SocialNetwork};

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn hash_byte(hash: &mut u64, byte: u8) {
    *hash ^= u64::from(byte);
    *hash = hash.wrapping_mul(FNV_PRIME);
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        hash_byte(hash, byte);
    }
}

#[test]
fn barabasi_albert_matches_golden_outputs() {
    for (line_number, line) in include_str!("data/ba_golden.txt").lines().enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(
            fields.len(),
            7,
            "invalid golden row at line {}",
            line_number + 1
        );

        let kind = fields[0];
        let n: usize = fields[1].parse().unwrap();
        let m: usize = fields[2].parse().unwrap();
        let seed: u64 = fields[3].parse().unwrap();
        let duplicate_ids: bool = fields[4].parse().unwrap();
        let expected_edges: usize = fields[5].parse().unwrap();
        let expected_hash = u64::from_str_radix(fields[6], 16).unwrap();

        let mut ids = (0..n as u64).map(AgentId).collect::<Vec<_>>();
        if duplicate_ids && n >= 4 {
            ids.insert(2, AgentId(1));
            ids.insert(n / 2 + 1, AgentId(3));
        }

        let mut rng = SimRng::from_seed(seed);
        let (actual_edges, actual_hash) = match kind {
            "undirected" => {
                let net = SocialNetwork::barabasi_albert(&ids, m, &mut rng);
                let mut hash = FNV_OFFSET_BASIS;
                for (a, b) in net.edges() {
                    hash_bytes(&mut hash, &a.0.to_le_bytes());
                    hash_bytes(&mut hash, &b.0.to_le_bytes());
                }
                hash_bytes(&mut hash, &rng.gen::<u64>().to_le_bytes());
                (net.edges().count(), hash)
            }
            "directed" => {
                let net = DiSocialNetwork::barabasi_albert_directed(&ids, m, &mut rng);
                let mut hash = FNV_OFFSET_BASIS;
                for (a, b) in net.edges() {
                    hash_bytes(&mut hash, &a.0.to_le_bytes());
                    hash_bytes(&mut hash, &b.0.to_le_bytes());
                }
                hash_bytes(&mut hash, &rng.gen::<u64>().to_le_bytes());
                (net.edges().count(), hash)
            }
            _ => panic!("unknown network kind {kind:?} at line {}", line_number + 1),
        };

        let case = format!(
            "line {}: {kind} n={n} m={m} seed={seed} dup={duplicate_ids}",
            line_number + 1
        );
        assert_eq!(
            actual_edges, expected_edges,
            "edge count mismatch for {case}"
        );
        assert_eq!(
            actual_hash, expected_hash,
            "hash mismatch for {case}: got {actual_hash:016x}"
        );
    }
}
