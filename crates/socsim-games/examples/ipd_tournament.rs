use std::sync::Arc;

use socsim_core::AgentId;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, EncounterId, InitialActions, MatrixGame, NormalFormOutcome, PayoffAggregation,
    PlayOptions, RoundRobin, RuleBook,
};

const ROUNDS: usize = 200;
const TEMPTATION: f64 = 5.0;
const REWARD: f64 = 3.0;
const PUNISHMENT: f64 = 1.0;
const SUCKER: f64 = 0.0;

fn main() {
    let game = MatrixGame::bimatrix(
        &[vec![REWARD, SUCKER], vec![TEMPTATION, PUNISHMENT]],
        &[vec![REWARD, TEMPTATION], vec![SUCKER, PUNISHMENT]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let strategies = [
        ("all_c", AutomatonKind::AllC),
        ("all_d", AutomatonKind::AllD),
        ("tft", AutomatonKind::Tft),
        ("grim", AutomatonKind::Grim),
        ("wsls", AutomatonKind::Wsls),
    ];
    let mut rules = RuleBook::new();
    for (kind, automaton) in &strategies {
        rules
            .register(kind, Arc::new(Automaton::new(automaton.clone(), 0)))
            .expect("strategy names are unique");
    }

    let seed = 1;
    let world = build_world(
        game,
        strategies.len(),
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(
            strategies
                .iter()
                .map(|(kind, _)| AssignGroup {
                    kind: AssignKind::Rule((*kind).to_owned()),
                    count: 1,
                })
                .collect(),
        ),
        ROUNDS,
        ROUNDS as u64,
        seed,
    )
    .expect("the tournament world is valid");
    let mut simulation = assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new()
            .decision_scope(DecisionScope::PerEncounter)
            .payoff_aggregation(PayoffAggregation::Sum),
        seed,
    )
    .expect("the tournament configuration is valid")
    .build();

    let mut total_scores = vec![0.0; strategies.len()];
    let mut tft_all_d_scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().expect("the tournament step succeeds");
        for (score, payoff) in total_scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
        for encounter_index in 0..simulation.world().plays().n_encounters() {
            let encounter_id = EncounterId(encounter_index as u32);
            let encounter = simulation
                .world()
                .plays()
                .encounter(encounter_id)
                .expect("the encounter exists");
            if encounter.seats.contains(&AgentId(1)) && encounter.seats.contains(&AgentId(2)) {
                let payoffs = simulation
                    .world()
                    .plays()
                    .payoffs(encounter_id)
                    .expect("the encounter payoffs exist");
                for (&agent, &payoff) in encounter.seats.iter().zip(payoffs) {
                    if agent == AgentId(2) {
                        tft_all_d_scores[0] += payoff;
                    } else if agent == AgentId(1) {
                        tft_all_d_scores[1] += payoff;
                    }
                }
            }
        }
    }

    let expected_tft = SUCKER + (ROUNDS as f64 - 1.0) * PUNISHMENT;
    let expected_all_d = TEMPTATION + (ROUNDS as f64 - 1.0) * PUNISHMENT;
    assert_eq!(tft_all_d_scores[0], expected_tft);
    assert_eq!(tft_all_d_scores[1], expected_all_d);

    for ((kind, _), score) in strategies.iter().zip(total_scores) {
        println!("{kind}: {score}");
    }
    println!(
        "TFT vs ALLD: TFT {}, ALLD {}",
        tft_all_d_scores[0], tft_all_d_scores[1]
    );
}
