use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

use crate::{
    world::{InternalPairHistory, InternalStepHistory},
    Credit, EncounterId, EncounterRef, ExperimentState, HistoryView, PairView, PeerHistory,
    PlayArena, StageGame, StepRecord, Topology,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DecisionScope {
    #[default]
    PerStep,
    PerAgentRole,
    PerEncounter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecisionUnit {
    Step,
    AgentRole(usize),
    Encounter(EncounterId),
}

pub struct DecisionContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) me: AgentId,
    pub(crate) unit: DecisionUnit,
    pub(crate) role: Option<usize>,
    pub(crate) encounter: Option<EncounterRef<'a>>,
    pub(crate) opponents: Vec<AgentId>,
    pub(crate) ids: &'a [AgentId],
    pub(crate) game: &'a G,
    pub(crate) held: &'a [G::Action],
    pub(crate) history: &'a InternalStepHistory<G::Action>,
    pub(crate) pair_history: Option<&'a InternalPairHistory<G::Action>>,
    pub(crate) x: &'a X,
}

impl<'a, G: StageGame, X: ExperimentState> DecisionContext<'a, G, X> {
    pub fn t(&self) -> u64 {
        self.t
    }

    pub fn me(&self) -> AgentId {
        self.me
    }

    pub fn unit(&self) -> DecisionUnit {
        self.unit
    }

    pub fn scope(&self) -> DecisionScope {
        match self.unit {
            DecisionUnit::Step => DecisionScope::PerStep,
            DecisionUnit::AgentRole(_) => DecisionScope::PerAgentRole,
            DecisionUnit::Encounter(_) => DecisionScope::PerEncounter,
        }
    }

    pub fn role(&self) -> Option<usize> {
        self.role
    }

    pub fn encounter(&self) -> Option<EncounterRef<'a>> {
        self.encounter
    }

    pub fn opponents(&self) -> &[AgentId] {
        &self.opponents
    }

    pub fn my_history(&self) -> HistoryView<'a, StepRecord<G::Action>> {
        self.history
            .view(self.me)
            .expect("DecisionContext is constructed only for a known agent")
    }

    pub fn opponent_history(&self, opponent: AgentId) -> Option<PeerHistory<'a, G::Action>> {
        self.opponents
            .contains(&opponent)
            .then(|| self.history.view(opponent))
            .flatten()
            .map(PeerHistory::new)
    }

    /// Returns pair-specific history for an opponent in this decision unit.
    ///
    /// `None` means that pair history was not enabled for this world, or that
    /// `other` is not among [`Self::opponents`]. If pair history is enabled and
    /// `other` is a current opponent, a pair that has not played before returns
    /// an empty view.
    pub fn pair_history(&self, other: AgentId) -> Option<PairView<'a, G::Action>> {
        self.opponents
            .contains(&other)
            .then(|| {
                self.pair_history
                    .map(|history| history.view(self.me, other))
            })
            .flatten()
    }

    pub fn game(&self) -> &'a G {
        self.game
    }

    pub fn view(&self) -> X::View<'_> {
        self.x.view_for(self.me)
    }

    /// Returns unrestricted world data.
    ///
    /// Calling this removes the information-structure guarantee of the default
    /// view. During Decision, `held` may also depend on activation order because
    /// agents activated earlier have already updated their held action.
    pub fn omniscient(&self) -> OmniscientContext<'a, G, X> {
        OmniscientContext {
            ids: self.ids,
            held: self.held,
            history: self.history,
            pair_history: self.pair_history,
            x: self.x,
            marker: std::marker::PhantomData,
        }
    }
}

pub struct LearnContext<'a, G: StageGame, X: ExperimentState> {
    pub(crate) t: u64,
    pub(crate) me: AgentId,
    pub(crate) scope: DecisionScope,
    pub(crate) record: &'a StepRecord<G::Action>,
    pub(crate) ids: &'a [AgentId],
    pub(crate) game: &'a G,
    pub(crate) held: &'a [G::Action],
    pub(crate) history: &'a InternalStepHistory<G::Action>,
    pub(crate) plays: &'a PlayArena<G::Action>,
    pub(crate) x: &'a X,
}

