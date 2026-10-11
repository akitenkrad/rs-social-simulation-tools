use socsim_core::AgentId;

pub(crate) mod kernel;

use crate::{EncounterRef, ExperimentState, NormalFormGame, StageGame, Topology};

pub trait OutcomeRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NormalFormOutcome;

impl<G: NormalFormGame, X: ExperimentState> OutcomeRule<G, X> for NormalFormOutcome {
    fn name(&self) -> &str {
        "normal_form"
    }

    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        _encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        ctx.game().payoffs_into(actions, payoffs);
        Ok(())
    }
}

pub struct OutcomeContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) game: &'a G,
    pub(crate) x: &'a X,
    pub(crate) held: &'a [G::Action],
    pub(crate) topology: &'a Topology,
}

impl<'a, G: StageGame, X: ExperimentState> OutcomeContext<'a, G, X> {
    pub fn t(&self) -> u64 {
        self.t
    }

    pub fn game(&self) -> &'a G {
        self.game
    }

    pub fn x(&self) -> &'a X {
        self.x
    }

    pub fn held(&self, agent: AgentId) -> Option<&'a G::Action> {
        self.held.get(agent.0 as usize)
    }

    pub fn topology(&self) -> &'a Topology {
        self.topology
    }
}
