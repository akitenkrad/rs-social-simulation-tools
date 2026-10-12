use std::sync::atomic::{AtomicUsize, Ordering};

use socsim_games::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};
use socsim_games::solve::{
    best_response_iteration, best_response_step, golden_section_best_response, BestResponseOracle,
    BrIteration, BrIterationOptions, BrResult, BrStatus, GoldenSectionOptions, SolveError,
};
use socsim_games::{
    check_continuous_action, ActionError, AggregateGame, ContinuousGame, NormalFormGame, NumericBr,
    StageGame,
};

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0)
}

fn symmetric(n: usize, value: f64) -> Vec<Vec<f64>> {
    vec![vec![value]; n]
}

#[test]
fn cournot_equilibrium_quantity_matches_the_closed_form() {
    for (n, expected) in [(2, 30.0), (3, 22.5), (5, 15.0), (10, 90.0 / 11.0)] {
        let game = Cournot::new(n, 100.0, 1.0, 10.0, 100.0).unwrap();
        assert_eq!(game.n_roles(), n);
        assert!(
            close(game.equilibrium_quantity(), expected),
            "n = {n}: {} != {expected}",
            game.equilibrium_quantity()
        );
    }
}

#[test]
fn cournot_equilibrium_quantity_is_capped_at_q_max() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 10.0).unwrap();
    assert_eq!(game.equilibrium_quantity(), 10.0);
    let br = game.best_response(0, &symmetric(3, 10.0)).unwrap();
    assert_eq!(br, vec![10.0]);
}

#[test]
fn cournot_best_response_is_the_projected_first_order_condition() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    assert_eq!(game.bounds(1), &[(0.0, 100.0)]);
    assert_eq!(game.numeric_br(), NumericBr::Unimodal1D);

    // Interior: (a - c - b Q_{-i}) / (2b) with Q_{-i} = 30.
    let profile = vec![vec![0.0], vec![10.0], vec![20.0]];
    assert_eq!(game.best_response(0, &profile).unwrap(), vec![30.0]);
    // Projected onto 0 when the others already flood the market.
    let flooded = vec![vec![0.0], vec![60.0], vec![60.0]];
    assert_eq!(game.best_response(0, &flooded).unwrap(), vec![0.0]);
    // Projected onto q_max when the cap binds.
    let capped = Cournot::new(2, 1000.0, 1.0, 0.0, 100.0).unwrap();
    assert_eq!(
        capped.best_response(1, &[vec![0.0], vec![0.0]]).unwrap(),
        vec![100.0]
    );

    // The symmetric equilibrium is a fixed point of the best response.
    let q = game.equilibrium_quantity();
    let br = game.best_response(2, &symmetric(3, q)).unwrap();
    assert!((br[0] - q).abs() <= 1e-8);
    // The analytic gradient vanishes at the interior equilibrium.
    let gradient = game.gradient(2, &symmetric(3, q)).unwrap();
    assert_eq!(gradient.len(), 1);
    assert!(gradient[0].abs() <= 1e-8);
}

#[test]
fn best_responses_do_not_read_the_own_entry_of_the_profile() {
    let cournot = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let low = vec![vec![0.0], vec![10.0], vec![20.0]];
    let high = vec![vec![99.0], vec![10.0], vec![20.0]];
    assert_eq!(
        cournot.best_response(0, &low),
        cournot.best_response(0, &high)
    );

    let bertrand = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    let low = vec![vec![3.0], vec![0.0], vec![7.0]];
    let high = vec![vec![3.0], vec![90.0], vec![7.0]];
    assert_eq!(
        bertrand.best_response(1, &low),
        bertrand.best_response(1, &high)
    );
}

#[test]
fn cournot_payoffs_into_matches_payoff() {
    let game = Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap();
    let profile = vec![vec![0.0], vec![50.0], vec![12.5]];
    let mut out = [0.0; 3];
    game.payoffs_into(&profile, &mut out);
    for (role, value) in out.iter().enumerate() {
        assert_eq!(*value, game.payoff(role, &profile));
    }
    // Firm 1: 50 * (100 - 1.1 * 62.5 - 10).
    assert!(close(out[1], 50.0 * (100.0 - 1.1 * 62.5 - 10.0)));
    assert_eq!(out[0], 0.0);

    let mut aggregates = [0.0; 2];
    game.aggregate_into(&profile, &mut aggregates);
    let names: Vec<_> = game
        .aggregate_spec()
        .iter()
        .map(|spec| spec.name())
        .collect();
    assert_eq!(names, ["total_output", "price"]);
    assert_eq!(aggregates[0], 62.5);
    assert!(close(aggregates[1], 100.0 - 1.1 * 62.5));
}

