use std::sync::{Arc, Mutex};

use socsim_core::{AgentId, SimRng, SocsimError};
use socsim_engine::Simulation;
use socsim_games::continuous::Cournot;
use socsim_games::{
    assemble, best_response_step, build_world, check_continuous_action, ActionError, AssignGroup,
    AssignKind, Assignment, BestResponseOracle, BrResult, BrStatus, ContinuousGame,
    ContinuousState, DampedBestResponse, DecisionContext, DecisionRule, GameWorld, GradientAscent,
    InitialActions, NormalFormGame, NormalFormOutcome, NumericBr, PlayOptions, RuleBook, RuleError,
    RuleInitContext, Slot, StageGame, WholePopulation,
};

// Helpers.

fn everyone(kind: &str, n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count: n_agents,
    }])
}

fn simulation<G: ContinuousGame>(
    game: G,
    rule: Arc<dyn DecisionRule<G, ()>>,
    initial: Vec<Vec<f64>>,
) -> Simulation<GameWorld<G>> {
    let n_agents = initial.len();
    let mut rules = RuleBook::new();
    rules.register("rule", rule).unwrap();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &everyone("rule", n_agents),
        4,
        1_000,
        11,
    )
    .unwrap();
    assemble(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        11,
    )
    .unwrap()
    .build()
}

fn held<G: ContinuousGame>(simulation: &Simulation<GameWorld<G>>) -> Vec<Vec<f64>> {
    simulation.world().held().to_vec()
}

fn mechanism_message(error: SocsimError) -> String {
    match error {
        SocsimError::Mechanism(message) => message,
        other => panic!("expected a mechanism error, got {other:?}"),
    }
}

fn cournot(n: usize, q_max: f64) -> Cournot {
    Cournot::new(n, 100.0, 1.0, 10.0, q_max).unwrap()
}

/// Cournot competition seen only through its payoffs: no analytic best
/// response or gradient. Payoffs outside the bounds are NaN, so any evaluation
/// outside the bounds is visible.
struct PayoffOnly {
    inner: Cournot,
    numeric: NumericBr,
}

impl StageGame for PayoffOnly {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        self.inner.n_roles()
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for PayoffOnly {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let inside = profile
            .iter()
            .enumerate()
            .all(|(r, action)| check_continuous_action(self, r, action).is_ok());
        if inside {
            self.inner.payoff(role, profile)
        } else {
            f64::NAN
        }
    }
}

impl ContinuousGame for PayoffOnly {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        self.inner.bounds(role)
    }

    fn numeric_br(&self) -> NumericBr {
        self.numeric
    }
}

// my_held.

type Seen = Arc<Mutex<Vec<(u64, AgentId, Vec<f64>)>>>;

/// Records `my_held` and plays it plus one.
struct MyHeldProbe {
    seen: Seen,
}

impl DecisionRule<Cournot, ()> for MyHeldProbe {
    fn name(&self) -> &str {
        "my_held_probe"
    }

    fn init_state(
        &self,
        _ctx: &RuleInitContext,
        _game: &Cournot,
    ) -> Result<<() as socsim_games::ExperimentState>::PolicyState, RuleError> {
        Ok(<_ as Slot<ContinuousState>>::from_slot(
            ContinuousState::default(),
        ))
    }

    fn decide(
        &self,
        _state: &mut <() as socsim_games::ExperimentState>::PolicyState,
        ctx: &DecisionContext<'_, Cournot, ()>,
        _rng: &mut SimRng,
    ) -> Result<Vec<f64>, RuleError> {
        let mine = ctx.my_held().clone();
        self.seen
            .lock()
            .unwrap()
            .push((ctx.t(), ctx.me(), mine.clone()));
        Ok(vec![mine[0] + 1.0])
    }
}

#[test]
fn my_held_is_the_agents_own_held_action_before_the_decision() {
    let seen = Seen::default();
    let mut simulation = simulation(
        cournot(2, 100.0),
        Arc::new(MyHeldProbe { seen: seen.clone() }),
        vec![vec![3.0], vec![7.0]],
    );
    for _ in 0..3 {
        simulation.step().unwrap();
    }
    let mut seen = seen.lock().unwrap().clone();
    seen.sort_by_key(|(t, agent, _)| (*t, *agent));
    let values: Vec<(AgentId, f64)> = seen.iter().map(|(_, agent, a)| (*agent, a[0])).collect();
    assert_eq!(
        values,
        vec![
            (AgentId(0), 3.0),
            (AgentId(1), 7.0),
            (AgentId(0), 4.0),
            (AgentId(1), 8.0),
            (AgentId(0), 5.0),
            (AgentId(1), 9.0),
        ]
    );
    assert_eq!(held(&simulation), vec![vec![6.0], vec![10.0]]);
}

