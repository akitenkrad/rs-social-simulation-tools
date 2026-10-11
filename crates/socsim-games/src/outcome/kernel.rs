use std::any::Any;

use socsim_core::{AgentId, SocsimError};

use crate::{
    AnonymousGame, Credit, EncounterId, EncounterRef, ExperimentState, ImplicitPattern,
    InternalPlayedActionCounts, OutcomeContext, OutcomeRule, PlayArena, StageGame, Topology,
};

pub(crate) struct PayoffAccum<'a> {
    payoff: &'a mut [f64],
    n_played: &'a mut [u32],
    played_counts: Option<&'a mut InternalPlayedActionCounts>,
}

impl<'a> PayoffAccum<'a> {
    pub(crate) fn new(
        payoff: &'a mut [f64],
        n_played: &'a mut [u32],
        played_counts: Option<&'a mut InternalPlayedActionCounts>,
    ) -> Self {
        Self {
            payoff,
            n_played,
            played_counts,
        }
    }

    fn apply<A: 'static>(
        &mut self,
        seats: &[AgentId],
        credit: Credit,
        actions: &[A],
        payoffs: &[f64],
    ) -> socsim_core::Result<()> {
        if let Some(counts) = &mut self.played_counts {
            for (role, action) in actions.iter().enumerate() {
                if let Some(action) = (action as &dyn Any).downcast_ref::<usize>() {
                    counts.increment(role, *action)?;
                }
            }
        }
        let credited_roles = match credit {
            Credit::All => 0..seats.len(),
            Credit::Center => 0..1,
        };
        for role in credited_roles {
            let index = usize::try_from(seats[role].0)
                .map_err(|_| SocsimError::Mechanism("agent index exceeds usize".to_owned()))?;
            let total = self.payoff.get_mut(index).ok_or_else(|| {
                SocsimError::Mechanism(format!("unknown agent {}", seats[role].0))
            })?;
            *total += payoffs[role];
            let count = self.n_played.get_mut(index).ok_or_else(|| {
                SocsimError::Mechanism(format!("unknown agent {}", seats[role].0))
            })?;
            *count = count.checked_add(1).ok_or_else(|| {
                SocsimError::Mechanism(format!("play count overflowed for agent {}", seats[role].0))
            })?;
        }
        Ok(())
    }
}

pub(crate) fn play_pairwise<G, X, O>(
    context: &OutcomeContext<'_, G, X>,
    outcome: &O,
    arena: &mut PlayArena<G::Action>,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    // Scratch buffers are reused across encounters so that the loop does not
    // allocate per encounter.
    let mut actions = Vec::with_capacity(arena.n_roles);
    let mut payoffs = vec![0.0; arena.n_roles];
    for encounter_index in 0..arena.n_encounters() {
        let start = encounter_index * arena.n_roles;
        let end = start + arena.n_roles;
        fill_actions(&mut actions, &arena.actions[start..end])?;
        let credit = arena.credit[encounter_index];
        let seats = &arena.seats[start..end];
        let encounter = EncounterRef {
            id: EncounterId(encounter_index as u32),
            seats,
            credit,
        };
        payoffs.fill(0.0);
        outcome.outcome(context, encounter, &actions, &mut payoffs)?;
        arena.payoffs[start..end].copy_from_slice(&payoffs);
        acc.apply(seats, credit, &actions, &payoffs)?;
    }
    Ok(())
}

/// Copies one encounter's filled action seats into `out`, reusing its buffer.
pub(crate) fn fill_actions<A: Clone>(
    out: &mut Vec<A>,
    seats: &[Option<A>],
) -> socsim_core::Result<()> {
    out.clear();
    for action in seats {
        out.push(
            action
                .clone()
                .ok_or_else(|| SocsimError::Mechanism("unfilled action seat".to_owned()))?,
        );
    }
    Ok(())
}

/// Reusable buffers of the counting kernel.
struct CountingScratch {
    counts: Vec<usize>,
    by_action: Vec<f64>,
    payoffs: Vec<f64>,
}

impl CountingScratch {
    fn new(n_actions: usize) -> Self {
        Self {
            counts: vec![0; n_actions],
            by_action: vec![0.0; n_actions],
            payoffs: Vec::new(),
        }
    }

    /// Fills `self.payoffs` with the payoff of every seat of one encounter.
    fn compute<G: AnonymousGame>(
        &mut self,
        game: &G,
        actions: &[usize],
    ) -> socsim_core::Result<()> {
        let n_actions = game.n_actions(0);
        self.counts.clear();
        self.counts.resize(n_actions, 0);
        for &action in actions {
            let count = self.counts.get_mut(action).ok_or_else(|| {
                SocsimError::Mechanism(format!("counting kernel found invalid action {action}"))
            })?;
            *count += 1;
        }
        self.by_action.clear();
        self.by_action.resize(n_actions, 0.0);
        for action in 0..n_actions {
            if self.counts[action] == 0 {
                continue;
            }
            self.counts[action] -= 1;
            self.by_action[action] = game.payoff_by_counts(action, &self.counts);
            self.counts[action] += 1;
        }
        self.payoffs.clear();
        self.payoffs
            .extend(actions.iter().map(|&action| self.by_action[action]));
        Ok(())
    }
}