#[test]
fn bertrand_equilibrium_price_follows_the_case_split() {
    // Interior: p* = (a + b c) / (2b - (n - 1) d) = 12 / 2 = 6.
    let game = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    assert_eq!(game.equilibrium_price(), Some(6.0));
    let br = game.best_response(0, &symmetric(3, 6.0)).unwrap();
    assert!((br[0] - 6.0).abs() <= 1e-8);
    // The equilibrium demand is b (p* - c) = 10.
    assert!(close(
        game.payoff(0, &symmetric(3, 6.0)),
        (6.0 - 1.0) * 10.0
    ));
    let gradient = game.gradient(0, &symmetric(3, 6.0)).unwrap();
    assert!(gradient[0].abs() <= 1e-8);

    // Capped at p_max: the best response to all-5 is 22 / 4 = 5.5, projected to 5.
    let capped = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 5.0).unwrap();
    assert_eq!(capped.equilibrium_price(), Some(5.0));
    assert_eq!(
        capped.best_response(2, &symmetric(3, 5.0)).unwrap(),
        vec![5.0]
    );

    // p* = -4 < c: outside the scope of the theory.
    let negative = BertrandDifferentiated::new(3, -10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    assert_eq!(negative.equilibrium_price(), None);

    assert_eq!(game.bounds(0), &[(0.0, 100.0)]);
    let mut aggregates = [0.0; 2];
    game.aggregate_into(&symmetric(3, 6.0), &mut aggregates);
    let names: Vec<_> = game
        .aggregate_spec()
        .iter()
        .map(|spec| spec.name())
        .collect();
    assert_eq!(names, ["mean_price", "total_demand"]);
    assert!(close(aggregates[0], 6.0));
    assert!(close(aggregates[1], 30.0));
}

#[test]
fn public_goods_theory_values_cover_every_region() {
    // r = 2: social dilemma, equilibrium 0 and optimum e.
    let dilemma = PublicGoodsContinuous::new(4, 10.0, 2.0).unwrap();
    assert_eq!(dilemma.equilibrium_contribution(), Some(0.0));
    assert_eq!(dilemma.social_optimum_contribution(), Some(10.0));
    assert_eq!(
        dilemma.best_response(0, &symmetric(4, 5.0)).unwrap(),
        vec![0.0]
    );

    // r = 6 > k: contributing everything is dominant.
    let generous = PublicGoodsContinuous::new(4, 10.0, 6.0).unwrap();
    assert_eq!(generous.equilibrium_contribution(), Some(10.0));
    assert_eq!(generous.social_optimum_contribution(), Some(10.0));
    assert_eq!(
        generous.best_response(1, &symmetric(4, 0.0)).unwrap(),
        vec![10.0]
    );

    // r = 0.5 < 1: neither the equilibrium nor the optimum contributes.
    let wasteful = PublicGoodsContinuous::new(4, 10.0, 0.5).unwrap();
    assert_eq!(wasteful.equilibrium_contribution(), Some(0.0));
    assert_eq!(wasteful.social_optimum_contribution(), Some(0.0));

    // r = k: every contribution is a best response; the smallest is returned.
    let indifferent = PublicGoodsContinuous::new(4, 10.0, 4.0).unwrap();
    assert_eq!(indifferent.equilibrium_contribution(), None);
    assert_eq!(
        indifferent.best_response(2, &symmetric(4, 7.0)).unwrap(),
        vec![0.0]
    );

    // r = 1: the total payoff does not depend on the contributions.
    let neutral = PublicGoodsContinuous::new(4, 10.0, 1.0).unwrap();
    assert_eq!(neutral.social_optimum_contribution(), None);
    assert_eq!(neutral.equilibrium_contribution(), Some(0.0));
}