// DampedBestResponse.

#[test]
fn damped_best_response_repeats_the_held_action_first_and_then_responds_simultaneously() {
    let game = cournot(3, 100.0);
    let rule = DampedBestResponse::<Cournot>::new(0.5).unwrap();
    let initial = vec![vec![0.0], vec![50.0], vec![100.0]];
    let mut simulation = simulation(game.clone(), Arc::new(rule), initial.clone());

    // First step: no previous actions are known, so the held actions repeat.
    simulation.step().unwrap();
    assert_eq!(held(&simulation), initial);

    // Second step: every firm responds to the first step's actions.
    // Firm 0: BR(150) = 0, firm 1: 0.5 * 50 + 0.5 * BR(100) = 25,
    // firm 2: 0.5 * 100 + 0.5 * BR(50) = 0.5 * 100 + 0.5 * 20 = 60.
    simulation.step().unwrap();
    assert_eq!(held(&simulation), vec![vec![0.0], vec![25.0], vec![60.0]]);

    // Later steps follow the simultaneous damped map exactly.
    let mut expected = held(&simulation);
    for _ in 0..50 {
        expected = best_response_step(&game, &expected, 0.5, None).unwrap();
        simulation.step().unwrap();
        assert_eq!(held(&simulation), expected);
    }
    for _ in 0..150 {
        simulation.step().unwrap();
    }
    for action in held(&simulation) {
        assert!((action[0] - 22.5).abs() <= 1e-9, "{action:?}");
    }
}

/// An oracle that always answers `action` with `status`.
struct ConstantOracle {
    action: Vec<f64>,
    status: BrStatus,
}

impl<G: ContinuousGame> BestResponseOracle<G> for ConstantOracle {
    fn best_response(&self, _game: &G, _role: usize, _profile: &[Vec<f64>]) -> BrResult {
        BrResult {
            action: self.action.clone(),
            residual: 0.0,
            status: self.status,
        }
    }
}

fn payoff_only(numeric: NumericBr) -> PayoffOnly {
    PayoffOnly {
        inner: cournot(2, 100.0),
        numeric,
    }
}

#[test]
fn damped_best_response_uses_a_converged_oracle() {
    let rule = DampedBestResponse::<PayoffOnly>::new(0.5)
        .unwrap()
        .with_oracle(Arc::new(ConstantOracle {
            action: vec![30.0],
            status: BrStatus::Converged,
        }));
    let mut simulation = simulation(
        payoff_only(NumericBr::Unavailable),
        Arc::new(rule),
        vec![vec![10.0], vec![50.0]],
    );
    simulation.step().unwrap();
    simulation.step().unwrap();
    assert_eq!(held(&simulation), vec![vec![20.0], vec![40.0]]);
}

#[test]
fn damped_best_response_stops_on_an_oracle_result_that_did_not_converge() {
    for status in [BrStatus::MaxIter, BrStatus::Failed] {
        let rule = DampedBestResponse::<PayoffOnly>::new(0.5)
            .unwrap()
            .with_oracle(Arc::new(ConstantOracle {
                action: vec![30.0],
                status,
            }));
        let mut simulation = simulation(
            payoff_only(NumericBr::Unavailable),
            Arc::new(rule),
            vec![vec![10.0], vec![50.0]],
        );
        // The first step repeats the held actions without a best response.
        simulation.step().unwrap();
        let message = mechanism_message(simulation.step().unwrap_err());
        assert!(message.contains("best response unavailable"), "{message}");
        assert!(message.contains(&format!("{status:?}")), "{message}");
        // The held actions are those of the first step.
        assert_eq!(held(&simulation), vec![vec![10.0], vec![50.0]]);
    }
}

