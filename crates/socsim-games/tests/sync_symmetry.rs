//! Synchronous imitate-the-best on the Nowak–May lattice preserves the
//! dihedral symmetry of a symmetric initial configuration, and the world at
//! `PostStep` holds pre-update payoffs together with post-update actions.

use std::any::Any;

use socsim_core::{AgentId, Recorder};
use socsim_engine::Simulation;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, GameMetrics, GameWorld,
    GridBoundary, GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, ImitationTie,
    InitialActions, MetricsOptions, NormalFormOutcome, Pairwise, PlayOptions, RuleBook, Topology,
    Update,
};

const SIDE: usize = 99;
const CENTER: usize = 49;
const B: f64 = 1.85;
const GENERATIONS: u64 = 60;
const COOPERATE: usize = 0;
const DEFECT: usize = 1;

type World = GameWorld<PrisonersDilemma>;

#[derive(Default)]
struct RowRecorder {
    rows: Vec<(u64, String, f64)>,
}

impl Recorder for RowRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.rows.extend(
            row.iter()
                .map(|(column, value)| (t, format!("{table}.{column}"), *value)),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn index(row: usize, col: usize) -> usize {
    row * SIDE + col
}

fn simulation() -> Simulation<World> {
    let game = PrisonersDilemma::weak(B).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true)).unwrap();
    let mut initial = vec![COOPERATE; SIDE * SIDE];
    initial[index(CENTER, CENTER)] = DEFECT;
    let rules = RuleBook::new();
    let world = build_world(
        game,
        SIDE * SIDE,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: SIDE * SIDE,
        }]),
        2,
        GENERATIONS,
        92,
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
    .unwrap();
    assemble_with(
        world,
        GridNeighborhood::new().include_self(true),
        rules,
        Pairwise::new(NormalFormOutcome),
        ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
        PlayOptions::new(),
        92,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

/// Asserts that `values` is invariant under left-right reflection,
/// up-down reflection, and transposition of the lattice.
fn assert_dihedral<T: PartialEq + std::fmt::Debug>(values: &[T], what: &str, generation: u64) {
    for row in 0..SIDE {
        for col in 0..SIDE {
            let value = &values[index(row, col)];
            assert_eq!(
                value,
                &values[index(row, SIDE - 1 - col)],
                "{what} breaks left-right symmetry at ({row}, {col}) in generation {generation}"
            );
            assert_eq!(
                value,
                &values[index(SIDE - 1 - row, col)],
                "{what} breaks up-down symmetry at ({row}, {col}) in generation {generation}"
            );
            assert_eq!(
                value,
                &values[index(col, row)],
                "{what} breaks transpose symmetry at ({row}, {col}) in generation {generation}"
            );
        }
    }
}

/// Recomputes each agent's payoff from `held` with Moore neighbors and self-play.
fn hand_payoffs(held: &[usize]) -> Vec<f64> {
    let payoff = |own: usize, other: usize| match (own, other) {
        (COOPERATE, COOPERATE) => 1.0,
        (DEFECT, COOPERATE) => B,
        _ => 0.0,
    };
    let mut out = vec![0.0; held.len()];
    for row in 0..SIDE {
        for col in 0..SIDE {
            let own = held[index(row, col)];
            let mut total = 0.0;
            for dr in -1_i64..=1 {
                for dc in -1_i64..=1 {
                    let r = row as i64 + dr;
                    let c = col as i64 + dc;
                    if r < 0 || c < 0 || r >= SIDE as i64 || c >= SIDE as i64 {
                        continue;
                    }
                    total += payoff(own, held[index(r as usize, c as usize)]);
                }
            }
            out[index(row, col)] = total;
        }
    }
    out
}

#[test]
fn synchronous_imitate_best_preserves_lattice_symmetry_and_records_pre_update_payoffs() {
    let mut simulation = simulation();
    let mut defectors = Vec::new();
    for generation in 1..=GENERATIONS {
        let before = simulation.world().held().to_vec();
        simulation.step().unwrap();
        let world = simulation.world();

        // Payoffs, play counts, and step records describe the pre-update actions.
        let expected = hand_payoffs(&before);
        for (agent, (&actual, &expected)) in world.payoffs().iter().zip(&expected).enumerate() {
            assert!(
                (actual - expected).abs() <= 1e-12,
                "agent {agent} payoff {actual} differs from {expected} in generation {generation}"
            );
        }
        for (agent, &action) in before.iter().enumerate() {
            let record = world
                .history(AgentId(agent as u64))
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(record.action, Some(action));
            assert_eq!(record.payoff, world.payoffs()[agent]);
            assert!(world.n_played()[agent] >= 4);
        }

        assert_dihedral(world.held(), "held", generation);
        assert_dihedral(world.payoffs(), "payoff", generation);

        if generation == 1 {
            // Eight defections against cooperators plus self-play (D, D) = 0.
            assert!((world.payoffs()[index(CENTER, CENTER)] - 8.0 * B).abs() <= 1e-12);
            for row in 0..SIDE {
                for col in 0..SIDE {
                    let in_block = row.abs_diff(CENTER) <= 1 && col.abs_diff(CENTER) <= 1;
                    let expected = if in_block { DEFECT } else { COOPERATE };
                    assert_eq!(world.held()[index(row, col)], expected);
                }
            }
        }
        defectors.push(world.held().iter().filter(|&&a| a == DEFECT).count());
    }
    assert_eq!(&defectors[..4], &[9, 25, 41, 81]);
    assert_eq!(defectors[GENERATIONS as usize - 1], 5165);

    // population_share reports post-update held actions at PostStep.
    let rows = &simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows;
    let total = (SIDE * SIDE) as f64;
    let shares: Vec<_> = rows
        .iter()
        .filter(|(_, column, _)| column == "game.population_share.a1")
        .map(|(_, _, value)| *value)
        .collect();
    assert_eq!(shares.len(), GENERATIONS as usize);
    for (share, &count) in shares.iter().zip(&defectors) {
        assert_eq!(*share, count as f64 / total);
    }
}