#[test]
fn public_goods_payoffs_gradient_and_aggregates() {
    let game = PublicGoodsContinuous::new(4, 10.0, 2.0).unwrap();
    assert_eq!(game.bounds(3), &[(0.0, 10.0)]);
    let profile = vec![vec![0.0], vec![10.0], vec![4.0], vec![2.0]];
    let mut out = [0.0; 4];
    game.payoffs_into(&profile, &mut out);
    for (role, value) in out.iter().enumerate() {
        assert_eq!(*value, game.payoff(role, &profile));
    }
    // Player 1: 10 - 10 + (2 / 4) * 16 = 8.
    assert!(close(out[1], 8.0));
    assert_eq!(game.gradient(0, &profile).unwrap(), vec![2.0 / 4.0 - 1.0]);

    let mut aggregates = [0.0];
    game.aggregate_into(&profile, &mut aggregates);
    assert_eq!(game.aggregate_spec()[0].name(), "total_contribution");
    assert_eq!(aggregates[0], 16.0);
}

// ---------------------------------------------------------------------------
// Best-response iteration.
// ---------------------------------------------------------------------------

/// Error bound of the analytic solver path.
const ANALYTIC_TOL: f64 = 1e-8;

/// Relative error bound of the numeric (golden-section) path:
/// `|x_num - x_br| <= 1e-6 * max(1, |x_br|)`.
fn numeric_close(numeric: f64, analytic: f64) -> bool {
    (numeric - analytic).abs() <= 1e-6 * analytic.abs().max(1.0)
}

fn midpoints<G: ContinuousGame>(game: &G) -> Vec<Vec<f64>> {
    (0..game.n_roles())
        .map(|role| {
            game.bounds(role)
                .iter()
                .map(|&(lo, hi)| 0.5 * (lo + hi))
                .collect()
        })
        .collect()
}

fn solve<G: ContinuousGame>(game: &G) -> BrIteration {
    best_response_iteration(game, &midpoints(game), &BrIterationOptions::new()).unwrap()
}

fn assert_all_within(result: &BrIteration, expected: f64, tol: f64) {
    for (role, action) in result.profile.iter().enumerate() {
        assert_eq!(action.len(), 1);
        assert!(
            (action[0] - expected).abs() <= tol,
            "role {role}: {} is not within {tol} of {expected}",
            action[0]
        );
    }
}

/// Cournot without the analytic best response, so the numeric fallback (or an
/// oracle) is used.
struct NumericCournot(Cournot);

impl StageGame for NumericCournot {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        self.0.n_roles()
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for NumericCournot {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        self.0.payoff(role, profile)
    }
}

impl ContinuousGame for NumericCournot {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        self.0.bounds(role)
    }

    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unimodal1D
    }
}

/// A two-dimensional game without an analytic best response. It even claims
/// to be unimodal, which must not lead to a coordinate-wise search.
struct TwoDimensional {
    bounds: [(f64, f64); 2],
    numeric: NumericBr,
}

impl TwoDimensional {
    fn new(numeric: NumericBr) -> Self {
        Self {
            bounds: [(0.0, 1.0), (0.0, 1.0)],
            numeric,
        }
    }
}

impl StageGame for TwoDimensional {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for TwoDimensional {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        // Maximized at (0.25, 0.75) whatever the opponent does.
        let own = &profile[role];
        -(own[0] - 0.25).powi(2) - (own[1] - 0.75).powi(2)
    }
}

impl ContinuousGame for TwoDimensional {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    fn numeric_br(&self) -> NumericBr {
        self.numeric
    }
}

struct FixedOracle {
    action: Vec<f64>,
    status: BrStatus,
    calls: AtomicUsize,
}

impl FixedOracle {
    fn new(action: Vec<f64>, status: BrStatus) -> Self {
        Self {
            action,
            status,
            calls: AtomicUsize::new(0),
        }
    }
}

impl<G: ContinuousGame> BestResponseOracle<G> for FixedOracle {
    fn best_response(&self, _game: &G, _role: usize, _profile: &[Vec<f64>]) -> BrResult {
        self.calls.fetch_add(1, Ordering::Relaxed);
        BrResult {
            action: self.action.clone(),
            residual: 0.0,
            status: self.status,
        }
    }
}

