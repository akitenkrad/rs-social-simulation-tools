//! Social network layer for the `socsim` platform.
//!
//! Provides [`Network`] — a thin wrapper around [`petgraph`] whose nodes are
//! keyed by [`AgentId`] and whose backing store is a
//! [`petgraph::stable_graph::StableGraph`], so node indices stay valid across
//! removals.  The wrapper is generic over the edge payload `E` (default `()`)
//! and the directedness `Ty` (default [`Undirected`]), with two ready-made
//! aliases:
//!
//! - [`SocialNetwork`] = `Network<(), Undirected>` — the original undirected,
//!   unweighted network.  Every method and the `{ nodes, edges }` serde format
//!   are preserved bit-for-bit, so existing callers are unaffected.
//! - [`DiSocialNetwork`] = `Network<(), Directed>` — a directed variant with
//!   `out_neighbors` / `in_neighbors`.
//!
//! Edge payloads carry weights or labels: `add_edge_weighted(a, b, w)` plus
//! [`Network::edge_weight`] / [`Network::edge_weight_mut`].
//!
//! All random generators accept a `&mut SimRng` for full reproducibility.
//!
//! # Included generators
//!
//! | Constructor | Model |
//! |---|---|
//! | [`Network::erdos_renyi`] | Erdős–Rényi G(n,p) |
//! | [`Network::watts_strogatz`] | Watts–Strogatz small-world |
//! | [`Network::barabasi_albert`] | Barabási–Albert preferential attachment |
//! | [`Network::empty`] | Start from scratch |

use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::{Directed, Direction, Undirected};
use rand::Rng;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

// Re-export SimRng so callers need only one dep.
pub use socsim_core::SimRng;
// Re-export the directedness markers so callers can name the type parameter.
pub use petgraph::{Directed as DirectedTy, Undirected as UndirectedTy};

// ── Network ─────────────────────────────────────────────────────────────────

/// A social network whose nodes are [`AgentId`]s.
///
/// Generic over:
/// - `E`: the per-edge payload (weight / label).  Defaults to `()` for the
///   classic unweighted network.
/// - `Ty`: the directedness, a [`petgraph::EdgeType`] marker — either
///   [`Undirected`] (default) or [`Directed`].  This is a zero-sized
///   type-level switch (no runtime branching), so an undirected and a directed
///   network are distinct types and cannot be mixed by accident.
///
/// Internally stores a [`petgraph::stable_graph::StableGraph`] and an `AgentId
/// → NodeIndex` index for O(1) agent lookups.  Because the backing store is a
/// `StableGraph`, removing a node does **not** invalidate the indices of other
/// nodes, so [`Network::remove_node`] is O(degree) rather than O(V).
///
/// Serialises as a plain `{ nodes, edges }` structure of [`AgentId`]s
/// (petgraph's internal `NodeIndex`es are *not* persisted) and is rebuilt on
/// load, so snapshots stay stable across petgraph versions.  For weighted
/// networks (`E: Serialize`) each edge additionally carries its payload.
#[derive(Clone)]
pub struct Network<E = (), Ty = Undirected>
where
    Ty: petgraph::EdgeType,
{
    graph: StableGraph<AgentId, E, Ty>,
    index: HashMap<AgentId, NodeIndex>,
    _ty: PhantomData<Ty>,
}

/// An **undirected** social network with no edge payload.
///
/// This is the original `SocialNetwork`: all existing methods and the
/// `{ nodes, edges }` serde format are preserved.
pub type SocialNetwork = Network<(), Undirected>;

/// A **directed** social network with no edge payload.
///
/// `add_edge(a, b)` adds the directed arc `a → b`.  Use
/// [`Network::out_neighbors`] / [`Network::in_neighbors`] to follow arcs in a
/// given direction; [`Network::neighbors`] returns successors (out-neighbours).
pub type DiSocialNetwork = Network<(), Directed>;

/// An **undirected, weighted** social network carrying a payload `E` per edge.
pub type WeightedNetwork<E> = Network<E, Undirected>;

/// A **directed, weighted** social network carrying a payload `E` per edge.
pub type DiWeightedNetwork<E> = Network<E, Directed>;

// ── serde ─────────────────────────────────────────────────────────────────────