#[test]
fn damped_best_response_falls_back_to_the_golden_section_search() {
    let analytic = cournot(3, 100.0);
    let numeric = PayoffOnly {
        inner: analytic.clone(),
        numeric: NumericBr::Unimodal1D,
    };
    let initial = vec![vec![0.0], vec![50.0], vec![100.0]];
    let mut numeric_run = simulation(
        numeric,
        Arc::new(DampedBestResponse::<PayoffOnly>::new(0.5).unwrap()),
        initial.clone(),
    );
    let mut analytic_run = simulation(
        analytic,
        Arc::new(DampedBestResponse::<Cournot>::new(0.5).unwrap()),
        initial,
    );
    for _ in 0..2 {
        numeric_run.step().unwrap();
        analytic_run.step().unwrap();
    }
    // The projected best responses 0 of firms 0 and 1 are ends of the bounds
    // and are returned exactly.
    assert_eq!(held(&numeric_run)[0], vec![0.0]);
    assert_eq!(held(&numeric_run)[1], vec![25.0]);
    for (x, y) in held(&numeric_run).iter().zip(held(&analytic_run)) {
        assert!(
            (x[0] - y[0]).abs() <= 1e-6 * y[0].abs().max(1.0),
            "{x:?} {y:?}"
        );
    }
}

// GradientAscent.

#[test]
fn gradient_ascent_repeats_the_held_action_on_the_first_step() {
    let initial = vec![vec![20.0], vec![30.0]];
    let mut simulation = simulation(
        cournot(2, 100.0),
        Arc::new(GradientAscent::new(0.1).unwrap()),
        initial.clone(),
    );
    simulation.step().unwrap();
    assert_eq!(held(&simulation), initial);
}

#[test]
fn numeric_gradient_matches_the_analytic_gradient_without_evaluating_outside_the_bounds() {
    // With |x| <= 1 the difference step is h = 1e-6, and a one-sided
    // difference of the Cournot payoff (second derivative -2b) is off by
    // exactly b h = 1e-6; central differences of the quadratic payoff are exact
    // up to rounding.
    let step = 0.1;
    let small = Cournot::new(2, 2.5, 1.0, 0.0, 1.0).unwrap();
    let large = Cournot::new(2, 100.0, 1.0, 10.0, 50.0).unwrap();
    for (game, initial, expected) in [
        // Firm 0 on the lower end (forward difference), firm 1 interior.
        (small.clone(), vec![vec![0.0], vec![0.2]], vec![0.23, 0.41]),
        // Firm 1 on the upper end (backward difference), firm 0 interior.
        (small, vec![vec![0.8], vec![1.0]], vec![0.79, 0.97]),
        // Both interior (central differences).
        (large, vec![vec![20.0], vec![30.0]], vec![22.0, 31.0]),
    ] {
        let numeric = PayoffOnly {
            inner: game.clone(),
            numeric: NumericBr::Unavailable,
        };
        let mut analytic_run = simulation(
            game,
            Arc::new(GradientAscent::new(step).unwrap()),
            initial.clone(),
        );
        let mut numeric_run = simulation(
            numeric,
            Arc::new(GradientAscent::new(step).unwrap()),
            initial,
        );
        for _ in 0..2 {
            analytic_run.step().unwrap();
            numeric_run.step().unwrap();
        }
        for ((x, y), e) in held(&numeric_run)
            .iter()
            .zip(held(&analytic_run))
            .zip(&expected)
        {
            assert!((y[0] - e).abs() <= 1e-12, "{y:?} {e}");
            // The difference of the gradients is at most 2e-6.
            assert!((x[0] - y[0]).abs() <= step * 2e-6, "{x:?} {y:?}");
        }
    }
}

/// Two roles, each on a dimension narrower than the difference step, with
/// payoff `3 x_role` inside the bounds and NaN outside.
struct Narrow;

impl StageGame for Narrow {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for Narrow {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        if check_continuous_action(self, role, &profile[role]).is_ok() {
            3.0 * profile[role][0]
        } else {
            f64::NAN
        }
    }
}

impl ContinuousGame for Narrow {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &[(0.0, 1e-7)]
    }
}

#[test]
fn numeric_gradient_on_a_narrow_dimension_uses_half_its_width() {
    let mut simulation = simulation(
        Narrow,
        Arc::new(GradientAscent::new(1e-9).unwrap()),
        vec![vec![0.0], vec![1e-7]],
    );
    simulation.step().unwrap();
    simulation.step().unwrap();
    let held = held(&simulation);
    assert!((held[0][0] - 3e-9).abs() <= 1e-6 * 3e-9, "{held:?}");
    assert_eq!(held[1], vec![1e-7]);
}

