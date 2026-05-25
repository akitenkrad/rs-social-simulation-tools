//! Axelrod (1997) cultural-dissemination mechanism.
//!
//! Operates over any world implementing
//! [`CultureVectors`](socsim_core::CultureVectors) +
//! [`Neighbors`](socsim_core::Neighbors).  Each agent holds an `F`-feature
//! categorical culture vector; interaction makes culturally similar neighbours
//! converge while leaving dissimilar ones apart, producing stable cultural
//! regions.
//!
//! [`AxelrodMechanism`] runs `events_per_step` micro-events per step; each event
//! reproduces the `wang2025` reference's `classical_event`:
//!
//! 1. draw a site `s` uniformly and a random neighbour `nb`;
//! 2. compute the similarity `sim = (#matching features) / F`;
//! 3. if `0 < sim < 1`, with probability `sim` copy one randomly-chosen
//!    *differing* feature from `nb` into `s`.
//!
//! The free helper [`is_absorbing`] tests the absorbing state (every adjacent
//! pair has `sim ∈ {0, 1}`), i.e. no further interaction can change any agent.

use socsim_core::{AgentId, CultureVectors, Mechanism, Neighbors, Phase, Result, StepContext};

use rand::Rng;

/// Fraction of matching features between two agents' culture vectors, in
/// `[0, 1]`.  `F` is read from [`CultureVectors::n_features`].
fn similarity<W: CultureVectors>(world: &W, a: AgentId, b: AgentId) -> f64 {
    let f = world.n_features();
    if f == 0 {
        return 1.0;
    }
    let matching = (0..f)
        .filter(|&k| world.feature(a, k) == world.feature(b, k))
        .count();
    matching as f64 / f as f64
}

/// Run one classical Axelrod event against `world`, drawing the site and
/// neighbour via `rng`.  Returns whether a feature was copied.
///
/// Ported verbatim from the `wang2025` reference's `classical_event`.  The site
/// is drawn uniformly from the agent roster; the neighbour uniformly from
/// `neighbors_of(site)`.
pub fn axelrod_event<W: CultureVectors + Neighbors>(
    world: &mut W,
    rng: &mut socsim_core::SimRng,
) -> bool {
    let ids = world.agent_ids();
    if ids.is_empty() {
        return false;
    }
    let s = ids[rng.gen_range(0..ids.len())];
    let neighbors = world.neighbors_of(s);
    if neighbors.is_empty() {
        return false;
    }
    let nb = neighbors[rng.gen_range(0..neighbors.len())];
    if s == nb {
        return false;
    }

    let sim = similarity(world, s, nb);
    // sim == 0: nothing in common. sim == 1: identical, nothing to copy.
    if sim <= 0.0 || sim >= 1.0 {
        return false;
    }
    // Interact with probability sim.
    if rng.gen::<f64>() >= sim {
        return false;
    }

    let f = world.n_features();
    let diffs: Vec<usize> = (0..f)
        .filter(|&k| world.feature(s, k) != world.feature(nb, k))
        .collect();
    debug_assert!(!diffs.is_empty()); // sim < 1 ⇒ at least one diff
    let feat = diffs[rng.gen_range(0..diffs.len())];
    let new_val = world.feature(nb, feat);
    world.set_feature(s, feat, new_val);
    true
}

/// Whether `world` has reached the Axelrod absorbing state: every adjacent pair
/// of agents has similarity ∈ {0, 1} (identical, or sharing nothing), so no
/// further event can change any culture.
///
/// Cheap for small/sparse topologies; scans every agent's neighbour set once.
pub fn is_absorbing<W: CultureVectors + Neighbors>(world: &W) -> bool {
    for id in world.agent_ids() {
        for nb in world.neighbors_of(id) {
            if nb == id {
                continue;
            }
            let sim = similarity(world, id, nb);
            if sim > 0.0 && sim < 1.0 {
                return false;
            }
        }
    }
    true
}

/// Classical (deterministic-core) Axelrod cultural-dissemination mechanism.
///
/// Runs `events_per_step` micro-events per engine tick via [`axelrod_event`].
/// No convergence/stop logic — pair with a driver- or world-side absorbing-state
/// check (see [`is_absorbing`]) if a stop is desired.
#[derive(Clone, Copy, Debug)]
pub struct AxelrodMechanism {
    /// Micro-events per engine tick.
    pub events_per_step: usize,
}

impl AxelrodMechanism {
    /// Create an Axelrod mechanism running `events_per_step` micro-events per
    /// step.
    pub fn new(events_per_step: usize) -> Self {
        Self { events_per_step }
    }
}

impl Default for AxelrodMechanism {
    /// One micro-event per step.
    fn default() -> Self {
        Self { events_per_step: 1 }
    }
}

impl<W: CultureVectors + Neighbors> Mechanism<W> for AxelrodMechanism {
    fn name(&self) -> &str {
        "axelrod"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        for _ in 0..self.events_per_step {
            axelrod_event(ctx.world, ctx.rng);
        }
        Ok(())
    }
}