/// Wire format for an **unweighted** [`Network`]: node list + edge list.
///
/// This matches the original `SocialNetwork` format exactly, so snapshots
/// written by earlier versions still deserialise.
#[derive(Serialize, Deserialize)]
struct NetData {
    nodes: Vec<AgentId>,
    edges: Vec<(AgentId, AgentId)>,
}

/// Wire format for a **weighted** [`Network`]: node list + `(a, b, weight)`
/// edge list.
#[derive(Serialize, Deserialize)]
struct WeightedNetData<E> {
    nodes: Vec<AgentId>,
    edges: Vec<(AgentId, AgentId, E)>,
}

// Unweighted (`E = ()`) serde keeps the historical `{ nodes, edges }` shape.
impl<Ty> Serialize for Network<(), Ty>
where
    Ty: petgraph::EdgeType,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut nodes: Vec<AgentId> = self.index.keys().copied().collect();
        nodes.sort();

        let mut edges: Vec<(AgentId, AgentId)> = self.canonical_endpoints().collect();
        edges.sort();

        NetData { nodes, edges }.serialize(serializer)
    }
}

impl<'de, Ty> Deserialize<'de> for Network<(), Ty>
where
    Ty: petgraph::EdgeType,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = NetData::deserialize(deserializer)?;
        let mut net = Network::empty();
        for id in data.nodes {
            net.add_node(id);
        }
        for (a, b) in data.edges {
            net.add_edge(a, b);
        }
        Ok(net)
    }
}

impl<E, Ty> Network<E, Ty>
where
    E: Serialize + Clone,
    Ty: petgraph::EdgeType,
{
    /// Serialise a **weighted** network as `{ nodes, edges: [(a, b, weight)] }`.
    ///
    /// `()`-payload networks use the historical `{ nodes, edges }` format via
    /// the blanket [`Serialize`] impl; this method is for `E != ()` payloads,
    /// for which a generic serde impl would conflict with that one.
    pub fn to_weighted_json(&self) -> Result<String, serde_json::Error> {
        let mut nodes: Vec<AgentId> = self.index.keys().copied().collect();
        nodes.sort();

        let edges: Vec<(AgentId, AgentId, E)> = self
            .graph
            .edge_references()
            .map(|e| {
                let a = self.graph[e.source()];
                let b = self.graph[e.target()];
                (a, b, e.weight().clone())
            })
            .collect();

        serde_json::to_string(&WeightedNetData { nodes, edges })
    }
}

impl<E, Ty> Network<E, Ty>
where
    E: for<'de> Deserialize<'de>,
    Ty: petgraph::EdgeType,
{
    /// Rebuild a **weighted** network from the `{ nodes, edges }` JSON written
    /// by [`Network::to_weighted_json`].
    pub fn from_weighted_json(json: &str) -> Result<Self, serde_json::Error> {
        let data: WeightedNetData<E> = serde_json::from_str(json)?;
        let mut net = Network::empty();
        for id in data.nodes {
            net.add_node(id);
        }
        for (a, b, w) in data.edges {
            net.add_edge_weighted(a, b, w);
        }
        Ok(net)
    }
}

// ── construction / mutation (generic over E, Ty) ────────────────────────────

impl<E, Ty> Default for Network<E, Ty>
where
    Ty: petgraph::EdgeType,
{
    fn default() -> Self {
        Self::empty()
    }
}

