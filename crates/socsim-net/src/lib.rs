//! Social network layer for the `socsim` platform.
//!
//! Provides [`SocialNetwork`] — a thin, undirected-graph wrapper around
//! [`petgraph`] whose nodes are keyed by [`AgentId`].  All random generators
//! accept a `&mut SimRng` for full reproducibility.
//!
//! # Included generators
//!
//! | Constructor | Model |
//! |---|---|
//! | [`SocialNetwork::erdos_renyi`] | Erdős–Rényi G(n,p) |
//! | [`SocialNetwork::watts_strogatz`] | Watts–Strogatz small-world |
//! | [`SocialNetwork::barabasi_albert`] | Barabási–Albert preferential attachment |
//! | [`SocialNetwork::empty`] | Start from scratch |

use std::collections::HashMap;

use petgraph::graph::{NodeIndex, UnGraph};
use rand::Rng;
use socsim_core::AgentId;

// Re-export SimRng so callers need only one dep.
pub use socsim_core::SimRng;

// ── SocialNetwork ─────────────────────────────────────────────────────────────

/// An undirected social network whose nodes are [`AgentId`]s.
///
/// Internally stores a [`petgraph::graph::UnGraph`] and an `AgentId →
/// NodeIndex` index for O(1) agent lookups.  All edge weights are `()`.
pub struct SocialNetwork {
    graph: UnGraph<AgentId, ()>,
    index: HashMap<AgentId, NodeIndex>,
}

impl SocialNetwork {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Create an empty network with no nodes or edges.
    pub fn empty() -> Self {
        Self {
            graph: UnGraph::new_undirected(),
            index: HashMap::new(),
        }
    }

