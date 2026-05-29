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
//! | [`Network::erdos_renyi_directed`] | Directed Erdős–Rényi (independent arc per ordered pair) |
//! | [`Network::barabasi_albert_directed`] | Directed Barabási–Albert (preferential attachment on in-degree) |
//! | [`Network::to_directed`] | Assign directions to an undirected network |
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
#[derive(Clone, Debug)]
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

    // ── analysis / export (#20) ───────────────────────────────────────────────

    /// Iterate over every edge as an `(a, b)` [`AgentId`] pair.
    ///
    /// For undirected graphs each edge is yielded once with `a <= b`
    /// (canonical orientation), so the output is suitable for an undirected
    /// edge-list export.  For directed graphs each arc is yielded as
    /// `(source, target)`.
    pub fn edges(&self) -> impl Iterator<Item = (AgentId, AgentId)> + '_ {
        self.canonical_endpoints()
    }

    /// Iterate over every edge together with a reference to its payload:
    /// `(a, b, &weight)`, with the same orientation rules as
    /// [`edges`](Self::edges).
    pub fn weighted_edges(&self) -> impl Iterator<Item = (AgentId, AgentId, &E)> + '_ {
        self.graph.edge_references().map(move |e| {
            let a = self.graph[e.source()];
            let b = self.graph[e.target()];
            if !Ty::is_directed() && a > b {
                (b, a, e.weight())
            } else {
                (a, b, e.weight())
            }
        })
    }

    /// Iterate over the neighbours of `id` together with a reference to the
    /// connecting edge's payload: `(neighbour, &weight)`.
    ///
    /// This is the per-node analogue of [`weighted_edges`](Self::weighted_edges)
    /// and the weighted counterpart of [`neighbors_iter`](Self::neighbors_iter):
    /// with it, weight-filtered traversal is a one-liner
    /// (`.filter(|(_, w)| pred(w))`).
    ///
    /// For an undirected network these are all incident edges; for a directed
    /// network they are the **outgoing** edges (heads of arcs `id → x`), matching
    /// [`neighbors`](Self::neighbors) / [`neighbors_iter`](Self::neighbors_iter).
    /// Use [`weighted_out_neighbors`](Self::weighted_out_neighbors) /
    /// [`weighted_in_neighbors`](Self::weighted_in_neighbors) on a directed
    /// network to pick a direction explicitly.
    ///
    /// Yields nothing if `id` is not present.  No heap `Vec` is allocated.
    pub fn weighted_neighbors(&self, id: AgentId) -> impl Iterator<Item = (AgentId, &E)> + '_ {
        self.index
            .get(&id)
            .copied()
            .into_iter()
            .flat_map(move |ni| {
                // `edges` walks the outgoing incidence list; for an undirected
                // graph that is every incident edge.
                self.graph.edges(ni)
            })
            .map(move |e| (self.graph[e.target()], e.weight()))
    }

    /// The set of nodes reachable from `seed`, traversing **only** edges whose
    /// payload satisfies `edge_allowed`.
    ///
    /// A BFS over the predicate-filtered subgraph.  The returned `Vec` is sorted
    /// for determinism and **includes `seed` itself** (a node is reachable from
    /// itself in zero hops).  Returns an empty `Vec` if `seed` is not present.
    ///
    /// For directed graphs the traversal **follows arc direction** (it visits
    /// successors only), mirroring [`neighbors`](Self::neighbors); the edge
    /// payload tested is that of each outgoing arc.  For undirected graphs every
    /// incident edge is considered.
    ///
    /// # Example
    ///
    /// Strong-only vs. weak-allowing reachability over a tie-strength network
    /// (Granovetter's "strength of weak ties"): filtering to strong ties stays
    /// within a cluster, while allowing the weak bridge crosses to the other
    /// cluster.
    pub fn reachable_from<F: Fn(&E) -> bool>(
        &self,
        seed: AgentId,
        edge_allowed: F,
    ) -> Vec<AgentId> {
        let start = match self.index.get(&seed) {
            Some(&ni) => ni,
            None => return Vec::new(),
        };
        let mut visited: std::collections::HashSet<NodeIndex> = std::collections::HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            // `edges` yields the outgoing incidence list; for undirected graphs
            // that covers every incident edge.
            for e in self.graph.edges(cur) {
                if edge_allowed(e.weight()) {
                    let nb = e.target();
                    if visited.insert(nb) {
                        queue.push_back(nb);
                    }
                }
            }
        }
        let mut out: Vec<AgentId> = visited.into_iter().map(|ni| self.graph[ni]).collect();
        out.sort();
        out
    }

    /// The degree sequence: every node's [`degree`](Self::degree), sorted
    /// descending.  Node order is deterministic (sorted by `AgentId` before
    /// sorting by degree).
    pub fn degree_sequence(&self) -> Vec<usize> {
        let mut ids: Vec<AgentId> = self.index.keys().copied().collect();
        ids.sort();
        let mut degs: Vec<usize> = ids.into_iter().map(|id| self.degree(id)).collect();
        degs.sort_unstable_by(|a, b| b.cmp(a));
        degs
    }

    /// The degree distribution: `histogram[d]` = number of nodes with degree
    /// exactly `d`.  The returned `Vec` has length `max_degree + 1` (empty if
    /// the network has no nodes).
    pub fn degree_distribution(&self) -> Vec<usize> {
        let degs = self.degree_sequence();
        let max = degs.first().copied().unwrap_or(0);
        if self.node_count() == 0 {
            return Vec::new();
        }
        let mut hist = vec![0usize; max + 1];
        for d in degs {
            hist[d] += 1;
        }
        hist
    }

    /// Number of nodes in the largest connected component (0 for an empty
    /// network).  Weak connectivity for directed graphs.
    pub fn largest_component_size(&self) -> usize {
        let comp = self.component_membership();
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for &c in comp.values() {
            *counts.entry(c).or_insert(0) += 1;
        }
        counts.values().copied().max().unwrap_or(0)
    }

    /// Average shortest-path length over all reachable ordered node pairs.
    ///
    /// Computed by an unweighted BFS from every node.  Returns `None` if the
    /// network has fewer than two nodes or no reachable pairs at all.
    ///
    /// **Disconnected handling:** unreachable pairs are *excluded* from the
    /// average (the average is taken over reachable pairs only), so a
    /// disconnected graph still yields a finite value describing its connected
    /// portions rather than `inf`.  For directed graphs, "reachable" follows
    /// arc direction.
    pub fn average_path_length(&self) -> Option<f64> {
        if self.node_count() < 2 {
            return None;
        }
        let mut total: u64 = 0;
        let mut pairs: u64 = 0;
        let starts: Vec<NodeIndex> = self.graph.node_indices().collect();
        for &s in &starts {
            for (idx, d) in self.bfs_distances(s) {
                if idx != s {
                    total += d as u64;
                    pairs += 1;
                }
            }
        }
        if pairs == 0 {
            None
        } else {
            Some(total as f64 / pairs as f64)
        }
    }

    /// The clustering coefficient of node `id`: the fraction of its neighbour
    /// pairs that are themselves connected (local transitivity).
    ///
    /// Returns `None` for an absent node or a node with fewer than two
    /// neighbours (the coefficient is undefined there).  For directed graphs
    /// the underlying neighbour relation is taken **undirectedly** (a node's
    /// neighbours are all nodes adjacent by an arc in either direction).
    pub fn clustering_coefficient(&self, id: AgentId) -> Option<f64> {
        let &ni = self.index.get(&id)?;
        let neighbours: Vec<NodeIndex> = self.undirected_neighbor_indices(ni);
        let k = neighbours.len();
        if k < 2 {
            return None;
        }
        let mut links = 0usize;
        for i in 0..k {
            for j in (i + 1)..k {
                if self.undirected_adjacent(neighbours[i], neighbours[j]) {
                    links += 1;
                }
            }
        }
        let possible = k * (k - 1) / 2;
        Some(links as f64 / possible as f64)
    }

    /// The **average** clustering coefficient over all nodes that have at least
    /// two neighbours.  Returns `None` if no such node exists.
    pub fn average_clustering_coefficient(&self) -> Option<f64> {
        let mut ids: Vec<AgentId> = self.index.keys().copied().collect();
        ids.sort();
        let mut sum = 0.0;
        let mut count = 0usize;
        for id in ids {
            if let Some(c) = self.clustering_coefficient(id) {
                sum += c;
                count += 1;
            }
        }
        if count == 0 {
            None
        } else {
            Some(sum / count as f64)
        }
    }

    /// Whether the edge `(a, b)` is a **local bridge** in Granovetter's sense:
    /// an edge whose endpoints would be more than 2 hops apart if it were
    /// removed (equivalently, `a` and `b` share no common neighbour).
    ///
    /// Returns `false` if there is no edge between `a` and `b`.  Connectivity
    /// is evaluated **undirectedly**.
    pub fn is_local_bridge(&self, a: AgentId, b: AgentId) -> bool {
        let (na, nb) = match (self.index.get(&a), self.index.get(&b)) {
            (Some(&na), Some(&nb)) => (na, nb),
            _ => return false,
        };
        if !self.undirected_adjacent(na, nb) {
            return false;
        }
        // A local bridge has no shared neighbour: removing it pushes the
        // endpoints' distance above 2.
        let an: Vec<NodeIndex> = self.undirected_neighbor_indices(na);
        let bn: Vec<NodeIndex> = self.undirected_neighbor_indices(nb);
        for x in &an {
            if *x == nb {
                continue;
            }
            if bn.contains(x) {
                return false; // shared neighbour ⇒ distance stays 2 ⇒ not a bridge
            }
        }
        true
    }

    /// All local bridges in the network (see [`is_local_bridge`]).
    ///
    /// Each is returned once as a canonical `(a, b)` pair with `a <= b` for
    /// undirected graphs.
    ///
    /// [`is_local_bridge`]: Self::is_local_bridge
    pub fn local_bridges(&self) -> Vec<(AgentId, AgentId)> {
        let mut out: Vec<(AgentId, AgentId)> = self
            .edges()
            .filter(|&(a, b)| self.is_local_bridge(a, b))
            .collect();
        out.sort();
        out.dedup();
        out
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

    /// Neighbour node indices treating the graph as undirected (union of in-
    /// and out-neighbours, deduplicated).
    fn undirected_neighbor_indices(&self, ni: NodeIndex) -> Vec<NodeIndex> {
        if Ty::is_directed() {
            let mut v: Vec<NodeIndex> = self
                .graph
                .neighbors_directed(ni, Direction::Outgoing)
                .chain(self.graph.neighbors_directed(ni, Direction::Incoming))
                .collect();
            v.sort();
            v.dedup();
            v.retain(|&x| x != ni);
            v
        } else {
            let mut v: Vec<NodeIndex> = self.graph.neighbors(ni).collect();
            v.sort();
            v.dedup();
            v
        }
    }

    /// Whether `x` and `y` are adjacent ignoring direction.
    fn undirected_adjacent(&self, x: NodeIndex, y: NodeIndex) -> bool {
        self.graph.find_edge(x, y).is_some() || self.graph.find_edge(y, x).is_some()
    }

    /// BFS shortest-path distances (in hops) from `start` to every reachable
    /// node, following arc direction for directed graphs.
    fn bfs_distances(&self, start: NodeIndex) -> Vec<(NodeIndex, u32)> {
        let mut dist: HashMap<NodeIndex, u32> = HashMap::new();
        let mut queue = VecDeque::new();
        dist.insert(start, 0);
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            let d = dist[&cur];
            for nb in self.graph.neighbors(cur) {
                if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(nb) {
                    e.insert(d + 1);
                    queue.push_back(nb);
                }
            }
        }
        dist.into_iter().collect()
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

    /// Out-neighbours (successors) of `id` paired with the connecting arc's
    /// payload: `(neighbour, &weight)` for each arc `id → x`.
    ///
    /// The directed, payload-carrying analogue of
    /// [`out_neighbors`](Self::out_neighbors).  Yields nothing if `id` is
    /// absent.  Equivalent to [`weighted_neighbors`](Self::weighted_neighbors)
    /// on a directed network, but named explicitly for symmetry with
    /// [`weighted_in_neighbors`](Self::weighted_in_neighbors).
    pub fn weighted_out_neighbors(&self, id: AgentId) -> impl Iterator<Item = (AgentId, &E)> + '_ {
        self.weighted_neighbors_directed(id, Direction::Outgoing)
    }

    /// In-neighbours (predecessors) of `id` paired with the connecting arc's
    /// payload: `(neighbour, &weight)` for each arc `x → id`.
    ///
    /// The directed, payload-carrying analogue of
    /// [`in_neighbors`](Self::in_neighbors).  Yields nothing if `id` is absent.
    pub fn weighted_in_neighbors(&self, id: AgentId) -> impl Iterator<Item = (AgentId, &E)> + '_ {
        self.weighted_neighbors_directed(id, Direction::Incoming)
    }

    /// Neighbours of `id` in the given [`Direction`] paired with the connecting
    /// arc's payload.  The `AgentId` yielded is always the *other* endpoint
    /// (the predecessor for [`Direction::Incoming`], the successor for
    /// [`Direction::Outgoing`]).
    fn weighted_neighbors_directed(
        &self,
        id: AgentId,
        dir: Direction,
    ) -> impl Iterator<Item = (AgentId, &E)> + '_ {
        self.index
            .get(&id)
            .copied()
            .into_iter()
            .flat_map(move |ni| self.graph.edges_directed(ni, dir))
            .map(move |e| {
                // For an incoming arc `x → id`, the other endpoint is the
                // source; for an outgoing arc `id → x` it is the target.
                let other = if e.target() == e.source() {
                    e.target()
                } else if matches!(dir, Direction::Incoming) {
                    e.source()
                } else {
                    e.target()
                };
                (self.graph[other], e.weight())
            })
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

    /// Build a **directed** graph by assigning a direction to each undirected
    /// edge.
    ///
    /// With probability `p_mutual` an edge becomes **bidirectional** (both arcs
    /// `a → b` and `b → a` are added); otherwise a single RNG-chosen direction
    /// is kept.  The full node set is preserved, including isolated nodes.
    ///
    /// Edges are iterated in a deterministic (sorted) order, so for a fixed
    /// `rng` seed the output is fully reproducible.  `p_mutual` is clamped to
    /// `[0.0, 1.0]`.
    pub fn to_directed(&self, p_mutual: f64, rng: &mut SimRng) -> Network<(), Directed> {
        let p_mutual = p_mutual.clamp(0.0, 1.0);
        let mut net = Network::<(), Directed>::empty();

        // Preserve every node, including isolated ones, in a deterministic order.
        let mut nodes: Vec<AgentId> = self.index.keys().copied().collect();
        nodes.sort();
        for id in nodes {
            net.add_node(id);
        }

        // Iterate edges in canonical, sorted order so the RNG draws are
        // reproducible for a fixed seed.
        let mut edges: Vec<(AgentId, AgentId)> = self.edges().collect();
        edges.sort();
        for (a, b) in edges {
            if rng.gen::<f64>() < p_mutual {
                net.add_edge(a, b);
                net.add_edge(b, a);
            } else if rng.gen::<bool>() {
                net.add_edge(a, b);
            } else {
                net.add_edge(b, a);
            }
        }

        net
    }
}

