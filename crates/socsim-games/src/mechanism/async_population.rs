//! Asynchronous, in-place population updates for held agents.

use std::marker::PhantomData;

use rand::Rng;
use socsim_core::{Mechanism, Phase, SocsimError, StepContext};

use super::play::{record_implicit_step, AggregateCollector, KernelRunner};
use super::population::sealed::ActionSpace;
use super::population::{
    check_dynamics, check_payoffs, CopyKernel, FermiKernel, ImitateBestKernel, ProportionalKernel,
};
use crate::assemble::sealed::DynamicsCheck;
use crate::outcome::kernel::LocalEncounterIds;
use crate::{
    CommonActionSpace, ExperimentState, FermiUpdate, FiniteGame, GameError, GameWorld, ImitateBest,
    ImplicitPattern, NormalFormGame, OutcomeContext, PayoffAggregation, ProportionalImitation,
    StageGame, Topology, Update,
};

/// Order in which an asynchronous population step activates agents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AsyncActivation {
    /// Activate every agent once in [`StepContext::agent_order`].
    #[default]
    AgentOrder,
    /// Make `N` independent uniform draws with replacement in a population of
    /// size `N`.
    ///
    /// This is the activation scheme used for the asynchronous procedure of
    /// Huberman and Glance (1993). A sequential scheduler avoids consuming an
    /// otherwise unused random permutation before these draws.
    UniformWithReplacement,
}

/// Configuration for an asynchronous population update.
///
/// Each activation recomputes every payoff that the copying rule may inspect
/// from the current held actions, applies the rule, and immediately writes the
/// activated agent's held action. At the end of the step, payoffs and history
/// are recorded once from the final held actions. Only matchings with an
/// implicit grid, network, or whole-population pattern are supported.
///
/// The supported dynamics are [`ImitateBest`], [`FermiUpdate`], and
/// [`ProportionalImitation`]. The Moran process is not supported because it is
/// one birth-death event per step rather than one rule application per
/// activated agent.
#[derive(Clone, Debug)]
pub struct AsyncPopulationStep<D> {
    pub(crate) dynamics: D,
    activation: AsyncActivation,
    log_updates: bool,
}

impl<D> AsyncPopulationStep<D> {
    /// Constructs a step using [`AsyncActivation::AgentOrder`] without update
    /// events.
    pub fn new(dynamics: D) -> Self {
        Self {
            dynamics,
            activation: AsyncActivation::AgentOrder,
            log_updates: false,
        }
    }

    /// Selects the activation scheme.
    pub fn activation(mut self, activation: AsyncActivation) -> Self {
        self.activation = activation;
        self
    }

    /// Enables or disables `game.async_update` events for changed actions.
    pub fn log_updates(mut self, log_updates: bool) -> Self {
        self.log_updates = log_updates;
        self
    }

    /// Returns the selected activation scheme.
    pub fn activation_mode(&self) -> AsyncActivation {
        self.activation
    }

    /// Returns whether changed actions produce update events.
    pub fn updates_logged(&self) -> bool {
        self.log_updates
    }
}

pub(crate) trait AsyncDynamics<G, X>: Send + Sync + Sized + 'static
where
    G: NormalFormGame,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError>;
    fn into_kernel(self) -> Box<dyn CopyKernel<G>>;
}

fn require_asynchronous(update: Update) -> Result<(), GameError> {
    match update {
        Update::Asynchronous => Ok(()),
        Update::Synchronous => Err(GameError::Population(
            "assemble_async requires Update::Asynchronous, got Synchronous".to_owned(),
        )),
    }
}

impl<G, X> AsyncDynamics<G, X> for ImitateBest
where
    G: NormalFormGame + CommonActionSpace,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        require_asynchronous(self.update())?;
        check_dynamics(self, check)
    }

    fn into_kernel(self) -> Box<dyn CopyKernel<G>> {
        Box::new(ImitateBestKernel::<G>::new(self.tie_policy()))
    }
}

impl<G, X> AsyncDynamics<G, X> for FermiUpdate
where
    G: NormalFormGame + CommonActionSpace,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        require_asynchronous(self.update())?;
        check_dynamics(self, check)
    }

    fn into_kernel(self) -> Box<dyn CopyKernel<G>> {
        Box::new(FermiKernel::new(self.k()))
    }
}

impl<G, X> AsyncDynamics<G, X> for ProportionalImitation
where
    G: FiniteGame + CommonActionSpace,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn prepare(&self, check: &DynamicsCheck<'_, G, X>) -> Result<(), GameError> {
        require_asynchronous(self.update())?;
        check_dynamics(self, check)
    }

    fn into_kernel(self) -> Box<dyn CopyKernel<G>> {
        Box::new(ProportionalKernel::new(self.beta()))
    }
}

pub(crate) struct AsyncPopulationMechanism<G: StageGame, X: ExperimentState> {
    kernel: Box<dyn CopyKernel<G>>,
    runner: Box<dyn KernelRunner<G, X>>,
    pattern: ImplicitPattern,
    aggregation: PayoffAggregation,
    aggregate: Option<AggregateCollector<G>>,
    activation: AsyncActivation,
    log_updates: bool,
    encounter_ids: LocalEncounterIds,
    local: Vec<f64>,
    local_played: Vec<u32>,
    marker: PhantomData<fn() -> X>,
}