impl<E, Ty> Network<E, Ty>
where
    Ty: petgraph::EdgeType,
{
    /// Create an empty network with no nodes or edges.
    pub fn empty() -> Self {
        Self {
            graph: StableGraph::default(),
            index: HashMap::new(),
            _ty: PhantomData,
        }
    }

    /// `true` if this network is directed (its `Ty` is [`Directed`]).
    pub fn is_directed(&self) -> bool {
        Ty::is_directed()
    }

    /// Add a node for `id`.  No-op if `id` is already present.
    pub fn add_node(&mut self, id: AgentId) {
        self.index
            .entry(id)
            .or_insert_with(|| self.graph.add_node(id));
    }

    /// Add an edge between `a` and `b` carrying the payload `weight`.
    ///
    /// For an undirected network the edge is symmetric; for a directed network
    /// it is the arc `a → b`.  Both nodes must already exist (call
    /// [`add_node`](Self::add_node) first).  If an edge already exists between
    /// the two endpoints (in this direction, for directed graphs) its payload
    /// is **overwritten** with `weight`.
    pub fn add_edge_weighted(&mut self, a: AgentId, b: AgentId, weight: E) {
        if let (Some(&na), Some(&nb)) = (self.index.get(&a), self.index.get(&b)) {
            match self.directed_find_edge(na, nb) {
                Some(e) => {
                    self.graph[e] = weight;
                }
                None => {
                    self.graph.add_edge(na, nb, weight);
                }
            }
        }
    }

    /// Remove the edge between `a` and `b` (the arc `a → b` for directed
    /// graphs).  Returns `true` if an edge was removed.
    pub fn remove_edge(&mut self, a: AgentId, b: AgentId) -> bool {
        if let (Some(&na), Some(&nb)) = (self.index.get(&a), self.index.get(&b)) {
            if let Some(e) = self.directed_find_edge(na, nb) {
                self.graph.remove_edge(e);
                return true;
            }
        }
        false
    }

    /// Remove a node and all its incident edges from the network.
    ///
    /// Returns `true` if the node existed, `false` otherwise.
    ///
    /// Because the backing store is a [`StableGraph`], removing a node leaves
    /// every other node's `NodeIndex` untouched, so this is **O(degree)** (drop
    /// the node's incident edges + one `HashMap` removal) rather than the
    /// O(V) full-index rebuild the non-stable graph required.
    pub fn remove_node(&mut self, id: AgentId) -> bool {
        if let Some(ni) = self.index.remove(&id) {
            self.graph.remove_node(ni);
            true
        } else {
            false
        }
    }

    /// Return the payload of the edge between `a` and `b` (arc `a → b` for
    /// directed graphs), or `None` if there is no such edge.
    pub fn edge_weight(&self, a: AgentId, b: AgentId) -> Option<&E> {
        let (&na, &nb) = (self.index.get(&a)?, self.index.get(&b)?);
        let e = self.directed_find_edge(na, nb)?;
        self.graph.edge_weight(e)
    }

    /// Mutable access to the payload of the edge between `a` and `b` (arc
    /// `a → b` for directed graphs), or `None` if there is no such edge.
    pub fn edge_weight_mut(&mut self, a: AgentId, b: AgentId) -> Option<&mut E> {
        let na = *self.index.get(&a)?;
        let nb = *self.index.get(&b)?;
        let e = self.directed_find_edge(na, nb)?;
        self.graph.edge_weight_mut(e)
    }

    // ── Queries ─────────────────────────────────────────────────────────────

    /// Return all neighbours of `id`.
    ///
    /// For a directed network these are the **successors** (out-neighbours,
    /// i.e. heads of arcs `id → x`); use [`in_neighbors`](Self::in_neighbors)
    /// for predecessors.  Returns an empty `Vec` if `id` is not present.
    ///
    /// This allocates a fresh `Vec` per call; for hot loops prefer
    /// [`neighbors_into`](Self::neighbors_into) (reuse a buffer) or
    /// [`neighbors_iter`](Self::neighbors_iter) (no heap allocation).
    pub fn neighbors(&self, id: AgentId) -> Vec<AgentId> {
        let mut out = Vec::new();
        self.neighbors_into(id, &mut out);
        out
    }

    /// Like [`neighbors`](Self::neighbors), but writes into a caller-supplied
    /// buffer.
    ///
    /// `out` is cleared and then filled with the neighbours of `id`.  Reusing a
    /// single `Vec` across many calls avoids a per-call heap allocation,
    /// mirroring `socsim-grid`'s `neighbors_into`.
    pub fn neighbors_into(&self, id: AgentId, out: &mut Vec<AgentId>) {
        out.clear();
        if let Some(&ni) = self.index.get(&id) {
            // For directed graphs, `neighbors` already means "outgoing".
            out.extend(self.graph.neighbors(ni).map(|nb| self.graph[nb]));
        }
    }

    /// A non-allocating iterator over the neighbours of `id` (successors for
    /// directed networks), borrowing from `self`.
    ///
    /// Yields nothing if `id` is not present.  No heap `Vec` is allocated.
    pub fn neighbors_iter(&self, id: AgentId) -> impl Iterator<Item = AgentId> + '_ {
        self.index
            .get(&id)
            .copied()
            .into_iter()
            .flat_map(move |ni| self.graph.neighbors(ni).map(move |nb| self.graph[nb]))
    }

    /// Return the degree of `id`.
    ///
    /// For a directed network this is the **out-degree** (number of outgoing
    /// arcs); use [`in_neighbors`](Self::in_neighbors)`.count()` for in-degree.
    /// Returns `0` if `id` is not present.
    pub fn degree(&self, id: AgentId) -> usize {
        match self.index.get(&id) {
            Some(&ni) => self.graph.neighbors(ni).count(),
            None => 0,
        }
    }

    /// Total number of nodes in the network.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Total number of edges in the network.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Number of connected components.
    ///
    /// For directed graphs this counts **weakly** connected components (edge
    /// direction is ignored).  Computed by labelling every node with
    /// [`component_membership`](Self::component_membership) (a BFS over the
    /// undirected view), since the backing [`StableGraph`] does not implement
    /// petgraph's `NodeCompactIndexable` bound required by
    /// `algo::connected_components`.
    pub fn connected_components(&self) -> usize {
        if self.node_count() == 0 {
            return 0;
        }
        let comp = self.component_membership();
        let mut labels: Vec<usize> = comp.into_values().collect();
        labels.sort_unstable();
        labels.dedup();
        labels.len()
    }

    /// Returns `true` if `id` exists in the network.
    pub fn contains(&self, id: AgentId) -> bool {
        self.index.contains_key(&id)
    }

    /// The component id of every node, as an `AgentId → usize` map.
    ///
    /// Components are numbered `0..k` in order of their smallest member
    /// `AgentId`, so the labelling is deterministic.  For directed graphs this
    /// uses **weak** connectivity (edge direction ignored).
    pub fn component_membership(&self) -> HashMap<AgentId, usize> {
        let mut ids: Vec<AgentId> = self.index.keys().copied().collect();
        ids.sort();

        let mut comp: HashMap<AgentId, usize> = HashMap::new();
        let mut next = 0usize;
        for &start in &ids {
            if comp.contains_key(&start) {
                continue;
            }
            // BFS over the *undirected* view (both in- and out-neighbours).
            let label = next;
            next += 1;
            let mut queue = VecDeque::new();
            queue.push_back(start);
            comp.insert(start, label);
            while let Some(cur) = queue.pop_front() {
                let cni = self.index[&cur];
                let it = self
                    .graph
                    .neighbors_directed(cni, Direction::Outgoing)
                    .chain(self.graph.neighbors_directed(cni, Direction::Incoming));
                for nb in it {
                    let nid = self.graph[nb];
                    if let std::collections::hash_map::Entry::Vacant(e) = comp.entry(nid) {
                        e.insert(label);
                        queue.push_back(nid);
                    }
                }
            }
        }
        comp
    }

    // ── internal helpers ──────────────────────────────────────────────────────

    /// Find the edge `na → nb`.  For undirected graphs petgraph treats the
    /// pair symmetrically; for directed graphs this respects orientation.
    fn directed_find_edge(
        &self,
        na: NodeIndex,
        nb: NodeIndex,
    ) -> Option<petgraph::stable_graph::EdgeIndex> {
        self.graph.find_edge(na, nb)
    }

    /// Iterate edge endpoints as `AgentId` pairs, canonicalised (`a <= b`) for
    /// undirected graphs and left as `(source, target)` for directed graphs.
    fn canonical_endpoints(&self) -> impl Iterator<Item = (AgentId, AgentId)> + '_ {
        self.graph.edge_indices().filter_map(move |e| {
            let (a, b) = self.graph.edge_endpoints(e)?;
            let (ia, ib) = (self.graph[a], self.graph[b]);
            if !Ty::is_directed() && ia > ib {
                Some((ib, ia))
            } else {
                Some((ia, ib))
            }
        })
    }
}