    /// **Erdős–Rényi G(n,p)**: add each possible undirected edge independently
    /// with probability `p`.
    ///
    /// `p` is clamped to `[0.0, 1.0]`.
    pub fn erdos_renyi(ids: &[AgentId], p: f64, rng: &mut SimRng) -> Self {
        let p = p.clamp(0.0, 1.0);
        let mut net = Self::empty();
        for &id in ids {
            net.add_node(id);
        }
        let n = ids.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if rng.gen::<f64>() < p {
                    net.add_edge(ids[i], ids[j]);
                }
            }
        }
        net
    }

    /// **Watts–Strogatz** small-world model.
    ///
    /// Starts from a k-regular ring lattice (each node connected to `k/2`
    /// neighbours on each side) and rewires each edge with probability `beta`.
    ///
    /// - `k` is the mean degree; it is rounded down to the nearest even number.
    /// - `beta` is clamped to `[0.0, 1.0]`.
    pub fn watts_strogatz(ids: &[AgentId], k: usize, beta: f64, rng: &mut SimRng) -> Self {
        let beta = beta.clamp(0.0, 1.0);
        let n = ids.len();
        let k = k.min(n.saturating_sub(1));
        let half_k = (k / 2).max(1);

        let mut net = Self::empty();
        for &id in ids {
            net.add_node(id);
        }

        // Build ring lattice: each node connects to half_k neighbours on each side.
        for i in 0..n {
            for d in 1..=half_k {
                let j = (i + d) % n;
                // Avoid duplicate edges (petgraph allows multi-edges; we guard here).
                if net
                    .graph
                    .find_edge(net.index[&ids[i]], net.index[&ids[j]])
                    .is_none()
                {
                    net.add_edge(ids[i], ids[j]);
                }
            }
        }

        // Rewiring pass.
        for i in 0..n {
            for d in 1..=half_k {
                if rng.gen::<f64>() < beta {
                    let j = (i + d) % n;
                    let ni = net.index[&ids[i]];
                    let nj = net.index[&ids[j]];
                    if let Some(e) = net.graph.find_edge(ni, nj) {
                        // Pick a random replacement target (not self, not existing neighbour).
                        let mut tries = 0u32;
                        loop {
                            tries += 1;
                            if tries > 2 * n as u32 + 10 {
                                break; // Give up; keep original edge.
                            }
                            let k_new = rng.gen_range(0..n);
                            if k_new == i {
                                continue;
                            }
                            let nk = net.index[&ids[k_new]];
                            if net.graph.find_edge(ni, nk).is_none() {
                                net.graph.remove_edge(e);
                                net.graph.add_edge(ni, nk, ());
                                break;
                            }
                        }
                    }
                }
            }
        }

        net
    }

    /// **Barabási–Albert** preferential-attachment model.
    ///
    /// Each new node attaches to `m` existing nodes chosen with probability
    /// proportional to current degree (+ 1 to avoid zero-probability isolation
    /// for the seed nodes).
    ///
    /// `m` is clamped to `[1, n-1]`.
    pub fn barabasi_albert(ids: &[AgentId], m: usize, rng: &mut SimRng) -> Self {
        let n = ids.len();
        let m = m.clamp(1, n.saturating_sub(1).max(1));
        let mut net = Self::empty();

        if n == 0 {
            return net;
        }

        // Seed: connect the first min(m+1, n) nodes as a clique.
        let seed_n = (m + 1).min(n);
        for &id in &ids[..seed_n] {
            net.add_node(id);
        }
        for i in 0..seed_n {
            for j in (i + 1)..seed_n {
                net.add_edge(ids[i], ids[j]);
            }
        }

        // Preferential attachment for the remaining nodes.
        for &new_id in &ids[seed_n..n] {
            net.add_node(new_id);
            let ni = net.index[&new_id];

            // Build degree-weighted cumulative distribution over existing nodes
            // (excluding the new node itself).
            let existing: Vec<NodeIndex> =
                net.graph.node_indices().filter(|&idx| idx != ni).collect();

            let weights: Vec<f64> = existing
                .iter()
                .map(|&idx| (net.graph.edges(idx).count() as f64) + 1.0)
                .collect();
            let total: f64 = weights.iter().sum();

            let mut chosen: Vec<NodeIndex> = Vec::with_capacity(m);
            let mut attempts = 0u32;
            while chosen.len() < m.min(existing.len()) {
                attempts += 1;
                if attempts > 10 * m as u32 + 100 {
                    break;
                }
                let r = rng.gen::<f64>() * total;
                let mut cum = 0.0;
                for (idx_pos, &w) in weights.iter().enumerate() {
                    cum += w;
                    if r < cum {
                        let target = existing[idx_pos];
                        if !chosen.contains(&target) && net.graph.find_edge(ni, target).is_none() {
                            chosen.push(target);
                        }
                        break;
                    }
                }
            }

            for target in chosen {
                net.graph.add_edge(ni, target, ());
            }
        }

        net
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Add a node for `id`.  No-op if `id` is already present.
    pub fn add_node(&mut self, id: AgentId) {
        self.index
            .entry(id)
            .or_insert_with(|| self.graph.add_node(id));
    }

    /// Add an undirected edge between `a` and `b`.
    ///
    /// Both nodes must exist (call [`add_node`](Self::add_node) first).
    /// Duplicate edges are silently ignored.
    pub fn add_edge(&mut self, a: AgentId, b: AgentId) {
        if let (Some(&na), Some(&nb)) = (self.index.get(&a), self.index.get(&b)) {
            if self.graph.find_edge(na, nb).is_none() {
                self.graph.add_edge(na, nb, ());
            }
        }
    }

    /// Remove a node and all its incident edges from the network.
    ///
    /// Returns `true` if the node existed, `false` otherwise.  After removal
    /// the internal `petgraph` `NodeIndex` values for other nodes may shift;
    /// the `HashMap<AgentId, NodeIndex>` index is rebuilt to stay consistent.
    pub fn remove_node(&mut self, id: AgentId) -> bool {
        if let Some(&ni) = self.index.get(&id) {
            self.graph.remove_node(ni);
            // NodeIndex values after the removed node shift down by one in
            // StableGraph; however, we use the default (non-stable) graph which
            // swaps the last node into the removed slot.  We must rebuild the
            // full index.
            self.index.clear();
            for idx in self.graph.node_indices() {
                let agent = self.graph[idx];
                self.index.insert(agent, idx);
            }
            true
        } else {
            false
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return all neighbours of `id`.
    ///
    /// Returns an empty `Vec` if `id` is not present in the network.
    pub fn neighbors(&self, id: AgentId) -> Vec<AgentId> {
        match self.index.get(&id) {
            Some(&ni) => self.graph.neighbors(ni).map(|nb| self.graph[nb]).collect(),
            None => Vec::new(),
        }
    }

    /// Return the degree (number of incident edges) of `id`.
    ///
    /// Returns `0` if `id` is not present.
    pub fn degree(&self, id: AgentId) -> usize {
        match self.index.get(&id) {
            Some(&ni) => self.graph.edges(ni).count(),
            None => 0,
        }
    }

    /// Total number of nodes in the network.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of connected components (using union-find via [`petgraph`]).
    pub fn connected_components(&self) -> usize {
        petgraph::algo::connected_components(&self.graph)
    }

    /// Returns `true` if `id` exists in the network.
    pub fn contains(&self, id: AgentId) -> bool {
        self.index.contains_key(&id)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u64) -> Vec<AgentId> {
        (0..n).map(AgentId).collect()
    }

    // ── erdos_renyi ───────────────────────────────────────────────────────────

    #[test]
    fn erdos_renyi_node_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(20);
        let net = SocialNetwork::erdos_renyi(&ids, 0.3, &mut rng);
        assert_eq!(net.node_count(), 20);
    }

    #[test]
    fn erdos_renyi_p0_no_edges() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(10);
        let net = SocialNetwork::erdos_renyi(&ids, 0.0, &mut rng);
        for id in &ids {
            assert_eq!(net.degree(*id), 0);
        }
    }

    #[test]
    fn erdos_renyi_p1_complete_graph() {
        let mut rng = SimRng::from_seed(0);
        let n = 6u64;
        let ids = ids(n);
        let net = SocialNetwork::erdos_renyi(&ids, 1.0, &mut rng);
        // In a complete graph each node has degree n-1.
        for id in &ids {
            assert_eq!(net.degree(*id), (n - 1) as usize);
        }
    }

    #[test]
    fn erdos_renyi_deterministic() {
        let ids = ids(15);
        let net1 = SocialNetwork::erdos_renyi(&ids, 0.4, &mut SimRng::from_seed(42));
        let net2 = SocialNetwork::erdos_renyi(&ids, 0.4, &mut SimRng::from_seed(42));
        // Compare degree sequences as a proxy for identical graphs.
        let deg1: Vec<usize> = ids.iter().map(|&id| net1.degree(id)).collect();
        let deg2: Vec<usize> = ids.iter().map(|&id| net2.degree(id)).collect();
        assert_eq!(deg1, deg2);
    }

    // ── watts_strogatz ────────────────────────────────────────────────────────

    #[test]
    fn watts_strogatz_node_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(20);
        let net = SocialNetwork::watts_strogatz(&ids, 4, 0.1, &mut rng);
        assert_eq!(net.node_count(), 20);
    }

    #[test]
    fn watts_strogatz_deterministic() {
        let ids = ids(16);
        let net1 = SocialNetwork::watts_strogatz(&ids, 4, 0.2, &mut SimRng::from_seed(7));
        let net2 = SocialNetwork::watts_strogatz(&ids, 4, 0.2, &mut SimRng::from_seed(7));
        let deg1: Vec<usize> = ids.iter().map(|&id| net1.degree(id)).collect();
        let deg2: Vec<usize> = ids.iter().map(|&id| net2.degree(id)).collect();
        assert_eq!(deg1, deg2);
    }

    #[test]
    fn watts_strogatz_beta0_regular() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(10);
        let k = 4usize;
        let net = SocialNetwork::watts_strogatz(&ids, k, 0.0, &mut rng);
        // With beta=0 no rewiring: all degrees should be k (= 2 * half_k).
        for id in &ids {
            assert_eq!(net.degree(*id), k);
        }
    }

    // ── barabasi_albert ───────────────────────────────────────────────────────

    #[test]
    fn barabasi_albert_node_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(20);
        let net = SocialNetwork::barabasi_albert(&ids, 2, &mut rng);
        assert_eq!(net.node_count(), 20);
    }

    #[test]
    fn barabasi_albert_deterministic() {
        let ids = ids(20);
        let net1 = SocialNetwork::barabasi_albert(&ids, 2, &mut SimRng::from_seed(13));
        let net2 = SocialNetwork::barabasi_albert(&ids, 2, &mut SimRng::from_seed(13));
        let deg1: Vec<usize> = ids.iter().map(|&id| net1.degree(id)).collect();
        let deg2: Vec<usize> = ids.iter().map(|&id| net2.degree(id)).collect();
        assert_eq!(deg1, deg2);
    }

    // ── neighbors symmetric ───────────────────────────────────────────────────

    #[test]
    fn neighbors_are_symmetric() {
        let mut rng = SimRng::from_seed(3);
        let ids = ids(10);
        let net = SocialNetwork::erdos_renyi(&ids, 0.5, &mut rng);
        for &a in &ids {
            for b in net.neighbors(a) {
                assert!(
                    net.neighbors(b).contains(&a),
                    "expected {b:?} to list {a:?} as neighbour"
                );
            }
        }
    }

    // ── remove_node ───────────────────────────────────────────────────────────

    #[test]
    fn remove_node_decrements_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(5);
        let mut net = SocialNetwork::erdos_renyi(&ids, 1.0, &mut rng);
        assert_eq!(net.node_count(), 5);
        assert!(net.remove_node(AgentId(2)));
        assert_eq!(net.node_count(), 4);
        assert!(!net.contains(AgentId(2)));
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut net = SocialNetwork::empty();
        assert!(!net.remove_node(AgentId(99)));
    }

    #[test]
    fn remove_node_clears_from_neighbors() {
        let mut net = SocialNetwork::empty();
        let a = AgentId(0);
        let b = AgentId(1);
        net.add_node(a);
        net.add_node(b);
        net.add_edge(a, b);
        assert_eq!(net.neighbors(a), vec![b]);
        net.remove_node(b);
        assert!(net.neighbors(a).is_empty());
    }

    // ── connected_components ─────────────────────────────────────────────────

    #[test]
    fn connected_components_empty() {
        let net = SocialNetwork::empty();
        assert_eq!(net.connected_components(), 0);
    }

    #[test]
    fn connected_components_isolated_nodes() {
        let mut net = SocialNetwork::empty();
        net.add_node(AgentId(0));
        net.add_node(AgentId(1));
        assert_eq!(net.connected_components(), 2);
    }

    #[test]
    fn connected_components_single_component() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(5);
        let net = SocialNetwork::erdos_renyi(&ids, 1.0, &mut rng);
        assert_eq!(net.connected_components(), 1);
    }
}
