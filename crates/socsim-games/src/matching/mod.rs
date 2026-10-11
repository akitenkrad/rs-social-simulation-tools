mod fixed;
#[cfg(feature = "grid")]
mod grid_neighborhood;
#[cfg(feature = "net")]
mod network_edges;
mod random_groups;
mod random_pairs;
mod round_robin;
mod whole_population;

pub use fixed::FixedPlayers;
#[cfg(feature = "grid")]
pub use grid_neighborhood::GridNeighborhood;
#[cfg(feature = "net")]
pub use network_edges::NetworkEdges;
pub use random_groups::RandomGroups;
pub use random_pairs::RandomPairs;
pub use round_robin::RoundRobin;
pub use whole_population::WholePopulation;

use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SimRng};

use crate::{
    world::InternalStepHistory, ArenaWriter, DecisionScope, ExperimentState, GameError,
    HistoryView, StageGame, StepRecord, Topology,
};

pub trait Matching<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError>;
    /// Returns whether this matching can seat a game with `n_roles` roles.
    ///
    /// Checked at assembly before [`check`](Self::check); `false` yields
    /// [`GameError::UnsupportedRoles`]. The default accepts only two-role games.
    fn supports_roles(&self, n_roles: usize) -> bool {
        n_roles == 2
    }
    /// Returns the range of payoff-credited encounters per agent in one step.
    ///
    /// Population dynamics use it to bound an agent's per-step payoff. `None`
    /// (the default) means the count is unknown: dynamics then cannot bound summed
    /// payoffs, so a check such as the upper bound on the `beta` of
    /// [`ProportionalImitation`](crate::ProportionalImitation) fails under
    /// [`PayoffAggregation::Sum`](crate::PayoffAggregation::Sum), and an agent is
    /// assumed it may be left idle (payoff 0) under
    /// [`PayoffAggregation::Mean`](crate::PayoffAggregation::Mean).
    fn encounter_bounds(&self, _ctx: &MatchContext<'_, G, X>) -> Option<EncounterBounds> {
        None
    }
    /// Returns whether one agent may occupy different roles within one step.
    ///
    /// With `false` (the default) and [`DecisionScope::PerStep`], an agent seated in
    /// two different roles in the same step is an error. With `true`, the agent
    /// makes one decision per step, the action is checked against every role it
    /// occupies, and its opponents are the union over all of its encounters.
    fn allows_multi_role_per_step(&self) -> bool {
        false
    }
    /// Returns a pattern that reproduces this matching's encounters without an arena.
    ///
    /// When `Some`, and the other conditions of
    /// [`PlayOptions::materialize`](crate::PlayOptions::materialize) hold, the play
    /// step traverses the pattern instead of calling
    /// [`encounters_into`](Self::encounters_into). The pattern must yield exactly
    /// the same encounters, seats, and credit in the same order. The default is
    /// `None`.
    fn implicit(&self) -> Option<ImplicitPattern> {
        None
    }
    fn init_state(&self, _ctx: &MatchContext<'_, G, X>) -> MatchingState {
        MatchingState::None
    }
    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()>;
}

/// A matching pattern that can be traversed without constructing a play arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImplicitPattern {
    /// Every grid cell plays each of its neighbors, as
    /// `GridNeighborhood` (feature `grid`) does.
    GridCenter {
        /// Whether each cell also plays itself.
        include_self: bool,
    },
    /// One encounter per network edge, as `NetworkEdges` (feature `net`) does.
    NetworkEdges,
    /// One encounter containing every agent, as
    /// [`WholePopulation`] does.
    WholePopulation,
}

/// Inclusive range of payoff-credited encounters an agent has in one step.
///
/// Counts only encounters whose [`Credit`](crate::Credit) pays the agent. A
/// `min` of 0 means some agent may sit out a step and receive payoff 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncounterBounds {
    /// Fewest credited encounters of any agent.
    pub min: u32,
    /// Most credited encounters of any agent.
    pub max: u32,
}

pub struct MatchContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) decision_scope: DecisionScope,
    pub(crate) ids: &'a [AgentId],
    pub(crate) game: &'a G,
    pub(crate) topology: &'a Topology,
    pub(crate) held: &'a [G::Action],
    pub(crate) history: &'a InternalStepHistory<G::Action>,
    pub(crate) x: &'a X,
}

impl<'a, G: StageGame, X: ExperimentState> MatchContext<'a, G, X> {
    pub fn t(&self) -> u64 {
        self.t
    }

    pub fn decision_scope(&self) -> DecisionScope {
        self.decision_scope
    }

    pub fn ids(&self) -> &'a [AgentId] {
        self.ids
    }

    pub fn game(&self) -> &'a G {
        self.game
    }

    pub fn topology(&self) -> &'a Topology {
        self.topology
    }

    pub fn held(&self, agent: AgentId) -> Option<&'a G::Action> {
        self.held.get(agent.0 as usize)
    }

    pub fn history(&self, agent: AgentId) -> Option<HistoryView<'a, StepRecord<G::Action>>> {
        self.history.view(agent)
    }

    pub fn x(&self) -> &'a X {
        self.x
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MatchingState {
    None,
    RoundRobin { cursor: u64 },
}