impl<'a, G: StageGame, X: ExperimentState> LearnContext<'a, G, X> {
    pub fn t(&self) -> u64 {
        self.t
    }

    pub fn me(&self) -> AgentId {
        self.me
    }

    pub fn scope(&self) -> DecisionScope {
        self.scope
    }

    pub fn record(&self) -> &'a StepRecord<G::Action> {
        self.record
    }

    pub fn my_history(&self) -> HistoryView<'a, StepRecord<G::Action>> {
        self.history
            .view(self.me)
            .expect("LearnContext is constructed only for a known agent")
    }

    /// Returns this agent's encounter outcomes in ascending [`EncounterId`]
    /// order.
    ///
    /// Learn の時点では `game_play` が全席の埋まりを検査済みなので，
    /// [`EncounterOutcome::actions`] はすべて `Some` です．
    pub fn outcomes(&self) -> impl Iterator<Item = EncounterOutcome<'a, G::Action>> + '_ {
        (0..self.plays.n_encounters()).filter_map(move |encounter_index| {
            let start = encounter_index * self.plays.n_roles;
            let end = start + self.plays.n_roles;
            let seats = &self.plays.seats[start..end];
            let role = seats.iter().position(|&agent| agent == self.me)?;
            Some(EncounterOutcome {
                id: EncounterId(encounter_index as u32),
                role,
                seats,
                actions: &self.plays.actions[start..end],
                payoffs: &self.plays.payoffs[start..end],
                credit: self.plays.credit[encounter_index],
            })
        })
    }

    pub fn game(&self) -> &'a G {
        self.game
    }

    pub fn view(&self) -> X::View<'_> {
        self.x.view_for(self.me)
    }

    /// Returns unrestricted world data. This removes the default information
    /// structure guarantee; see [`DecisionContext::omniscient`].
    pub fn omniscient(&self) -> OmniscientContext<'a, G, X> {
        OmniscientContext {
            ids: self.ids,
            held: self.held,
            history: self.history,
            pair_history: None,
            x: self.x,
            marker: std::marker::PhantomData,
        }
    }
}

pub struct OmniscientContext<'a, G: StageGame, X: ExperimentState> {
    ids: &'a [AgentId],
    held: &'a [G::Action],
    history: &'a InternalStepHistory<G::Action>,
    pair_history: Option<&'a InternalPairHistory<G::Action>>,
    x: &'a X,
    marker: std::marker::PhantomData<G>,
}

impl<'a, G: StageGame, X: ExperimentState> OmniscientContext<'a, G, X> {
    pub fn ids(&self) -> &'a [AgentId] {
        self.ids
    }

    pub fn held(&self, agent: AgentId) -> Option<&'a G::Action> {
        self.held.get(agent.0 as usize)
    }

    pub fn history(&self, agent: AgentId) -> Option<HistoryView<'a, StepRecord<G::Action>>> {
        self.history.view(agent)
    }

    /// Returns the history of `(a, b)`, oriented as `a`'s actions followed by
    /// `b`'s actions.
    ///
    /// `None` means pair history was not enabled, at least one agent is not in
    /// this world, or the context that created this view does not expose pair
    /// history. A known pair that has not played returns an empty view.
    pub fn pair_history(&self, a: AgentId, b: AgentId) -> Option<PairView<'a, G::Action>> {
        (self.ids.contains(&a) && self.ids.contains(&b))
            .then(|| self.pair_history.map(|history| history.view(a, b)))
            .flatten()
    }

    pub fn x(&self) -> &'a X {
        self.x
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EncounterOutcome<'a, A> {
    pub id: EncounterId,
    pub role: usize,
    pub seats: &'a [AgentId],
    /// 席ごとの行動．Learn の時点では `game_play` が全席の埋まりを
    /// 検査済みなので，すべて `Some` です．
    pub actions: &'a [Option<A>],
    pub payoffs: &'a [f64],
    pub credit: Credit,
}

#[allow(dead_code)]
fn _topology_surface(_: &Topology) {}
