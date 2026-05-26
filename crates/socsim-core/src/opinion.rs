//! Capability traits for social-dynamics worlds.
//!
//! These traits let *general* (domain-agnostic) social-dynamics mechanisms — the
//! opinion-dynamics family (Hegselmann–Krause, Deffuant, Social Judgement,
//! Lorenz), the network-contagion family (SI, Granovetter threshold), and the
//! cultural-dissemination family (Axelrod) in `socsim-mechanisms` — operate
//! over any [`WorldState`] that can expose the relevant per-agent state and name
//! each agent's influence set.
//!
//! Each trait is deliberately minimal and dependency-free: a world only has to
//! answer a few "what is agent `i`'s …?" / "whose state does agent `i` see?"
//! questions.  Concrete worlds decide the representation (a `Vec<f64>`, a column
//! in a struct-of-arrays, a `BTreeMap`, etc.) and the topology (complete graph,
//! lattice, network, …).

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
/// The returned set is the pool of agents whose state agent `id` may be
/// influenced by *before* any mechanism-specific filtering (e.g. a
/// bounded-confidence ε test, or a per-edge infection draw) — that filtering
/// happens inside the mechanism.  Whether `id` itself appears in the set is up
/// to the world; mechanisms that need self-inclusion (e.g. Hegselmann–Krause)
/// add it explicitly.
///
/// Complete-graph (non-spatial) worlds return all *other* agents; networked or
/// lattice worlds delegate to their adjacency structure.  This single trait
/// serves every neighbour-based mechanism in the pack (opinion dynamics,
/// contagion, and culture).
pub trait Neighbors: WorldState {
    /// The agents whose state may influence agent `id` this step.
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId>;
}

/// A world whose agents carry a binary *active / informed / infected* flag.
///
/// This is the capability the contagion family (SI, Granovetter threshold)
/// operates on: every agent is in one of two states, and a mechanism flips
/// inactive agents to active according to its rule.  The flag's meaning
/// (informed, infected, mobilised, …) is the world's interpretation; the
/// mechanism only reads it via [`is_active`](BinaryState::is_active) and writes
/// it via [`set_active`](BinaryState::set_active).
pub trait BinaryState: WorldState {
    /// Whether agent `id` is currently active.
    fn is_active(&self, id: AgentId) -> bool;

    /// Set agent `id`'s active flag to `active`.
    fn set_active(&mut self, id: AgentId, active: bool);
}

/// A world whose agents carry a fixed-length categorical *culture vector*.
///
/// This is the capability the Axelrod cultural-dissemination model operates on:
/// each agent holds `n_features` cultural features, each a categorical trait
/// value (`q` possible traits).  Mechanisms read a feature via
/// [`feature`](CultureVectors::feature) and overwrite it via
/// [`set_feature`](CultureVectors::set_feature).  The trait imposes no upper
/// bound on values; the world chooses the trait alphabet size.
pub trait CultureVectors: WorldState {
    /// The number of cultural features `F` each agent carries (the vector
    /// length).  Assumed equal for every agent.
    fn n_features(&self) -> usize;

    /// The value of agent `id`'s feature `f` (`0 ≤ f < n_features`).
    fn feature(&self, id: AgentId, f: usize) -> u32;

    /// Overwrite agent `id`'s feature `f` with `value`.
    fn set_feature(&mut self, id: AgentId, f: usize, value: u32);
}

/// Stable identifier for a group/partition of agents.
pub type GroupId = u64;

/// A world that partitions its agents into named groups.
///
/// This is the capability the group-dynamics family (e.g. group conformity)
/// operates on: every agent belongs to *exactly one* group, and a group's
/// members can be enumerated.  The trait exposes only the *partition structure*
/// — which agent is in which group — and deliberately says nothing about what
/// per-agent quantity a mechanism aggregates over a group.  Mechanisms compute
/// their own aggregates (mean, sum, …) over a *separate* capability such as
/// [`ScalarOpinions`]; for example a within-group averaging mechanism pairs
/// `GroupMembership` with `ScalarOpinions` to nudge each agent toward its
/// group's mean opinion.
///
/// This mirrors how [`Neighbors`] exposes influence-set *structure* without
/// prescribing the dynamics that run over it: the world owns the partition (a
/// team index, a community label, a spatial block, …) and the mechanism owns
/// the update rule.  The three accessors must be mutually consistent —
/// [`group_of`](GroupMembership::group_of) of any member returned by
/// [`group_members`](GroupMembership::group_members) is that group, and every
/// group an agent maps to appears in [`groups`](GroupMembership::groups).
pub trait GroupMembership: WorldState {
    /// The group agent `id` currently belongs to.
    fn group_of(&self, id: AgentId) -> GroupId;

    /// All agents that currently belong to group `g`.
    fn group_members(&self, g: GroupId) -> Vec<AgentId>;

    /// All group identifiers currently present in the world.
    fn groups(&self) -> Vec<GroupId>;
}