impl<G: StageGame, X: ExperimentState> AsyncPopulationMechanism<G, X> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        kernel: Box<dyn CopyKernel<G>>,
        runner: Box<dyn KernelRunner<G, X>>,
        pattern: ImplicitPattern,
        aggregation: PayoffAggregation,
        aggregate: Option<AggregateCollector<G>>,
        activation: AsyncActivation,
        log_updates: bool,
        n_agents: usize,
        topology: &Topology,
    ) -> socsim_core::Result<Self> {
        Ok(Self {
            kernel,
            runner,
            pattern,
            aggregation,
            aggregate,
            activation,
            log_updates,
            encounter_ids: LocalEncounterIds::new(pattern, topology)?,
            local: vec![0.0; n_agents],
            local_played: vec![0; n_agents],
            marker: PhantomData,
        })
    }

    fn local_payoff(&mut self, world: &GameWorld<G, X>, index: usize) -> socsim_core::Result<()> {
        let context = OutcomeContext {
            t: world.clock.t(),
            game: &world.game,
            x: &world.x,
            held: &world.held,
            topology: &world.topology,
        };
        let result = self
            .runner
            .local_payoff(
                self.pattern,
                &context,
                &world.ids,
                &self.encounter_ids,
                index,
                self.aggregation,
            )
            .ok_or_else(|| {
                SocsimError::Mechanism(
                    "async_population_step requires a local payoff runner".to_owned(),
                )
            })??;
        if !result.0.is_finite() {
            return Err(SocsimError::Mechanism(format!(
                "{} found a non-finite payoff for agent {index}",
                self.kernel.name()
            )));
        }
        self.local[index] = result.0;
        self.local_played[index] = result.1;
        Ok(())
    }

    fn refresh_local(&mut self, world: &GameWorld<G, X>, index: usize) -> socsim_core::Result<()> {
        if self.pattern == ImplicitPattern::WholePopulation {
            let context = OutcomeContext {
                t: world.clock.t(),
                game: &world.game,
                x: &world.x,
                held: &world.held,
                topology: &world.topology,
            };
            self.runner
                .whole_population_payoffs(
                    &context,
                    &world.ids,
                    self.aggregation,
                    &mut self.local,
                    &mut self.local_played,
                )
                .ok_or_else(|| {
                    SocsimError::Mechanism(
                        "async_population_step requires a local payoff runner".to_owned(),
                    )
                })??;
            return check_payoffs(&self.local, self.kernel.name());
        }

        self.local_payoff(world, index)?;
        match &world.topology {
            Topology::None => {
                for other in 0..world.held.len() {
                    if other != index {
                        self.local_payoff(world, other)?;
                    }
                }
            }
            #[cfg(feature = "net")]
            Topology::Network(network) => {
                for neighbor in network
                    .neighbors(socsim_core::AgentId(index as u64))
                    .to_vec()
                {
                    self.local_payoff(world, neighbor.0 as usize)?;
                }
            }
            #[cfg(feature = "grid")]
            Topology::Grid(grid) => {
                for neighbor in grid.adjacency().neighbors(index).to_vec() {
                    self.local_payoff(world, neighbor)?;
                }
            }
        }
        Ok(())
    }

    fn activate(
        &mut self,
        step: &mut StepContext<'_, GameWorld<G, X>>,
        index: usize,
        order: usize,
    ) -> socsim_core::Result<()>
    where
        G::Action: ActionSpace<G>,
    {
        if index >= step.world.held.len() {
            return Err(SocsimError::Mechanism(format!(
                "async_population_step received unknown agent index {index}"
            )));
        }
        self.refresh_local(step.world, index)?;

        let (agent, from, to, changed) = {
            let world = &mut *step.world;
            let source = self.kernel.source(
                &world.game,
                &world.topology,
                &world.held,
                &self.local,
                index,
                step.rng,
            );
            let from = world.held[index].clone();
            let to = world.held[source.unwrap_or(index)].clone();
            let changed =
                <G::Action as ActionSpace<G>>::tie_cmp(&from, &to) != std::cmp::Ordering::Equal;
            world.held[index] = to.clone();
            (world.ids[index], from, to, changed)
        };

        if self.log_updates && changed {
            step.recorder.record_event(
                step.clock.t(),
                "game.async_update",
                serde_json::json!({
                    "agent": agent.0,
                    "order": order,
                    "from": <G::Action as ActionSpace<G>>::event_value(&from),
                    "to": <G::Action as ActionSpace<G>>::event_value(&to),
                }),
            );
        }
        Ok(())
    }

    fn update_and_record(
        &mut self,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()>
    where
        G::Action: ActionSpace<G>,
    {
        let n_agents = step.world.held.len();
        match self.activation {
            AsyncActivation::AgentOrder => {
                for order in 0..step.agent_order.len() {
                    let agent = step.agent_order[order];
                    self.activate(step, agent.0 as usize, order)?;
                }
            }
            AsyncActivation::UniformWithReplacement => {
                for order in 0..n_agents {
                    let index = step.rng.gen_range(0..n_agents);
                    self.activate(step, index, order)?;
                }
            }
        }

        step.world.payoff.fill(0.0);
        step.world.n_played.fill(0);
        step.world.played_counts = None;
        record_implicit_step(
            step.world,
            self.runner.as_ref(),
            self.pattern,
            self.aggregation,
            self.aggregate.as_ref(),
        )
    }
}

impl<G, X> Mechanism<GameWorld<G, X>> for AsyncPopulationMechanism<G, X>
where
    G: NormalFormGame,
    G::Action: ActionSpace<G>,
    X: ExperimentState,
{
    fn name(&self) -> &str {
        "async_population_step"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PreStep, Phase::Reward]
    }

    fn apply(
        &mut self,
        phase: Phase,
        step: &mut StepContext<'_, GameWorld<G, X>>,
    ) -> socsim_core::Result<()> {
        match phase {
            Phase::PreStep => {
                step.world.plays.clear();
                step.world.payoff.fill(0.0);
                step.world.n_played.fill(0);
                step.world.played_counts = None;
                Ok(())
            }
            Phase::Reward => self.update_and_record(step),
            _ => Ok(()),
        }
    }
}
