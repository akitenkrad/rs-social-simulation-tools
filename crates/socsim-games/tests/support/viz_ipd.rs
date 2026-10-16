#![allow(dead_code)]

use std::cell::RefCell;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use socsim_core::Recorder;
use socsim_engine::Simulation;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::solve::{pure_nash, support_enumeration};
use socsim_games::{
    assemble, build_world, write_game_spec, write_solutions, AssignGroup, AssignKind, Assignment,
    FixedPlayers, GameMetrics, GameSpec, GameWorld, InitialActions, MetricsOptions,
    NormalFormOutcome, PlayOptions, RuleBook, SolutionSet, TitForTat, UniformRandom,
};
use socsim_log::JsonlRecorder;

#[path = "viz_games.rs"]
mod viz_games;

pub const ROUNDS: u64 = 50;
pub const SEEDS: std::ops::Range<u64> = 0..5;

type VizResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Default)]
struct SharedBuffer(Rc<RefCell<Vec<u8>>>);

impl SharedBuffer {
    fn bytes(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn artifacts() -> VizResult<(GameSpec, SolutionSet)> {
    let (game, spec) = viz_games::prisoners_dilemma()?;
    let mut solutions = SolutionSet::new(&spec);
    solutions.push_pure_nash_all("pure", &pure_nash(&game)?)?;
    solutions.push_equilibrium_set("se", &support_enumeration(&game)?)?;
    Ok((spec, solutions))
}

pub fn build_simulation(
    seed: u64,
    recorder: Box<dyn Recorder>,
) -> VizResult<Simulation<GameWorld<PrisonersDilemma>>> {
    let game = viz_games::prisoners_dilemma()?.0;
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .cooperate_label("C")
            .population_share(true)
            .log_actions(true)
            .log_plays(true),
    )?;
    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0)))?;
    rules.register("uniform_random", Arc::new(UniformRandom))?;
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".to_owned()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("uniform_random".to_owned()),
                count: 1,
            },
        ]),
        1,
        ROUNDS,
        seed,
    )?;
    Ok(assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )?
    .add_mechanism(Box::new(metrics))
    .recorder(recorder)
    .build())
}

pub fn record_seed(seed: u64) -> VizResult<Vec<u8>> {
    let buffer = SharedBuffer::default();
    let mut simulation = build_simulation(seed, Box::new(JsonlRecorder::new(buffer.clone())))?;
    simulation.run()?;
    Ok(buffer.bytes())
}

pub fn write_output(output: &Path) -> VizResult<()> {
    fs::create_dir_all(output)?;
    let (spec, solutions) = artifacts()?;
    write_game_spec(&spec, &output.join("game_spec.json"))?;
    write_solutions(&solutions, &output.join("solutions.json"))?;
    for seed in SEEDS {
        fs::write(
            output.join(format!("seed_{seed}.jsonl")),
            record_seed(seed)?,
        )?;
    }
    Ok(())
}
