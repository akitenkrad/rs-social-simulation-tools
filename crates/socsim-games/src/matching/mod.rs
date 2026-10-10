mod fixed;
mod round_robin;

pub use fixed::FixedPlayers;
pub use round_robin::RoundRobin;

use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SimRng};

use crate::{
    world::InternalStepHistory, ArenaWriter, DecisionScope, ExperimentState, GameError,
    HistoryView, StageGame, StepRecord, Topology,
};

pub trait Matching<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError>;
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