/// An oracle that solves the Cournot best response in closed form.
struct CournotOracle;

impl BestResponseOracle<NumericCournot> for CournotOracle {
    fn best_response(&self, game: &NumericCournot, role: usize, profile: &[Vec<f64>]) -> BrResult {
        BrResult {
            action: game.0.best_response(role, profile).unwrap(),
            residual: 0.0,
            status: BrStatus::Converged,
        }
    }
}

#[test]
fn iteration_finds_the_cournot_equilibrium() {
    for (n, expected) in [(2, 30.0), (3, 22.5), (5, 15.0), (10, 90.0 / 11.0)] {
        let game = Cournot::new(n, 100.0, 1.0, 10.0, 100.0).unwrap();
        let result = solve(&game);
        assert_eq!(result.status, BrStatus::Converged, "n = {n}");
        assert_eq!(result.profile.len(), n);
        assert_all_within(&result, expected, ANALYTIC_TOL);
        assert!(result.residual <= 1e-12 * 100.0);
    }
}

#[test]
fn iteration_halves_the_damping_for_ten_firms() {
    // alpha = 0.5 needs alpha < 4 / (n + 1): it holds for n <= 6 and fails for n = 10.
    for n in [2, 3, 5] {
        let game = Cournot::new(n, 100.0, 1.0, 10.0, 100.0).unwrap();
        assert_eq!(solve(&game).damping, 0.5, "n = {n}");
    }

    let game = Cournot::new(10, 100.0, 1.0, 10.0, 100.0).unwrap();
    let result = solve(&game);
    assert_eq!(result.status, BrStatus::Converged);
    assert_eq!(result.damping, 0.25);
    // The first attempt used all of its updates before the restart.
    assert!(result.iterations > 10_000);
    assert_all_within(&result, 90.0 / 11.0, ANALYTIC_TOL);

    // Without halving, alpha = 0.5 does not converge.
    let fixed = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new().adaptive(false),
    )
    .unwrap();
    assert_eq!(fixed.status, BrStatus::MaxIter);
    assert_eq!(fixed.damping, 0.5);
    assert_eq!(fixed.iterations, 10_000);
}

#[test]
fn iteration_respects_the_capacity_cap() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 10.0).unwrap();
    let result = solve(&game);
    assert_eq!(result.status, BrStatus::Converged);
    assert_all_within(&result, 10.0, ANALYTIC_TOL);
}

#[test]
fn iteration_finds_the_bertrand_equilibrium() {
    let interior = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    let result = solve(&interior);
    assert_eq!(result.status, BrStatus::Converged);
    assert_all_within(&result, 6.0, ANALYTIC_TOL);

    let capped = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 5.0).unwrap();
    let result = solve(&capped);
    assert_eq!(result.status, BrStatus::Converged);
    assert_all_within(&result, 5.0, ANALYTIC_TOL);
}

#[test]
fn iteration_follows_the_public_goods_regions() {
    for (r, expected) in [(2.0, 0.0), (6.0, 10.0), (0.5, 0.0), (4.0, 0.0)] {
        let game = PublicGoodsContinuous::new(4, 10.0, r).unwrap();
        let result = solve(&game);
        assert_eq!(result.status, BrStatus::Converged, "r = {r}");
        assert_all_within(&result, expected, ANALYTIC_TOL);
    }
}

#[test]
fn golden_section_matches_the_analytic_best_response() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let options = GoldenSectionOptions::new();
    for others in [0.0, 1.0, 5.0, 12.5, 20.0, 22.5, 33.3, 44.0] {
        for role in 0..3 {
            let mut profile = vec![vec![others]; 3];
            profile[role] = vec![71.0];
            let numeric = golden_section_best_response(&game, role, &profile, &options);
            let analytic = game.best_response(role, &profile).unwrap();
            assert_eq!(numeric.status, BrStatus::Converged);
            assert!(numeric.residual <= 1e-12 * 100.0);
            assert!(
                numeric_close(numeric.action[0], analytic[0]),
                "others = {others}: {} vs {}",
                numeric.action[0],
                analytic[0]
            );
        }
    }

    let bertrand = BertrandDifferentiated::new(3, 10.0, 2.0, 1.0, 1.0, 100.0).unwrap();
    let profile = vec![vec![3.0], vec![4.0], vec![9.0]];
    let numeric = golden_section_best_response(&bertrand, 1, &profile, &options);
    let analytic = bertrand.best_response(1, &profile).unwrap();
    assert_eq!(numeric.status, BrStatus::Converged);
    assert!(numeric_close(numeric.action[0], analytic[0]));
}

