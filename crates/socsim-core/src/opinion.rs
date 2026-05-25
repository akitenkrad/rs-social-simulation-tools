//! Capability traits for scalar opinion-dynamics worlds.
//!
//! These two traits let *general* (domain-agnostic) opinion-dynamics
//! mechanisms — e.g. the bounded-confidence family in `socsim-social-dynamics`
//! (Hegselmann–Krause, Deffuant) — operate over any [`WorldState`] that can
//! expose a scalar opinion per agent and name each agent's influence set.
//!
//! Both traits are deliberately minimal and dependency-free: a world only has
//! to answer "what is agent `i`'s opinion?" and "whose opinions does agent `i`
//! see?".  Concrete worlds decide the representation (a `Vec<f64>`, a column in
//! a struct-of-arrays, etc.) and the topology (complete graph, lattice,
//! network, …).

use crate::{AgentId, WorldState};

/// A world whose agents each carry a single scalar opinion.
///
/// The scalar typically lives in `[-1, 1]` or `[0, 1]`, but the trait imposes
/// no range — bounding (clamping) is the mechanism's or world's responsibility.
/// Mechanisms read opinions via [`opinion`](ScalarOpinions::opinion) and write
/// them back via [`set_opinion`](ScalarOpinions::set_opinion).
pub trait ScalarOpinions: WorldState {
    /// The current scalar opinion of agent `id`.
    fn opinion(&self, id: AgentId) -> f64;

    /// Overwrite the scalar opinion of agent `id` with `value`.
    fn set_opinion(&mut self, id: AgentId, value: f64);
}

/// A world that can name the *influence set* (neighbours) of an agent.
///
/// The returned set is the pool of agents whose opinions agent `id` may be
/// influenced by *before* any bounded-confidence (ε) filtering — that filtering
/// happens inside the mechanism.  Whether `id` itself appears in the set is up
/// to the world; mechanisms that need self-inclusion (e.g. Hegselmann–Krause)
/// add it explicitly.
///
/// Complete-graph (non-spatial) worlds return all *other* agents; networked or
/// lattice worlds delegate to their adjacency structure.
pub trait OpinionNeighbors: WorldState {
    /// The agents whose opinions may influence agent `id` this step.
    fn opinion_neighbors(&self, id: AgentId) -> Vec<AgentId>;
}