/// A two-dimensional game whose analytic gradient has a NaN in dimension 1.
struct NanGradient;

impl StageGame for NanGradient {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for NanGradient {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for NanGradient {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &[(0.0, 1.0), (0.0, 1.0)]
    }

    fn gradient(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        Some(vec![1.0, f64::NAN])
    }
}

/// A one-dimensional game whose payoff is NaN inside its bounds above 0.5.
struct NanPayoff;

impl StageGame for NanPayoff {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for NanPayoff {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let x = profile[role][0];
        if x > 0.5 {
            f64::NAN
        } else {
            x
        }
    }
}

impl ContinuousGame for NanPayoff {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &[(0.0, 1.0)]
    }
}

#[test]
fn a_non_finite_gradient_stops_the_step() {
    let expected = RuleError::NonFiniteGradient { dim: 1 }.to_string();
    let mut analytic = simulation(
        NanGradient,
        Arc::new(GradientAscent::new(0.1).unwrap()),
        vec![vec![0.5, 0.5], vec![0.5, 0.5]],
    );
    analytic.step().unwrap();
    let message = mechanism_message(analytic.step().unwrap_err());
    assert!(message.contains(&expected), "{message}");

    let expected = RuleError::NonFiniteGradient { dim: 0 }.to_string();
    let mut numeric = simulation(
        NanPayoff,
        Arc::new(GradientAscent::new(0.1).unwrap()),
        vec![vec![0.5], vec![0.2]],
    );
    numeric.step().unwrap();
    let message = mechanism_message(numeric.step().unwrap_err());
    assert!(message.contains(&expected), "{message}");
}

// Gaussian mutation of the Moran process.

/// A four-role continuous game with a two-dimensional action box whose payoff
/// is the sum of the agent's own components.
#[derive(Clone, Debug)]
struct Box2;

const BOX2: [(f64, f64); 2] = [(0.0, 1.0), (-1.0, 1.0)];

impl StageGame for Box2 {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        4
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for Box2 {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        profile[role].iter().sum()
    }
}

impl ContinuousGame for Box2 {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &BOX2
    }
}

impl socsim_games::CommonActionSpace for Box2 {}

const MORAN_W: f64 = 0.1;

