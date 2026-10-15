//! Reproduces the asynchronous spatial prisoner's-dilemma result of Huberman
//! and Glance (1993), with the synchronous Nowak--May model as a control.
//!
//! The all-defection result is limited to `b > 1.8`; at or below that value,
//! cooperation remains even with asynchronous updates.

use std::time::Instant;

use socsim_engine::{SequentialScheduler, Simulation};
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_async, assemble_with, build_world, AssignGroup, AssignKind, Assignment,
    AsyncActivation, AsyncPopulationStep, GameWorld, GridBoundary, GridNeighborhood, GridNeighbors,
    GridSpec, ImitateBest, ImitationTie, InitialActions, NormalFormOutcome, Pairwise, PlayOptions,
    RuleBook, Topology, Update,
};

const SIDE: usize = 99;
const CENTER: usize = 49;
const STEPS: u64 = 217;
const COOPERATE: usize = 0;
const DEFECT: usize = 1;

fn held_assignment(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: n_agents,
    }])
}

fn world(seed: u64) -> GameWorld<PrisonersDilemma> {
    let n_agents = SIDE * SIDE;
    let mut initial = vec![COOPERATE; n_agents];
    initial[CENTER * SIDE + CENTER] = DEFECT;
    let rules = RuleBook::new();
    build_world(
        PrisonersDilemma::weak(1.85).unwrap(),
        n_agents,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &held_assignment(n_agents),
        1,
        STEPS,
        seed,
    )
    .unwrap()
    .with_topology(
        Topology::grid(GridSpec {
            rows: SIDE,
            cols: SIDE,
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })
        .unwrap(),
    )
    .unwrap()
}

fn asynchronous(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
    assemble_async(
        world(seed),
        GridNeighborhood::new().include_self(true),
        Pairwise::new(NormalFormOutcome),
        AsyncPopulationStep::new(
            ImitateBest::new(Update::Asynchronous).tie_break(ImitationTie::KeepOwn),
        )
        .activation(AsyncActivation::UniformWithReplacement),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .scheduler(Box::new(SequentialScheduler))
    .build()
}

fn synchronous(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
    assemble_with(
        world(seed),
        GridNeighborhood::new().include_self(true),
        RuleBook::new(),
        Pairwise::new(NormalFormOutcome),
        ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .scheduler(Box::new(SequentialScheduler))
    .build()
}

fn cooperator_count(simulation: &Simulation<GameWorld<PrisonersDilemma>>) -> usize {
    simulation
        .world()
        .held()
        .iter()
        .filter(|&&action| action == COOPERATE)
        .count()
}

#[test]
fn asynchronous_runs_reach_all_defection_by_generation_217() {
    for seed in 0..=4 {
        let started = Instant::now();
        let mut simulation = asynchronous(seed);
        let mut all_defection_generation = None;
        for generation in 1..=STEPS {
            simulation.step().unwrap();
            if all_defection_generation.is_none() && cooperator_count(&simulation) == 0 {
                all_defection_generation = Some(generation);
            }
        }
        let elapsed = started.elapsed();
        let cooperators = cooperator_count(&simulation);
        eprintln!(
            "seed={seed} generation={STEPS} cooperators={cooperators} all_defection_generation={all_defection_generation:?} elapsed={elapsed:?}"
        );
        assert_eq!(
            cooperators, 0,
            "seed {seed} retained {cooperators} cooperators at generation {STEPS}"
        );
        assert!(all_defection_generation.is_some());
    }
}

#[test]
fn synchronous_control_retains_cooperation_at_generation_217() {
    let started = Instant::now();
    let mut simulation = synchronous(0);
    for _ in 0..STEPS {
        simulation.step().unwrap();
    }
    let elapsed = started.elapsed();
    let cooperators = cooperator_count(&simulation);
    eprintln!("sync generation={STEPS} cooperators={cooperators} elapsed={elapsed:?}");
    assert!(
        cooperators > 0,
        "the synchronous control retained no cooperators at generation {STEPS}"
    );
}