// ── directed-specific queries ───────────────────────────────────────────────

impl<E> Network<E, Directed> {
    /// Out-neighbours (successors) of `id`: heads of arcs `id → x`.
    pub fn out_neighbors(&self, id: AgentId) -> Vec<AgentId> {
        self.neighbors_directed(id, Direction::Outgoing)
    }

    /// In-neighbours (predecessors) of `id`: tails of arcs `x → id`.
    pub fn in_neighbors(&self, id: AgentId) -> Vec<AgentId> {
        self.neighbors_directed(id, Direction::Incoming)
    }

    /// Neighbours of `id` in the given [`Direction`].
    pub fn neighbors_directed(&self, id: AgentId, dir: Direction) -> Vec<AgentId> {
        match self.index.get(&id) {
            Some(&ni) => self
                .graph
                .neighbors_directed(ni, dir)
                .map(|nb| self.graph[nb])
                .collect(),
            None => Vec::new(),
        }
    }

    /// Out-degree of `id` (number of outgoing arcs).
    pub fn out_degree(&self, id: AgentId) -> usize {
        match self.index.get(&id) {
            Some(&ni) => self
                .graph
                .neighbors_directed(ni, Direction::Outgoing)
                .count(),
            None => 0,
        }
    }

    /// In-degree of `id` (number of incoming arcs).
    pub fn in_degree(&self, id: AgentId) -> usize {
        match self.index.get(&id) {
            Some(&ni) => self
                .graph
                .neighbors_directed(ni, Direction::Incoming)
                .count(),
            None => 0,
        }
    }
}

