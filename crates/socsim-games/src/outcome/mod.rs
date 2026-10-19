//! Payoff rules applied after every encounter's actions are known.
//!
//! See the [outcome-rules guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#outcome-rules).

use socsim_core::AgentId;

pub(crate) mod kernel;

use crate::{EncounterRef, ExperimentState, NormalFormGame, StageGame, Topology};

/// Computes one payoff per role for a completed encounter.
///
/// Outcome authors implement this trait; the play mechanism calls it after all
/// seats have supplied valid actions. Implementations must overwrite every
/// entry in `payoffs`, preserve role order, and avoid retaining references to
/// the transient context or encounter.
///
/// See the [outcome-rules guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#outcome-rules).
pub trait OutcomeRule<G: StageGame, X: ExperimentState>: Send + Sync {
    /// Returns the stable name used in mechanism diagnostics.
    fn name(&self) -> &str;
    /// Writes role-indexed payoffs for one action profile.
    ///
    /// `actions`, `payoffs`, and `encounter.seats` each have
    /// `ctx.game().n_roles()` entries.
    ///
    /// # Errors
    ///
    /// Returns a socsim error when the rule cannot evaluate the encounter.
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
/// Outcome rule that delegates directly to [`NormalFormGame::payoffs_into`].
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

/// Read-only world information available while computing encounter payoffs.
pub struct OutcomeContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) game: &'a G,
    pub(crate) x: &'a X,
    pub(crate) held: &'a [G::Action],
    pub(crate) topology: &'a Topology,
}

impl<'a, G: StageGame, X: ExperimentState> OutcomeContext<'a, G, X> {
    /// Returns the current simulation step.
    ///
    /// The first step is `1`: the engine advances the clock before mechanisms run.
    pub fn t(&self) -> u64 {
        self.t
    }

    /// Returns the stage game whose encounter is being evaluated.
    pub fn game(&self) -> &'a G {
        self.game
    }

    /// Returns experiment-specific state shared with the world.
    pub fn x(&self) -> &'a X {
        self.x
    }

    /// Returns an agent's currently held action, or `None` for an unknown ID.
    pub fn held(&self, agent: AgentId) -> Option<&'a G::Action> {
        self.held.get(agent.0 as usize)
    }

    /// Returns the world's optional spatial or network topology.
    pub fn topology(&self) -> &'a Topology {
        self.topology
    }
}
