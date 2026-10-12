//! Cournot oligopoly with three firms learning by damped best response.
//!
//! Inverse demand is `P = a - bQ` with `a = 100`, `b = 1`, unit cost `c = 10`,
//! and quantities in `[0, 100]`, so the Cournot-Nash quantity is
//! `(a - c) / ((n + 1) b) = 22.5`. Every firm moves simultaneously: it best
//! responds to the quantities its rivals played in the previous step and moves
//! half way there from its own last quantity. The firms start from
//! `[0, 50, 100]`.
//!
//! The run stops through the convergence check of [`ContinuousMetrics`] once
//! no firm's quantity has changed by `1e-9` or more for ten consecutive steps.
//! The example asserts that this happens before `t_max` and that every firm
//! ends within `1e-9` of the equilibrium, then prints the stopping step and
//! the quantities. Two runs print identical output.
//!
//! ```sh
//! cargo run -p socsim-games --features core,engine --example cournot
//! ```

use std::any::Any;
use std::sync::Arc;

use socsim_core::{Recorder, WorldState};
use socsim_games::continuous::Cournot;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, ContinuousMetrics,
    ContinuousMetricsOptions, DampedBestResponse, DecisionRule, InitialActions, NormalFormOutcome,
    PlayOptions, RuleBook, WholePopulation,
};

const FIRMS: usize = 3;
const A: f64 = 100.0;
const B: f64 = 1.0;
const COST: f64 = 10.0;
const Q_MAX: f64 = 100.0;
const DAMPING: f64 = 0.5;
const INITIAL: [f64; FIRMS] = [0.0, 50.0, 100.0];
const T_MAX: u64 = 500;
const STOP_TOL: f64 = 1.0e-9;
const STOP_WINDOW: u32 = 10;
const EQUILIBRIUM_TOL: f64 = 1.0e-9;
const SEED: u64 = 1;

/// Keeps the mean quantity reported by `game_metrics` for every step.
#[derive(Default)]
struct MeanRecorder {
    means: Vec<(u64, f64)>,
}

impl Recorder for MeanRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        if table != "game" {
            return;
        }
        for (column, value) in row {
            if *column == "mean_action.0" {
                self.means.push((t, *value));
            }
        }
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn main() {
    let game = Cournot::new(FIRMS, A, B, COST, Q_MAX).expect("the Cournot parameters are valid");
    let equilibrium = game.equilibrium_quantity();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new().stop(STOP_TOL, STOP_WINDOW),
    )
    .expect("the metrics configuration is valid");

    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<Cournot, ()>> =
        Arc::new(DampedBestResponse::<Cournot>::new(DAMPING).expect("the damping is in (0, 1]"));
    rules
        .register("damped_best_response", rule)
        .expect("rule kind is unique");

    let world = build_world(
        game,
        FIRMS,
        (),
        InitialActions::PerAgent(INITIAL.iter().map(|&q| vec![q]).collect()),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("damped_best_response".to_owned()),
            count: FIRMS,
        }]),
        1,
        T_MAX,
        SEED,
    )
    .expect("the world configuration is valid");
    let mut simulation = assemble(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        SEED,
    )
    .expect("the simulation configuration is valid")
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(MeanRecorder::default()))
    .build();

    simulation.run().expect("the game steps succeed");

    let steps = simulation.world().clock().t();
    let means = &simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<MeanRecorder>())
        .expect("the recorder is a MeanRecorder")
        .means;
    let quantities: Vec<f64> = simulation
        .world()
        .held()
        .iter()
        .map(|action| action[0])
        .collect();
    let max_error = quantities
        .iter()
        .map(|q| (q - equilibrium).abs())
        .fold(0.0, f64::max);

    assert!(
        simulation.stop_requested() && steps < T_MAX,
        "the convergence check stops the run before t_max"
    );
    assert_eq!(means.len() as u64, steps, "one mean quantity per step");
    assert!(
        max_error <= EQUILIBRIUM_TOL,
        "every firm is within {EQUILIBRIUM_TOL:e} of {equilibrium}"
    );

    println!("equilibrium quantity: {equilibrium:.10}");
    println!("stopped after {steps} of {T_MAX} steps");
    if let Some((t, mean)) = means.last() {
        println!("last metrics row: t = {t}, mean quantity {mean:.10}");
    }
    for (firm, quantity) in quantities.iter().enumerate() {
        println!("firm {firm}: {quantity:.10}");
    }
    println!("max |q_i - q*|: {max_error:.3e}");
}