#[test]
fn golden_section_returns_projected_best_responses_at_the_bounds() {
    // The others flood the market: the projected best response is exactly 0.
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let flooded = vec![vec![0.0], vec![60.0], vec![60.0]];
    let result = golden_section_best_response(&game, 0, &flooded, &GoldenSectionOptions::new());
    assert_eq!(result.status, BrStatus::Converged);
    assert_eq!(result.action, vec![0.0]);

    // The cap binds: the projected best response is exactly q_max.
    let capped = Cournot::new(2, 1000.0, 1.0, 0.0, 100.0).unwrap();
    let result = golden_section_best_response(
        &capped,
        1,
        &[vec![0.0], vec![0.0]],
        &GoldenSectionOptions::new(),
    );
    assert_eq!(result.status, BrStatus::Converged);
    assert_eq!(result.action, vec![100.0]);

    // A payoff that does not depend on the own action: the smallest action.
    let indifferent = PublicGoodsContinuous::new(4, 10.0, 4.0).unwrap();
    let result = golden_section_best_response(
        &indifferent,
        2,
        &vec![vec![7.0]; 4],
        &GoldenSectionOptions::new(),
    );
    assert_eq!(result.action, vec![0.0]);
}

#[test]
fn golden_section_reports_max_iter_and_failure() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let profile = vec![vec![10.0]; 3];
    let short =
        golden_section_best_response(&game, 0, &profile, &GoldenSectionOptions::new().max_iter(5));
    assert_eq!(short.status, BrStatus::MaxIter);

    let unavailable = TwoDimensional::new(NumericBr::Unavailable);
    let profile = vec![vec![0.5, 0.5]; 2];
    let result =
        golden_section_best_response(&unavailable, 0, &profile, &GoldenSectionOptions::new());
    assert_eq!(result.status, BrStatus::Failed);

    // Declared unimodal but two-dimensional: no coordinate-wise search.
    let claimed = TwoDimensional::new(NumericBr::Unimodal1D);
    let result = golden_section_best_response(&claimed, 0, &profile, &GoldenSectionOptions::new());
    assert_eq!(result.status, BrStatus::Failed);
}

#[test]
fn iteration_uses_the_numeric_fallback_for_one_dimensional_games() {
    let game = NumericCournot(Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap());
    assert!(game.best_response(0, &vec![vec![1.0]; 3]).is_none());
    // The numeric best response is resolved to about sqrt(eps) |x|, so the
    // residual tolerance is loosened accordingly.
    let result = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new().tol(1e-8),
    )
    .unwrap();
    assert_eq!(result.status, BrStatus::Converged);
    for action in &result.profile {
        assert!(numeric_close(action[0], 22.5), "{}", action[0]);
    }
}

#[test]
fn multi_dimensional_games_without_best_response_are_rejected() {
    for numeric in [NumericBr::Unavailable, NumericBr::Unimodal1D] {
        let game = TwoDimensional::new(numeric);
        let x0 = vec![vec![0.5, 0.5]; 2];
        assert_eq!(
            best_response_iteration(&game, &x0, &BrIterationOptions::new()),
            Err(SolveError::NoBestResponse { role: 0 })
        );
        assert_eq!(
            best_response_step(&game, &x0, 1.0, None),
            Err(SolveError::NoBestResponse { role: 0 })
        );
    }
}

#[test]
fn an_oracle_supplies_multi_dimensional_best_responses() {
    let game = TwoDimensional::new(NumericBr::Unavailable);
    let oracle = FixedOracle::new(vec![0.25, 0.75], BrStatus::Converged);
    let x0 = vec![vec![0.5, 0.5]; 2];
    let result =
        best_response_iteration(&game, &x0, &BrIterationOptions::new().oracle(&oracle)).unwrap();
    assert_eq!(result.status, BrStatus::Converged);
    for action in &result.profile {
        assert!((action[0] - 0.25).abs() <= ANALYTIC_TOL);
        assert!((action[1] - 0.75).abs() <= ANALYTIC_TOL);
    }

    let step = best_response_step(&game, &x0, 1.0, Some(&oracle)).unwrap();
    assert_eq!(step, vec![vec![0.25, 0.75]; 2]);
}

