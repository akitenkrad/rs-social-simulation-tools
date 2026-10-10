use std::sync::Arc;

use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, Mechanism, Phase, SimRng, StepContext};
use socsim_engine::Simulation;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, BuiltinPolicyState,
    DecisionContext, DecisionRule, EncounterRef, ExperimentState, FixedPlayers, GameWorld,
    InitialActions, MatrixGame, NormalFormGame, OutcomeContext, OutcomeRule, PlayOptions, RuleBook,
    RuleError, RuleInitContext, Slot, TftState, TitForTat,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReputationState {
    reputation: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum MyPolicyState {
    Builtin(BuiltinPolicyState),
    Discriminator { threshold: f64, seen: u32 },
}

impl ExperimentState for ReputationState {
    type PolicyState = MyPolicyState;
    type View<'a> = &'a [f64];

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {
        &self.reputation
    }
}

impl Slot<TftState> for MyPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self {
            Self::Builtin(state) => state.slot(),
            Self::Discriminator { .. } => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self {
            Self::Builtin(state) => state.slot_mut(),
            Self::Discriminator { .. } => None,
        }
    }

    fn from_slot(value: TftState) -> Self {
        Self::Builtin(<BuiltinPolicyState as Slot<TftState>>::from_slot(value))
    }
}

struct ReputationDiscriminator;

impl DecisionRule<MatrixGame, ReputationState> for ReputationDiscriminator {
    fn name(&self) -> &str {
        "reputation_discriminator"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &MatrixGame,
    ) -> Result<MyPolicyState, RuleError> {
        Ok(MyPolicyState::Discriminator {
            threshold: 0.5,
            seen: 0,
        })
    }

    fn decide(
        &self,
        state: &mut MyPolicyState,
        ctx: &DecisionContext<'_, MatrixGame, ReputationState>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let MyPolicyState::Discriminator { threshold, seen } = state else {
            return Err(RuleError::MissingSlot("Discriminator"));
        };
        *seen += 1;
        let opponent = ctx.opponents()[0];
        Ok(usize::from(ctx.view()[opponent.0 as usize] < *threshold))
    }
}

struct ReputationDiscount;

impl OutcomeRule<MatrixGame, ReputationState> for ReputationDiscount {
    fn name(&self) -> &str {
        "reputation_discount"
    }

    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, MatrixGame, ReputationState>,
        encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        ctx.game().payoffs_into(actions, payoffs);
        for (role, payoff) in payoffs.iter_mut().enumerate() {
            let agent = encounter.seats[role];
            *payoff *= ctx.x().reputation[agent.0 as usize];
        }
        Ok(())
    }
}

struct ReputationUpdate;

impl Mechanism<GameWorld<MatrixGame, ReputationState>> for ReputationUpdate {
    fn name(&self) -> &str {
        "reputation_update"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Reward]
    }

    fn apply(
        &mut self,
        _phase: Phase,
        ctx: &mut StepContext<'_, GameWorld<MatrixGame, ReputationState>>,
    ) -> socsim_core::Result<()> {
        let payoffs = ctx.world.payoffs().to_vec();
        for (reputation, payoff) in ctx.world.x_mut().reputation.iter_mut().zip(payoffs) {
            *reputation = payoff / 6.0;
        }
        Ok(())
    }
}

type World = GameWorld<MatrixGame, ReputationState>;

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn build_simulation(seed: u64, t_max: u64) -> Simulation<World> {
    let mut rules = RuleBook::new();
    rules
        .register("discriminator", Arc::new(ReputationDiscriminator))
        .unwrap();
    rules
        .register("tit_for_tat", Arc::new(TitForTat::new(0)))
        .unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        ReputationState {
            reputation: vec![0.5, 1.0],
        },
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("discriminator".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".into()),
                count: 1,
            },
        ]),
        8,
        t_max,
        seed,
    )
    .unwrap();

    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        ReputationDiscount,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(ReputationUpdate))
    .build()
}

fn run_steps(simulation: &mut Simulation<World>, count: u64) -> Vec<String> {
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn custom_rule_outcome_and_reward_mechanism_match_hand_calculation() {
    let mut simulation = build_simulation(31, 1);
    simulation.step().unwrap();

    assert_eq!(simulation.world().payoffs(), &[1.5, 3.0]);
    assert_eq!(simulation.world().x().reputation, vec![0.25, 0.5]);
    assert_eq!(
        simulation
            .world()
            .plays()
            .actions(socsim_games::EncounterId(0))
            .unwrap(),
        &[Some(0), Some(0)]
    );
    assert!(matches!(
        simulation.world().policies().state(AgentId(0)),
        Some(MyPolicyState::Discriminator { seen: 1, .. })
    ));
    assert!(matches!(
        simulation.world().policies().state(AgentId(1)),
        Some(MyPolicyState::Builtin(BuiltinPolicyState::TitForTat(
            TftState {
                last_opponent: Some(0)
            }
        )))
    ));
}

#[test]
fn custom_experiment_and_policy_state_resume_exactly_from_snapshot() {
    let mut uninterrupted = build_simulation(31, 6);
    let expected = run_steps(&mut uninterrupted, 6);

    let mut partial = build_simulation(31, 6);
    run_steps(&mut partial, 2);
    let snapshot = partial.snapshot();
    let mut resumed = build_simulation(999, 6);
    resumed.restore(snapshot);

    assert_eq!(run_steps(&mut resumed, 4), expected[2..]);
}