// ── unweighted convenience (E = ()) ─────────────────────────────────────────

impl<Ty> Network<(), Ty>
where
    Ty: petgraph::EdgeType,
{
    /// Add an edge between `a` and `b` with no payload.
    ///
    /// For undirected networks the edge is symmetric; for directed networks it
    /// is the arc `a → b`.  Both nodes must exist (call
    /// [`add_node`](Self::add_node) first).  Duplicate edges are silently
    /// ignored.
    pub fn add_edge(&mut self, a: AgentId, b: AgentId) {
        if let (Some(&na), Some(&nb)) = (self.index.get(&a), self.index.get(&b)) {
            if self.graph.find_edge(na, nb).is_none() {
                self.graph.add_edge(na, nb, ());
            }
        }
    }
}

// ── generators (undirected, unweighted) ─────────────────────────────────────

impl Network<(), Undirected> {
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

    /// StableGraph keeps surviving indices valid: a node added before a removal
    /// must still resolve to the right neighbours afterwards.
    #[test]
    fn remove_node_keeps_other_indices_valid() {
        let mut net = SocialNetwork::empty();
        for id in ids(4) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(3));
        net.add_edge(AgentId(1), AgentId(3));
        net.remove_node(AgentId(1));
        // 0–3 edge must survive, and 3 must still know about 0.
        assert!(net.neighbors(AgentId(0)).contains(&AgentId(3)));
        assert!(net.neighbors(AgentId(3)).contains(&AgentId(0)));
        assert!(!net.neighbors(AgentId(3)).contains(&AgentId(1)));
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