#[test]
fn an_oracle_takes_precedence_over_the_numeric_search() {
    let game = NumericCournot(Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap());
    let result = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new().oracle(&CournotOracle),
    )
    .unwrap();
    // The exact oracle reaches the default tolerance the numeric search cannot.
    assert_eq!(result.status, BrStatus::Converged);
    assert_all_within(&result, 22.5, ANALYTIC_TOL);
}

#[test]
fn the_analytic_best_response_takes_precedence_over_the_oracle() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let oracle = FixedOracle::new(vec![99.0], BrStatus::Converged);
    let result = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new().oracle(&oracle),
    )
    .unwrap();
    assert_eq!(oracle.calls.load(Ordering::Relaxed), 0);
    assert_all_within(&result, 22.5, ANALYTIC_TOL);
}

#[test]
fn non_converged_best_responses_are_never_used() {
    let game = TwoDimensional::new(NumericBr::Unavailable);
    let x0 = vec![vec![0.5, 0.5]; 2];
    for status in [BrStatus::MaxIter, BrStatus::Failed] {
        // The oracle's action is the true best response, but it is not converged.
        let oracle = FixedOracle::new(vec![0.25, 0.75], status);
        assert_eq!(
            best_response_iteration(&game, &x0, &BrIterationOptions::new().oracle(&oracle)),
            Err(SolveError::NoBestResponse { role: 0 })
        );
        assert_eq!(
            best_response_step(&game, &x0, 0.5, Some(&oracle)),
            Err(SolveError::NoBestResponse { role: 0 })
        );
    }

    // A converged result of the wrong dimension or with a non-finite component
    // is rejected as well.
    for action in [vec![0.25], vec![f64::NAN, 0.75]] {
        let oracle = FixedOracle::new(action, BrStatus::Converged);
        assert_eq!(
            best_response_step(&game, &x0, 0.5, Some(&oracle)),
            Err(SolveError::NoBestResponse { role: 0 })
        );
    }
}

#[test]
fn best_response_step_is_simultaneous_damped_and_projected() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let profile = vec![vec![0.0], vec![50.0], vec![100.0]];
    // Best responses to the same profile: (90 - 150) / 2 -> 0, (90 - 100) / 2 -> 0,
    // (90 - 50) / 2 = 20.
    let undamped = best_response_step(&game, &profile, 1.0, None).unwrap();
    assert_eq!(undamped, vec![vec![0.0], vec![0.0], vec![20.0]]);
    let damped = best_response_step(&game, &profile, 0.5, None).unwrap();
    assert_eq!(damped, vec![vec![0.0], vec![25.0], vec![60.0]]);
}

#[derive(Clone, Copy)]
enum BadBounds {
    Nan,
    Inverted,
    Degenerate,
    Empty,
}

struct BadBoundsGame {
    bounds: Vec<(f64, f64)>,
}

impl BadBoundsGame {
    fn new(kind: BadBounds) -> Self {
        let bounds = match kind {
            BadBounds::Nan => vec![(0.0, f64::NAN)],
            BadBounds::Inverted => vec![(1.0, 0.0)],
            BadBounds::Degenerate => vec![(1.0, 1.0)],
            BadBounds::Empty => Vec::new(),
        };
        Self { bounds }
    }
}

impl StageGame for BadBoundsGame {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, _role: usize, _action: &Vec<f64>) -> Result<(), ActionError> {
        Ok(())
    }
}

impl NormalFormGame for BadBoundsGame {
    fn payoff(&self, _role: usize, _profile: &[Vec<f64>]) -> f64 {
        0.0
    }
}

impl ContinuousGame for BadBoundsGame {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    fn best_response(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        Some(vec![0.5])
    }

    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unimodal1D
    }
}