// ── generators (directed, unweighted) ───────────────────────────────────────

impl Network<(), Directed> {
    /// **Directed Erdős–Rényi**: add each possible arc independently with
    /// probability `p`.
    ///
    /// Unlike the undirected [`erdos_renyi`](Network::erdos_renyi), each
    /// **ordered** pair `(i, j)` with `i != j` is considered separately, so the
    /// arcs `i → j` and `j → i` are drawn independently and the result may be
    /// asymmetric.  `p` is clamped to `[0.0, 1.0]`; `p = 1.0` yields the
    /// complete digraph (`n · (n − 1)` arcs).
    pub fn erdos_renyi_directed(ids: &[AgentId], p: f64, rng: &mut SimRng) -> Self {
        let p = p.clamp(0.0, 1.0);
        let mut net = Self::empty();
        for &id in ids {
            net.add_node(id);
        }
        let n = ids.len();
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                if rng.gen::<f64>() < p {
                    net.add_edge(ids[i], ids[j]);
                }
            }
        }
        net
    }

    /// **Directed Barabási–Albert** preferential attachment on **in-degree**.
    ///
    /// Each new node creates `m` out-arcs to existing nodes chosen with
    /// probability proportional to their current in-degree (+ 1 to avoid
    /// zero-probability isolation for the seed nodes).  Because attachment
    /// favours nodes that are already followed, this produces the heavy-tailed
    /// **in-degree** (follower-count) distribution typical of follow networks
    /// while every non-seed node keeps an out-degree of `m` (the number it
    /// follows).
    ///
    /// Mirrors the undirected [`barabasi_albert`](Network::barabasi_albert)
    /// seed-clique handling, adapted to arcs: the seed nodes form a mutually
    /// connected clique (both directions).  `m` is clamped to `[1, n-1]`.
    pub fn barabasi_albert_directed(ids: &[AgentId], m: usize, rng: &mut SimRng) -> Self {
        let n = ids.len();
        let m = m.clamp(1, n.saturating_sub(1).max(1));
        let mut net = Self::empty();

        if n == 0 {
            return net;
        }

        // Seed: connect the first min(m+1, n) nodes as a mutually-linked clique
        // (both directions), so every seed node starts with a non-zero
        // in-degree.
        let seed_n = (m + 1).min(n);
        for &id in &ids[..seed_n] {
            net.add_node(id);
        }
        for i in 0..seed_n {
            for j in (i + 1)..seed_n {
                net.add_edge(ids[i], ids[j]);
                net.add_edge(ids[j], ids[i]);
            }
        }

        // Preferential attachment (on in-degree) for the remaining nodes.
        for &new_id in &ids[seed_n..n] {
            net.add_node(new_id);
            let ni = net.index[&new_id];

            // Build in-degree-weighted cumulative distribution over existing
            // nodes (excluding the new node itself).
            let existing: Vec<NodeIndex> =
                net.graph.node_indices().filter(|&idx| idx != ni).collect();

            let weights: Vec<f64> = existing
                .iter()
                .map(|&idx| {
                    (net.graph
                        .neighbors_directed(idx, Direction::Incoming)
                        .count() as f64)
                        + 1.0
                })
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

            // New node follows the chosen nodes: arcs new_id → target.
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

    // ── #20: analysis ──────────────────────────────────────────────────────────

    #[test]
    fn edges_and_count() {
        let mut net = SocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1));
        net.add_edge(AgentId(1), AgentId(2));
        assert_eq!(net.edge_count(), 2);
        let mut es: Vec<(AgentId, AgentId)> = net.edges().collect();
        es.sort();
        assert_eq!(es, vec![(AgentId(0), AgentId(1)), (AgentId(1), AgentId(2))]);
    }

    #[test]
    fn degree_sequence_and_distribution() {
        // Path 0–1–2: degrees are 1, 2, 1.
        let mut net = SocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1));
        net.add_edge(AgentId(1), AgentId(2));
        assert_eq!(net.degree_sequence(), vec![2, 1, 1]);
        // histogram[0]=0, [1]=2, [2]=1
        assert_eq!(net.degree_distribution(), vec![0, 2, 1]);
    }

    #[test]
    fn average_path_length_path_graph() {
        // Path 0–1–2: undirected ordered-pair distances:
        // (0,1)=1,(1,0)=1,(1,2)=1,(2,1)=1,(0,2)=2,(2,0)=2 ⇒ sum 8 over 6 pairs.
        let mut net = SocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1));
        net.add_edge(AgentId(1), AgentId(2));
        let apl = net.average_path_length().unwrap();
        assert!((apl - (8.0 / 6.0)).abs() < 1e-12, "got {apl}");
    }

    #[test]
    fn clustering_coefficient_triangle_and_path() {
        // Triangle: every node's coefficient is 1.0.
        let mut tri = SocialNetwork::empty();
        for id in ids(3) {
            tri.add_node(id);
        }
        tri.add_edge(AgentId(0), AgentId(1));
        tri.add_edge(AgentId(1), AgentId(2));
        tri.add_edge(AgentId(0), AgentId(2));
        assert_eq!(tri.clustering_coefficient(AgentId(0)), Some(1.0));
        assert_eq!(tri.average_clustering_coefficient(), Some(1.0));

        // Path centre 1 has two neighbours that aren't connected ⇒ 0.0.
        let mut path = SocialNetwork::empty();
        for id in ids(3) {
            path.add_node(id);
        }
        path.add_edge(AgentId(0), AgentId(1));
        path.add_edge(AgentId(1), AgentId(2));
        assert_eq!(path.clustering_coefficient(AgentId(1)), Some(0.0));
        // Endpoints have <2 neighbours ⇒ undefined.
        assert_eq!(path.clustering_coefficient(AgentId(0)), None);
    }

    #[test]
    fn local_bridge_detection() {
        // Two triangles 0-1-2 and 3-4-5 joined by a single 2–3 edge.
        let mut net = SocialNetwork::empty();
        for id in ids(6) {
            net.add_node(id);
        }
        for (a, b) in [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5)] {
            net.add_edge(AgentId(a), AgentId(b));
        }
        net.add_edge(AgentId(2), AgentId(3)); // the bridge

        assert!(net.is_local_bridge(AgentId(2), AgentId(3)));
        // An edge inside a triangle shares a neighbour ⇒ not a bridge.
        assert!(!net.is_local_bridge(AgentId(0), AgentId(1)));
        assert_eq!(net.local_bridges(), vec![(AgentId(2), AgentId(3))]);
    }

    #[test]
    fn component_membership_and_largest() {
        // Two components: {0,1} and {2}.
        let mut net = SocialNetwork::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge(AgentId(0), AgentId(1));
        assert_eq!(net.connected_components(), 2);
        let comp = net.component_membership();
        assert_eq!(comp[&AgentId(0)], comp[&AgentId(1)]);
        assert_ne!(comp[&AgentId(0)], comp[&AgentId(2)]);
        assert_eq!(net.largest_component_size(), 2);
    }

    #[test]
    fn weighted_edges_exposes_payload() {
        let mut net: WeightedNetwork<f64> = Network::empty();
        for id in ids(2) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 0.9);
        let es: Vec<(AgentId, AgentId, f64)> =
            net.weighted_edges().map(|(a, b, w)| (a, b, *w)).collect();
        assert_eq!(es, vec![(AgentId(0), AgentId(1), 0.9)]);
    }

    // ── #24: per-node weighted neighbours + edge-filtered reachability ──────────

    /// Edge label for the Granovetter motivation: strong (within-cluster) vs.
    /// weak (bridging) ties.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Tie {
        Strong,
        Weak,
    }

    #[test]
    fn weighted_neighbors_pairs_match_edge_weights() {
        let mut net: WeightedNetwork<u32> = Network::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 10);
        net.add_edge_weighted(AgentId(0), AgentId(2), 20);

        let mut got: Vec<(AgentId, u32)> = net
            .weighted_neighbors(AgentId(0))
            .map(|(nb, w)| (nb, *w))
            .collect();
        got.sort();
        assert_eq!(got, vec![(AgentId(1), 10), (AgentId(2), 20)]);

        // Each pair's weight matches `edge_weight`.
        for (nb, w) in net.weighted_neighbors(AgentId(0)) {
            assert_eq!(net.edge_weight(AgentId(0), nb), Some(w));
        }
    }

    #[test]
    fn weighted_neighbors_empty_for_absent_node() {
        let net: WeightedNetwork<u32> = Network::empty();
        assert_eq!(net.weighted_neighbors(AgentId(99)).count(), 0);
    }

    #[test]
    fn weighted_neighbors_set_matches_neighbors_ignoring_weights() {
        let mut net: WeightedNetwork<u32> = Network::empty();
        for id in ids(4) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(1), AgentId(0), 5);
        net.add_edge_weighted(AgentId(1), AgentId(2), 6);
        net.add_edge_weighted(AgentId(1), AgentId(3), 7);

        let mut from_weighted: Vec<AgentId> = net
            .weighted_neighbors(AgentId(1))
            .map(|(nb, _)| nb)
            .collect();
        from_weighted.sort();
        let mut plain = net.neighbors(AgentId(1));
        plain.sort();
        assert_eq!(from_weighted, plain);
    }

    #[test]
    fn weighted_out_in_neighbors_directed() {
        let mut net: DiWeightedNetwork<u32> = Network::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), 11); // 0 → 1
        net.add_edge_weighted(AgentId(2), AgentId(1), 22); // 2 → 1

        // Out of 0: just (1, 11).
        let out0: Vec<(AgentId, u32)> = net
            .weighted_out_neighbors(AgentId(0))
            .map(|(nb, w)| (nb, *w))
            .collect();
        assert_eq!(out0, vec![(AgentId(1), 11)]);
        assert_eq!(net.weighted_in_neighbors(AgentId(0)).count(), 0);

        // Into 1: predecessors 0 (w=11) and 2 (w=22).
        let mut in1: Vec<(AgentId, u32)> = net
            .weighted_in_neighbors(AgentId(1))
            .map(|(nb, w)| (nb, *w))
            .collect();
        in1.sort();
        assert_eq!(in1, vec![(AgentId(0), 11), (AgentId(2), 22)]);
        assert_eq!(net.weighted_out_neighbors(AgentId(1)).count(), 0);

        // `weighted_neighbors` on a directed graph follows the outgoing convention.
        let n0: Vec<(AgentId, u32)> = net
            .weighted_neighbors(AgentId(0))
            .map(|(nb, w)| (nb, *w))
            .collect();
        assert_eq!(n0, vec![(AgentId(1), 11)]);
    }

    #[test]
    fn reachable_from_includes_seed_and_filters_edges() {
        // Two strong-tie triangles {0,1,2} and {3,4,5}, bridged by a single
        // *weak* tie 2–3 (Granovetter's "strength of weak ties").
        let mut net: WeightedNetwork<Tie> = Network::empty();
        for id in ids(6) {
            net.add_node(id);
        }
        for (a, b) in [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5)] {
            net.add_edge_weighted(AgentId(a), AgentId(b), Tie::Strong);
        }
        net.add_edge_weighted(AgentId(2), AgentId(3), Tie::Weak); // the bridge

        // Strong-only: stays within the seed's cluster (does NOT cross the bridge).
        let strong = net.reachable_from(AgentId(0), |t| *t == Tie::Strong);
        assert_eq!(strong, vec![AgentId(0), AgentId(1), AgentId(2)]);

        // Allowing all ties: the weak bridge connects everything.
        let all = net.reachable_from(AgentId(0), |_| true);
        assert_eq!(
            all,
            vec![
                AgentId(0),
                AgentId(1),
                AgentId(2),
                AgentId(3),
                AgentId(4),
                AgentId(5)
            ]
        );

        // Seed is always included, even with a predicate that admits no edge.
        let none = net.reachable_from(AgentId(0), |_| false);
        assert_eq!(none, vec![AgentId(0)]);
    }

    #[test]
    fn reachable_from_absent_seed_is_empty() {
        let net: WeightedNetwork<Tie> = Network::empty();
        assert!(net.reachable_from(AgentId(42), |_| true).is_empty());
    }

    #[test]
    fn reachable_from_directed_follows_arc_direction() {
        // Chain 0 → 1 → 2, all strong.  From 0 everything is reachable; from 2
        // only itself (no outgoing arcs).
        let mut net: DiWeightedNetwork<Tie> = Network::empty();
        for id in ids(3) {
            net.add_node(id);
        }
        net.add_edge_weighted(AgentId(0), AgentId(1), Tie::Strong);
        net.add_edge_weighted(AgentId(1), AgentId(2), Tie::Strong);

        assert_eq!(
            net.reachable_from(AgentId(0), |_| true),
            vec![AgentId(0), AgentId(1), AgentId(2)]
        );
        assert_eq!(net.reachable_from(AgentId(2), |_| true), vec![AgentId(2)]);
    }

    // ── #28: to_directed ─────────────────────────────────────────────────────

    /// Every undirected edge yields at least one arc, and the node set
    /// (including isolated nodes) is preserved.
    #[test]
    fn to_directed_preserves_nodes_and_covers_edges() {
        let mut und = SocialNetwork::empty();
        for id in ids(5) {
            und.add_node(id);
        }
        // Node 4 stays isolated.
        und.add_edge(AgentId(0), AgentId(1));
        und.add_edge(AgentId(1), AgentId(2));
        und.add_edge(AgentId(2), AgentId(3));

        let mut rng = SimRng::from_seed(0);
        let di = und.to_directed(0.5, &mut rng);

        assert!(di.is_directed());
        assert_eq!(di.node_count(), 5);
        assert!(di.contains(AgentId(4)));

        // Each undirected edge must produce at least one arc in some direction.
        for (a, b) in und.edges() {
            let has_fwd = di.out_neighbors(a).contains(&b);
            let has_rev = di.out_neighbors(b).contains(&a);
            assert!(has_fwd || has_rev, "edge {a:?}-{b:?} lost all direction");
        }
    }

    /// With `p_mutual = 1.0` every undirected edge becomes bidirectional.
    #[test]
    fn to_directed_p_mutual_one_is_bidirectional() {
        let mut und = SocialNetwork::empty();
        for id in ids(4) {
            und.add_node(id);
        }
        und.add_edge(AgentId(0), AgentId(1));
        und.add_edge(AgentId(1), AgentId(2));
        und.add_edge(AgentId(2), AgentId(3));

        let mut rng = SimRng::from_seed(1);
        let di = und.to_directed(1.0, &mut rng);

        // One undirected edge ⇒ two arcs.
        assert_eq!(di.edge_count(), 2 * und.edge_count());
        for (a, b) in und.edges() {
            assert!(di.out_neighbors(a).contains(&b));
            assert!(di.in_neighbors(a).contains(&b));
            assert!(di.out_neighbors(b).contains(&a));
            assert!(di.in_neighbors(b).contains(&a));
        }
    }

    /// With `p_mutual = 0.0` every undirected edge becomes exactly one arc.
    #[test]
    fn to_directed_p_mutual_zero_is_single_direction() {
        let mut und = SocialNetwork::empty();
        for id in ids(4) {
            und.add_node(id);
        }
        und.add_edge(AgentId(0), AgentId(1));
        und.add_edge(AgentId(1), AgentId(2));
        und.add_edge(AgentId(2), AgentId(3));

        let mut rng = SimRng::from_seed(2);
        let di = und.to_directed(0.0, &mut rng);

        assert_eq!(di.edge_count(), und.edge_count());
        for (a, b) in und.edges() {
            let fwd = di.out_neighbors(a).contains(&b);
            let rev = di.out_neighbors(b).contains(&a);
            // Exactly one direction is present.
            assert!(fwd ^ rev, "edge {a:?}-{b:?} must have exactly one arc");
        }
    }

    #[test]
    fn to_directed_deterministic() {
        let mut rng0 = SimRng::from_seed(99);
        let ids = ids(30);
        let und = SocialNetwork::erdos_renyi(&ids, 0.3, &mut rng0);

        let d1 = und.to_directed(0.4, &mut SimRng::from_seed(7));
        let d2 = und.to_directed(0.4, &mut SimRng::from_seed(7));

        let mut e1: Vec<(AgentId, AgentId)> = d1.edges().collect();
        let mut e2: Vec<(AgentId, AgentId)> = d2.edges().collect();
        e1.sort();
        e2.sort();
        assert_eq!(e1, e2);
    }

    // ── #28: erdos_renyi_directed ────────────────────────────────────────────

    #[test]
    fn erdos_renyi_directed_node_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(15);
        let net = DiSocialNetwork::erdos_renyi_directed(&ids, 0.3, &mut rng);
        assert_eq!(net.node_count(), 15);
        assert!(net.is_directed());
    }

    #[test]
    fn erdos_renyi_directed_p0_no_arcs() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(10);
        let net = DiSocialNetwork::erdos_renyi_directed(&ids, 0.0, &mut rng);
        assert_eq!(net.edge_count(), 0);
    }

    #[test]
    fn erdos_renyi_directed_p1_complete_digraph() {
        let mut rng = SimRng::from_seed(0);
        let n = 6u64;
        let ids = ids(n);
        let net = DiSocialNetwork::erdos_renyi_directed(&ids, 1.0, &mut rng);
        // Complete digraph: n * (n-1) arcs; every node has out- and in-degree n-1.
        assert_eq!(net.edge_count(), (n * (n - 1)) as usize);
        for id in &ids {
            assert_eq!(net.out_degree(*id), (n - 1) as usize);
            assert_eq!(net.in_degree(*id), (n - 1) as usize);
        }
    }

    #[test]
    fn erdos_renyi_directed_arc_count_grows_with_p() {
        let ids = ids(40);
        let low = DiSocialNetwork::erdos_renyi_directed(&ids, 0.1, &mut SimRng::from_seed(1));
        let high = DiSocialNetwork::erdos_renyi_directed(&ids, 0.6, &mut SimRng::from_seed(1));
        assert!(
            high.edge_count() > low.edge_count(),
            "expected more arcs at higher p: {} vs {}",
            high.edge_count(),
            low.edge_count()
        );
    }

    #[test]
    fn erdos_renyi_directed_deterministic() {
        let ids = ids(20);
        let n1 = DiSocialNetwork::erdos_renyi_directed(&ids, 0.4, &mut SimRng::from_seed(42));
        let n2 = DiSocialNetwork::erdos_renyi_directed(&ids, 0.4, &mut SimRng::from_seed(42));
        let mut e1: Vec<(AgentId, AgentId)> = n1.edges().collect();
        let mut e2: Vec<(AgentId, AgentId)> = n2.edges().collect();
        e1.sort();
        e2.sort();
        assert_eq!(e1, e2);
    }

    #[test]
    fn erdos_renyi_directed_can_be_asymmetric() {
        // With independent arc draws, at least one ordered pair should differ
        // from its reverse (an A→B without B→A, or vice versa).
        let ids = ids(30);
        let net = DiSocialNetwork::erdos_renyi_directed(&ids, 0.3, &mut SimRng::from_seed(5));
        let mut asymmetric = false;
        for (a, b) in net.edges() {
            if !net.out_neighbors(b).contains(&a) {
                asymmetric = true;
                break;
            }
        }
        assert!(asymmetric, "expected at least one one-directional arc");
    }

    // ── #28: barabasi_albert_directed ────────────────────────────────────────

    #[test]
    fn barabasi_albert_directed_node_count() {
        let mut rng = SimRng::from_seed(0);
        let ids = ids(30);
        let net = DiSocialNetwork::barabasi_albert_directed(&ids, 2, &mut rng);
        assert_eq!(net.node_count(), 30);
        assert!(net.is_directed());
    }

    #[test]
    fn barabasi_albert_directed_out_degree_is_m() {
        let m = 3usize;
        let ids = ids(40);
        let net = DiSocialNetwork::barabasi_albert_directed(&ids, m, &mut SimRng::from_seed(11));
        // Non-seed nodes each create exactly `m` out-arcs.
        let seed_n = m + 1;
        for &id in &ids[seed_n..] {
            assert_eq!(
                net.out_degree(id),
                m,
                "non-seed node {id:?} should follow exactly m nodes"
            );
        }
    }

    #[test]
    fn barabasi_albert_directed_in_degree_is_skewed() {
        let ids = ids(200);
        let net = DiSocialNetwork::barabasi_albert_directed(&ids, 2, &mut SimRng::from_seed(13));
        let max_in = ids.iter().map(|&id| net.in_degree(id)).max().unwrap();
        let total_arcs = net.edge_count();
        let mean_in = total_arcs as f64 / ids.len() as f64;
        // A scale-free hub: the most-followed node's in-degree dwarfs the mean.
        assert!(
            max_in as f64 > 4.0 * mean_in,
            "expected a hub (max in-degree {max_in} >> mean {mean_in})"
        );
    }

    #[test]
    fn barabasi_albert_directed_deterministic() {
        let ids = ids(50);
        let n1 = DiSocialNetwork::barabasi_albert_directed(&ids, 2, &mut SimRng::from_seed(21));
        let n2 = DiSocialNetwork::barabasi_albert_directed(&ids, 2, &mut SimRng::from_seed(21));
        let mut e1: Vec<(AgentId, AgentId)> = n1.edges().collect();
        let mut e2: Vec<(AgentId, AgentId)> = n2.edges().collect();
        e1.sort();
        e2.sort();
        assert_eq!(e1, e2);
    }

    // ── Debug derive (#58) ────────────────────────────────────────────────────

    #[test]
    fn debug_derived_on_all_aliases() {
        fn assert_debug<T: std::fmt::Debug>(_: &T) {}

        let mut rng = SimRng::from_seed(0);
        let ids = ids(5);

        let undirected: SocialNetwork = SocialNetwork::erdos_renyi(&ids, 0.5, &mut rng);
        let directed: DiSocialNetwork = DiSocialNetwork::erdos_renyi_directed(&ids, 0.5, &mut rng);
        let mut weighted: WeightedNetwork<f64> = WeightedNetwork::empty();
        let mut di_weighted: DiWeightedNetwork<f64> = DiWeightedNetwork::empty();
        for &id in &ids {
            weighted.add_node(id);
            di_weighted.add_node(id);
        }
        weighted.add_edge_weighted(ids[0], ids[1], 0.5);
        di_weighted.add_edge_weighted(ids[0], ids[1], 0.5);

        assert_debug(&undirected);
        assert_debug(&directed);
        assert_debug(&weighted);
        assert_debug(&di_weighted);

        let _ = format!("{undirected:?}");
        let _ = format!("{directed:?}");
        let _ = format!("{weighted:?}");
        let _ = format!("{di_weighted:?}");
    }

    #[test]
    fn debug_propagates_to_containing_struct() {
        #[derive(Debug)]
        #[allow(dead_code)] // fields are exercised via the derived Debug formatter
        struct Holder {
            net: SocialNetwork,
            tag: &'static str,
        }
        let mut rng = SimRng::from_seed(0);
        let h = Holder {
            net: SocialNetwork::erdos_renyi(&ids(3), 0.5, &mut rng),
            tag: "ok",
        };
        let s = format!("{h:?}");
        assert!(s.contains("Holder"));
        assert!(s.contains("tag: \"ok\""));
    }
}
