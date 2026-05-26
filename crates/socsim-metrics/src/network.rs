//! Network-structure and cascade metrics (feature `network`).
//!
//! Read-only summaries over a [`SocialNetwork`].  Several functions delegate to
//! `socsim-net`'s own analysis methods (degree distribution, clustering,
//! components) and re-express them as the dispersion-friendly shapes the rest
//! of this crate uses; the cascade helpers add reach/active counting over a
//! caller-supplied activity predicate.
//!
//! `SocialNetwork` exposes no public node-id iterator, so the node set is
//! recovered deterministically from
//! [`SocialNetwork::component_membership`](socsim_net::SocialNetwork::component_membership)
//! (whose keys are every node), sorted by `AgentId`.

use socsim_core::AgentId;
use socsim_net::SocialNetwork;

/// All node ids in the network, sorted by `AgentId` (deterministic).
fn node_ids(net: &SocialNetwork) -> Vec<AgentId> {
    let mut ids: Vec<AgentId> = net.component_membership().into_keys().collect();
    ids.sort();
    ids
}

/// Mean degree `(Σ degree) / N = 2·|E| / N` for an undirected network.
///
/// Empty network → `0.0`.
pub fn mean_degree(net: &SocialNetwork) -> f64 {
    let n = net.node_count();
    if n == 0 {
        return 0.0;
    }
    // Each undirected edge contributes to two endpoints' degrees.
    (2.0 * net.edge_count() as f64) / n as f64
}

/// Largest degree of any node (`0` for an empty / edgeless network).
pub fn max_degree(net: &SocialNetwork) -> usize {
    // degree_distribution has length max_degree + 1, or is empty for no nodes.
    net.degree_distribution().len().saturating_sub(1)
}

/// Degree distribution `hist[d]` = number of nodes with degree exactly `d`.
///
/// Length `max_degree + 1` (empty for a node-less network).  Thin wrapper over
/// [`SocialNetwork::degree_distribution`](socsim_net::SocialNetwork::degree_distribution).
pub fn degree_distribution(net: &SocialNetwork) -> Vec<usize> {
    net.degree_distribution()
}

/// **Average local clustering coefficient** over all nodes with `≥ 2`
/// neighbours.
///
/// This is the *average local* clustering coefficient (mean over nodes of the
/// fraction of each node's neighbour pairs that are connected), **not** the
/// global transitivity ratio.  Delegates to
/// [`SocialNetwork::average_clustering_coefficient`](socsim_net::SocialNetwork::average_clustering_coefficient);
/// returns `0.0` when no node has two neighbours.
pub fn global_clustering_coefficient(net: &SocialNetwork) -> f64 {
    net.average_clustering_coefficient().unwrap_or(0.0)
}

/// Sizes of all connected components, sorted descending.
///
/// Computed from
/// [`SocialNetwork::component_membership`](socsim_net::SocialNetwork::component_membership)
/// (weak connectivity).  Empty network → empty `Vec`.
pub fn component_sizes(net: &SocialNetwork) -> Vec<usize> {
    let membership = net.component_membership();
    let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &label in membership.values() {
        *counts.entry(label).or_insert(0) += 1;
    }
    let mut sizes: Vec<usize> = counts.into_values().collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes
}

/// Fraction of nodes in the largest connected component, `largest / N`.
///
/// Empty network → `0.0`.  `1.0` for a fully connected network.
pub fn largest_component_fraction(net: &SocialNetwork) -> f64 {
    let n = net.node_count();
    if n == 0 {
        return 0.0;
    }
    net.largest_component_size() as f64 / n as f64
}

/// Number of nodes for which `is_active` returns `true`.
///
/// This is the simplest "how big did the cascade get" measure: the **count of
/// active nodes** in the network, evaluated by calling `is_active` on every
/// node id.  It does **not** perform any graph traversal — pass an activity
/// predicate that already reflects the final infected/informed/adopted state.
/// For BFS reachability from seeds use [`reach_fraction`].
pub fn cascade_size<F>(net: &SocialNetwork, is_active: F) -> usize
where
    F: Fn(AgentId) -> bool,
{
    node_ids(net).into_iter().filter(|&id| is_active(id)).count()
}