#[test]
fn invalid_bounds_are_rejected_before_any_projection() {
    for kind in [
        BadBounds::Nan,
        BadBounds::Inverted,
        BadBounds::Degenerate,
        BadBounds::Empty,
    ] {
        let game = BadBoundsGame::new(kind);
        let x0 = vec![vec![0.5]; 2];
        assert!(matches!(
            best_response_iteration(&game, &x0, &BrIterationOptions::new()),
            Err(SolveError::InvalidGame(_))
        ));
        assert!(matches!(
            best_response_step(&game, &x0, 0.5, None),
            Err(SolveError::InvalidGame(_))
        ));
        let result = golden_section_best_response(&game, 0, &x0, &GoldenSectionOptions::new());
        assert_eq!(result.status, BrStatus::Failed);
    }
}

#[test]
fn invalid_options_and_profiles_are_rejected() {
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let x0 = midpoints(&game);
    let invalid = [
        BrIterationOptions::new().damping(0.0),
        BrIterationOptions::new().damping(1.5),
        BrIterationOptions::new().damping(f64::NAN),
        BrIterationOptions::new().min_damping(0.0),
        BrIterationOptions::new().min_damping(0.75),
        BrIterationOptions::new().tol(0.0),
        BrIterationOptions::new().tol(f64::INFINITY),
        BrIterationOptions::new().max_iter(0),
    ];
    for options in &invalid {
        assert!(
            matches!(
                best_response_iteration(&game, &x0, options),
                Err(SolveError::InvalidOptions(_))
            ),
            "{options:?}"
        );
    }
    for damping in [0.0, -0.5, 1.5, f64::NAN] {
        assert!(matches!(
            best_response_step(&game, &x0, damping, None),
            Err(SolveError::InvalidOptions(_))
        ));
    }
    for profile in [
        vec![vec![50.0]; 2],
        vec![vec![50.0], vec![50.0], vec![150.0]],
        vec![vec![50.0], vec![f64::NAN], vec![50.0]],
        vec![vec![50.0], vec![50.0, 1.0], vec![50.0]],
    ] {
        assert!(matches!(
            best_response_iteration(&game, &profile, &BrIterationOptions::new()),
            Err(SolveError::InvalidOptions(_))
        ));
        assert!(matches!(
            best_response_step(&game, &profile, 0.5, None),
            Err(SolveError::InvalidOptions(_))
        ));
    }
}

#[test]
fn the_default_tolerance_is_relaxed_only_on_the_numeric_path() {
    // Numeric path with the default options: the residual tolerance becomes
    // relative 1e-7. The distance to the equilibrium can be up to twice the
    // residual, so the result still agrees with the theory to relative 1e-6.
    for n in [2, 3, 5, 10] {
        let inner = Cournot::new(n, 100.0, 1.0, 10.0, 100.0).unwrap();
        let expected = inner.equilibrium_quantity();
        let game = NumericCournot(inner);
        let result =
            best_response_iteration(&game, &midpoints(&game), &BrIterationOptions::new()).unwrap();
        assert_eq!(result.status, BrStatus::Converged, "n = {n}");
        assert!(result.residual <= 1e-7 * 100.0);
        if n == 10 {
            assert_eq!(result.damping, 0.25);
        }
        for action in &result.profile {
            assert!(numeric_close(action[0], expected), "n = {n}: {}", action[0]);
        }
    }

    // Analytic path with the default options: the tolerance stays 1e-12.
    let game = Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap();
    let result = solve(&game);
    assert_eq!(result.status, BrStatus::Converged);
    assert!(result.residual <= 1e-12 * 100.0);

    // An exact oracle replaces the numeric search, so the tolerance stays 1e-12.
    let game = NumericCournot(Cournot::new(3, 100.0, 1.0, 10.0, 100.0).unwrap());
    let result = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new().oracle(&CournotOracle),
    )
    .unwrap();
    assert!(result.residual <= 1e-12 * 100.0);

    // An explicit tolerance is used as given, even on the numeric path.
    let result = best_response_iteration(
        &game,
        &midpoints(&game),
        &BrIterationOptions::new()
            .tol(1e-12)
            .max_iter(200)
            .adaptive(false),
    )
    .unwrap();
    assert_eq!(result.status, BrStatus::MaxIter);
}