    #[test]
    fn serde_round_trip_preserves_topology() {
        let mut rng = SimRng::from_seed(3);
        let ids = ids(8);
        let net = SocialNetwork::watts_strogatz(&ids, 4, 0.2, &mut rng);

        let json = serde_json::to_string(&net).unwrap();
        let restored: SocialNetwork = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.node_count(), net.node_count());
        assert_eq!(restored.connected_components(), net.connected_components());
        for &id in &ids {
            let mut a = net.neighbors(id);
            let mut b = restored.neighbors(id);
            a.sort();
            b.sort();
            assert_eq!(a, b, "neighbours of {id:?} must survive round-trip");
        }
    }

    #[test]
    fn serde_format_is_nodes_edges() {
        // Lock the historical wire shape: a plain { nodes, edges } object.
        let mut net = SocialNetwork::empty();
        net.add_node(AgentId(0));
        net.add_node(AgentId(1));
        net.add_edge(AgentId(0), AgentId(1));
        let v: serde_json::Value = serde_json::to_value(&net).unwrap();
        assert!(v.get("nodes").is_some());
        assert!(v.get("edges").is_some());
    }

    // ── #18: directed ──────────────────────────────────────────────────────────

    #[test]
    fn directed_in_out_neighbors() {
        let mut net = DiSocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1)); // 0 → 1
        net.add_edge(AgentId(2), AgentId(1)); // 2 → 1

        assert_eq!(net.out_neighbors(AgentId(0)), vec![AgentId(1)]);
        assert!(net.in_neighbors(AgentId(0)).is_empty());

        let mut inn = net.in_neighbors(AgentId(1));
        inn.sort();
        assert_eq!(inn, vec![AgentId(0), AgentId(2)]);
        assert!(net.out_neighbors(AgentId(1)).is_empty());

        assert_eq!(net.out_degree(AgentId(0)), 1);
        assert_eq!(net.in_degree(AgentId(1)), 2);
        assert!(net.is_directed());
    }

    #[test]
    fn directed_neighbors_means_outgoing() {
        let mut net = DiSocialNetwork::empty();
        net.add_node(AgentId(0));
        net.add_node(AgentId(1));
        net.add_edge(AgentId(0), AgentId(1));
        assert_eq!(net.neighbors(AgentId(0)), vec![AgentId(1)]);
        assert!(net.neighbors(AgentId(1)).is_empty()); // no arc 1 → 0
    }

    // ── #18: weighted ────────────────────────────────────────────────────────

    #[test]
    fn weighted_edge_get_set() {
        let mut net: WeightedNetwork<f64> = Network::empty();
        for id in ids(2) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 0.75);
        assert_eq!(net.edge_weight(AgentId(0), AgentId(1)), Some(&0.75));
        // Undirected: weight is visible from the other endpoint too.
        assert_eq!(net.edge_weight(AgentId(1), AgentId(0)), Some(&0.75));

        *net.edge_weight_mut(AgentId(0), AgentId(1)).unwrap() = 0.25;
        assert_eq!(net.edge_weight(AgentId(0), AgentId(1)), Some(&0.25));

        // Re-adding overwrites rather than duplicating.
        net.add_edge_weighted(AgentId(0), AgentId(1), 0.5);
        assert_eq!(net.edge_count(), 1);
        assert_eq!(net.edge_weight(AgentId(0), AgentId(1)), Some(&0.5));
    }

    #[test]
    fn weighted_directed_asymmetric() {
        let mut net: DiWeightedNetwork<i32> = Network::empty();
        for id in ids(2) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 7);
        assert_eq!(net.edge_weight(AgentId(0), AgentId(1)), Some(&7));
        assert_eq!(net.edge_weight(AgentId(1), AgentId(0)), None); // no reverse arc
    }

    #[test]
    fn weighted_serde_round_trip() {
        let mut net: WeightedNetwork<f64> = Network::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 1.5);
        net.add_edge_weighted(AgentId(1), AgentId(2), 2.5);
        let json = net.to_weighted_json().unwrap();
        let restored: WeightedNetwork<f64> = Network::from_weighted_json(&json).unwrap();
        assert_eq!(restored.edge_count(), 2);
        assert_eq!(restored.edge_weight(AgentId(0), AgentId(1)), Some(&1.5));
        assert_eq!(restored.edge_weight(AgentId(1), AgentId(2)), Some(&2.5));
    }

    // ── #19: zero-alloc / remove_edge ──────────────────────────────────────────

    #[test]
    fn neighbors_into_matches_neighbors() {
        let mut rng = SimRng::from_seed(5);
        let ids = ids(12);
        let net = SocialNetwork::erdos_renyi(&ids, 0.4, &mut rng);
        let mut buf = Vec::new();
        for &id in &ids {
            net.neighbors_into(id, &mut buf);
            let mut a = buf.clone();
            let mut b = net.neighbors(id);
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn neighbors_iter_matches_neighbors() {
        let mut rng = SimRng::from_seed(6);
        let ids = ids(12);
        let net = SocialNetwork::erdos_renyi(&ids, 0.4, &mut rng);
        for &id in &ids {
            let mut a: Vec<AgentId> = net.neighbors_iter(id).collect();
            let mut b = net.neighbors(id);
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn remove_edge_works() {
        let mut net = SocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1));
        net.add_edge(AgentId(1), AgentId(2));
        assert!(net.remove_edge(AgentId(0), AgentId(1)));
        assert!(!net.neighbors(AgentId(0)).contains(&AgentId(1)));
        assert!(!net.neighbors(AgentId(1)).contains(&AgentId(0)));
        // 1–2 untouched.
        assert!(net.neighbors(AgentId(1)).contains(&AgentId(2)));
        // Removing a missing edge returns false.
        assert!(!net.remove_edge(AgentId(0), AgentId(2)));
    }

    #[test]
    fn remove_edge_directed_respects_orientation() {
        let mut net = DiSocialNetwork::empty();
        net.add_node(AgentId(0));
        net.add_node(AgentId(1));
        net.add_edge(AgentId(0), AgentId(1)); // 0 → 1
                                              // Removing the reverse arc fails; the forward arc remains.
        assert!(!net.remove_edge(AgentId(1), AgentId(0)));
        assert!(net.remove_edge(AgentId(0), AgentId(1)));
        assert_eq!(net.edge_count(), 0);
    }
}