fn moran_simulation(mutation: f64, sd: Option<f64>, seed: u64) -> Simulation<GameWorld<Box2>> {
    let rules = RuleBook::new();
    let world = build_world(
        Box2,
        4,
        (),
        InitialActions::PerAgent(vec![
            vec![0.1, -0.9],
            vec![0.3, -0.2],
            vec![0.6, 0.4],
            vec![0.9, 0.7],
        ]),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 4,
        }]),
        4,
        1_000,
        seed,
    )
    .unwrap();
    let mut moran = socsim_games::MoranUpdate::new(
        socsim_games::Fitness::Exponential { w: MORAN_W },
        mutation,
        socsim_games::Update::Synchronous,
    )
    .unwrap();
    if let Some(sd) = sd {
        moran = moran.gaussian_mutation(sd).unwrap();
    }
    socsim_games::assemble_with(
        world,
        WholePopulation::new(),
        rules,
        socsim_games::Pairwise::new(NormalFormOutcome),
        moran,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

fn rng_state(simulation: &Simulation<GameWorld<Box2>>) -> String {
    serde_json::to_string(&simulation.snapshot().rng).unwrap()
}

/// Replays one Moran step from the engine stream: parent, dying agent,
/// mutation decision, then two uniforms per component for the Box-Muller
/// transform, drawn whether or not the mutation happens.
fn replay_moran_step(
    rng: &mut SimRng,
    held: &[Vec<f64>],
    payoffs: &[f64],
    mutation: f64,
    sd: f64,
) -> Vec<Vec<f64>> {
    use rand::Rng;

    let weights: Vec<f64> = payoffs
        .iter()
        .map(|payoff| (MORAN_W * payoff).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    let target = rng.gen::<f64>() * total;
    let mut cumulative = 0.0;
    let mut parent = weights.len() - 1;
    for (index, weight) in weights.iter().enumerate() {
        cumulative += weight;
        if target < cumulative {
            parent = index;
            break;
        }
    }
    let death = rng.gen_range(0..held.len());
    let mutate = rng.gen::<f64>() < mutation;
    let mutant: Vec<f64> = held[parent]
        .iter()
        .zip(BOX2)
        .map(|(&value, (lo, hi))| {
            let u1 = 1.0 - rng.gen::<f64>();
            let u2 = rng.gen::<f64>();
            let normal = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            (value + sd * normal).clamp(lo, hi)
        })
        .collect();
    let mut next = held.to_vec();
    next[death] = if mutate { mutant } else { held[parent].clone() };
    next
}

#[test]
fn moran_gaussian_mutation_follows_the_box_muller_draws_from_the_engine_stream() {
    for (mutation, sd) in [(1.0, 0.25), (0.5, 0.25), (0.0, 0.25)] {
        let mut simulation = moran_simulation(mutation, Some(sd), 71);
        for _ in 0..40 {
            let mut rng = simulation.snapshot().rng;
            let before = held(&simulation);
            simulation.step().unwrap();
            let payoffs = simulation.world().payoffs().to_vec();
            let expected = replay_moran_step(&mut rng, &before, &payoffs, mutation, sd);
            let actual = held(&simulation);
            let bits = |actions: &[Vec<f64>]| -> Vec<u64> {
                actions
                    .iter()
                    .flatten()
                    .map(|value| value.to_bits())
                    .collect()
            };
            assert_eq!(bits(&actual), bits(&expected), "{mutation} {sd}");
            assert_eq!(
                serde_json::to_string(&rng).unwrap(),
                rng_state(&simulation),
                "{mutation} {sd}"
            );
        }
    }
}

#[test]
fn moran_draws_the_same_number_of_values_whatever_the_outcome() {
    // Mutation probability, standard deviation, and its absence change the
    // trajectory but not the position of the engine stream after each step.
    let mut runs = [
        moran_simulation(0.0, None, 73),
        moran_simulation(0.0, Some(0.5), 73),
        moran_simulation(0.3, Some(0.5), 73),
        moran_simulation(1.0, Some(0.5), 73),
        moran_simulation(1.0, Some(3.0), 73),
    ];
    for _ in 0..50 {
        for simulation in &mut runs {
            simulation.step().unwrap();
        }
        let first = rng_state(&runs[0]);
        for simulation in &runs[1..] {
            assert_eq!(rng_state(simulation), first);
        }
    }
    assert_ne!(held(&runs[0]), held(&runs[3]));
}

#[test]
fn moran_gaussian_mutation_is_reproducible_and_clamped_to_the_bounds() {
    let trajectory = |seed| {
        let mut simulation = moran_simulation(1.0, Some(2.0), seed);
        (0..60)
            .map(|_| {
                simulation.step().unwrap();
                held(&simulation)
            })
            .collect::<Vec<_>>()
    };
    let first = trajectory(79);
    assert_eq!(first, trajectory(79));
    assert_ne!(first, trajectory(80));

    let mut at_bound = false;
    for actions in &first {
        for action in actions {
            assert_eq!(action.len(), 2);
            for (&value, (lo, hi)) in action.iter().zip(BOX2) {
                assert!((lo..=hi).contains(&value), "{value}");
                at_bound |= value == lo || value == hi;
            }
        }
    }
    assert!(at_bound, "a large standard deviation must reach the bounds");
}

#[test]
fn copy_dynamics_copy_continuous_actions_unchanged() {
    // Fermi updating copies whole action vectors between agents.
    let rules = RuleBook::new();
    let initial = vec![
        vec![0.1, -0.9],
        vec![0.3, -0.2],
        vec![0.6, 0.4],
        vec![0.9, 0.7],
    ];
    let world = build_world(
        Box2,
        4,
        (),
        InitialActions::PerAgent(initial.clone()),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 4,
        }]),
        4,
        1_000,
        83,
    )
    .unwrap();
    let mut simulation = socsim_games::assemble_with(
        world,
        WholePopulation::new(),
        rules,
        socsim_games::Pairwise::new(NormalFormOutcome),
        socsim_games::FermiUpdate::new(0.1, socsim_games::Update::Synchronous).unwrap(),
        PlayOptions::new(),
        83,
    )
    .unwrap()
    .build();
    let mut changed = false;
    for _ in 0..30 {
        simulation.step().unwrap();
        for action in held(&simulation) {
            assert!(initial.contains(&action), "{action:?}");
        }
        changed |= held(&simulation) != initial;
    }
    assert!(changed);
}