pub(crate) fn play_counting<G: AnonymousGame>(
    game: &G,
    arena: &mut PlayArena<usize>,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()> {
    let mut scratch = CountingScratch::new(game.n_actions(0));
    let mut actions = Vec::with_capacity(arena.n_roles);
    for encounter_index in 0..arena.n_encounters() {
        let start = encounter_index * arena.n_roles;
        let end = start + arena.n_roles;
        fill_actions(&mut actions, &arena.actions[start..end])?;
        scratch.compute(game, &actions)?;
        arena.payoffs[start..end].copy_from_slice(&scratch.payoffs);
        acc.apply(
            &arena.seats[start..end],
            arena.credit[encounter_index],
            &actions,
            &scratch.payoffs,
        )?;
    }
    Ok(())
}

pub(crate) fn for_each_implicit<A, F>(
    pattern: ImplicitPattern,
    _topology: &Topology,
    ids: &[AgentId],
    held: &[A],
    mut visit: F,
) -> socsim_core::Result<()>
where
    A: Clone,
    F: FnMut(u32, &[AgentId], Credit, &[A]) -> socsim_core::Result<()>,
{
    let mut id = 0_u32;
    let mut actions: Vec<A> = Vec::new();
    let mut emit = |seats: &[AgentId], credit: Credit| -> socsim_core::Result<()> {
        actions.clear();
        for agent in seats {
            actions.push(
                held.get(agent.0 as usize)
                    .cloned()
                    .ok_or_else(|| SocsimError::Mechanism(format!("unknown agent {}", agent.0)))?,
            );
        }
        visit(id, seats, credit, &actions)?;
        id = id
            .checked_add(1)
            .ok_or_else(|| SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned()))?;
        Ok(())
    };
    match pattern {
        #[cfg(feature = "grid")]
        ImplicitPattern::GridCenter { include_self } => {
            let Topology::Grid(grid) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit grid matching requires a grid topology".to_owned(),
                ));
            };
            for index in 0..grid.adjacency().len() {
                let center = AgentId(index as u64);
                for &neighbor in grid.adjacency().neighbors(index) {
                    emit(&[center, AgentId(neighbor as u64)], Credit::Center)?;
                }
                if include_self {
                    emit(&[center, center], Credit::Center)?;
                }
            }
        }
        #[cfg(not(feature = "grid"))]
        ImplicitPattern::GridCenter { .. } => {
            return Err(SocsimError::Mechanism(
                "implicit grid matching requires the grid feature".to_owned(),
            ));
        }
        #[cfg(feature = "net")]
        ImplicitPattern::NetworkEdges => {
            let Topology::Network(network) = _topology else {
                return Err(SocsimError::Mechanism(
                    "implicit network matching requires a network topology".to_owned(),
                ));
            };
            for &(source, target) in network.edges() {
                emit(&[source, target], Credit::All)?;
            }
        }
        #[cfg(not(feature = "net"))]
        ImplicitPattern::NetworkEdges => {
            return Err(SocsimError::Mechanism(
                "implicit network matching requires the net feature".to_owned(),
            ));
        }
        ImplicitPattern::WholePopulation => {
            let mut seats = ids.to_vec();
            seats.sort_unstable();
            emit(&seats, Credit::All)?;
        }
    }
    Ok(())
}

pub(crate) fn play_implicit_pairwise<G, X, O>(
    pattern: ImplicitPattern,
    context: &OutcomeContext<'_, G, X>,
    ids: &[AgentId],
    outcome: &O,
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()>
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
    let mut payoffs = Vec::new();
    for_each_implicit(
        pattern,
        context.topology(),
        ids,
        context.held,
        |id, seats, credit, actions| {
            let encounter = EncounterRef {
                id: EncounterId(id),
                seats,
                credit,
            };
            payoffs.clear();
            payoffs.resize(seats.len(), 0.0);
            outcome.outcome(context, encounter, actions, &mut payoffs)?;
            acc.apply(seats, credit, actions, &payoffs)
        },
    )
}

pub(crate) fn play_implicit_counting<G: AnonymousGame>(
    pattern: ImplicitPattern,
    game: &G,
    topology: &Topology,
    ids: &[AgentId],
    held: &[usize],
    acc: &mut PayoffAccum<'_>,
) -> socsim_core::Result<()> {
    let mut scratch = CountingScratch::new(game.n_actions(0));
    for_each_implicit(
        pattern,
        topology,
        ids,
        held,
        |_id, seats, credit, actions| {
            scratch.compute(game, actions)?;
            acc.apply(seats, credit, actions, &scratch.payoffs)
        },
    )
}