/// Fraction of all nodes that are **reachable from any active seed** via BFS,
/// `|reached| / N`.
///
/// Starting from the set of nodes where `is_active` is `true`, this does a BFS
/// over the network and counts every node reachable from a seed (the seeds
/// themselves are reachable in zero hops and are included).  It answers "if the
/// active set is the seed of a diffusion that crosses every edge, what fraction
/// of the population could it eventually touch?" — i.e. the size of the union
/// of the seeds' connected components, as a fraction.
///
/// Empty network → `0.0`.  No active seeds → `0.0`.
pub fn reach_fraction<F>(net: &SocialNetwork, is_active: F) -> f64
where
    F: Fn(AgentId) -> bool,
{
    let ids = node_ids(net);
    let n = ids.len();
    if n == 0 {
        return 0.0;
    }
    let mut visited: std::collections::HashSet<AgentId> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<AgentId> = std::collections::VecDeque::new();
    for &id in &ids {
        if is_active(id) && visited.insert(id) {
            queue.push_back(id);
        }
    }
    while let Some(cur) = queue.pop_front() {
        for nb in net.neighbors(cur) {
            if visited.insert(nb) {
                queue.push_back(nb);
            }
        }
    }
    visited.len() as f64 / n as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny hand-made network:
    ///   triangle 0-1-2 (all connected), plus an isolated edge 3-4, plus a lone node 5.
    fn fixture() -> SocialNetwork {
        let mut net = SocialNetwork::empty();
        for i in 0..6 {
            net.add_node(AgentId(i));
        }
        net.add_edge(AgentId(0), AgentId(1));
        net.add_edge(AgentId(1), AgentId(2));
        net.add_edge(AgentId(0), AgentId(2));
        net.add_edge(AgentId(3), AgentId(4));
        net
    }

    #[test]
    fn degree_stats() {
        let net = fixture();
        // Degrees: 0->2, 1->2, 2->2, 3->1, 4->1, 5->0.  Sum = 8, N = 6.
        assert!((mean_degree(&net) - 8.0 / 6.0).abs() < 1e-12);
        assert_eq!(max_degree(&net), 2);
        // hist: [1 (deg0), 2 (deg1), 3 (deg2)]
        assert_eq!(degree_distribution(&net), vec![1, 2, 3]);
    }

    #[test]
    fn clustering_of_triangle() {
        let net = fixture();
        // Only nodes 0,1,2 have >=2 neighbours; each forms the full triangle → 1.0.
        assert!((global_clustering_coefficient(&net) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn components() {
        let net = fixture();
        // Components: {0,1,2}=3, {3,4}=2, {5}=1.
        assert_eq!(component_sizes(&net), vec![3, 2, 1]);
        assert!((largest_component_fraction(&net) - 3.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn cascade_size_counts_active() {
        let net = fixture();
        // Active = even ids 0,2,4 → 3 active.
        assert_eq!(cascade_size(&net, |id| id.0 % 2 == 0), 3);
        assert_eq!(cascade_size(&net, |_| false), 0);
    }

    #[test]
    fn reach_fraction_bfs_from_seed() {
        let net = fixture();
        // Seed just node 0 → reaches the whole triangle {0,1,2} = 3 of 6.
        assert!((reach_fraction(&net, |id| id.0 == 0) - 3.0 / 6.0).abs() < 1e-12);
        // Seed node 3 → reaches {3,4} = 2 of 6.
        assert!((reach_fraction(&net, |id| id.0 == 3) - 2.0 / 6.0).abs() < 1e-12);
        // No seeds → 0.
        assert!((reach_fraction(&net, |_| false)).abs() < 1e-12);
        // Seed the lone node 5 → only itself = 1 of 6.
        assert!((reach_fraction(&net, |id| id.0 == 5) - 1.0 / 6.0).abs() < 1e-12);
    }
}
