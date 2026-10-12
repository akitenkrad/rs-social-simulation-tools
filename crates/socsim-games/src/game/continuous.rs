//! Built-in games with continuous actions.
//!
//! Every game here has one-dimensional actions, a closed interval as its
//! bounds, an analytic best response, and an analytic payoff gradient.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::catalog::CatalogError;
use super::{
    ActionError, AggregateGame, AggregateReduce, AggregateSpec, CommonActionSpace, ContinuousGame,
    DescribeGame, NormalFormGame, NumericBr, StageGame,
};

/// Checks a continuous action against the bounds of `role`.
///
/// The checks run in a fixed order and the first violation is returned:
/// the role must exist ([`ActionError::InvalidRole`]), the action must have
/// `bounds(role).len()` components ([`ActionError::DimensionMismatch`]), every
/// component must be finite ([`ActionError::NonFiniteComponent`]), and every
/// component must lie in its closed interval ([`ActionError::OutOfBounds`]).
pub fn check_continuous_action<G: ContinuousGame>(
    game: &G,
    role: usize,
    action: &[f64],
) -> Result<(), ActionError> {
    if role >= game.n_roles() {
        return Err(ActionError::InvalidRole { role });
    }
    let bounds = game.bounds(role);
    if action.len() != bounds.len() {
        return Err(ActionError::DimensionMismatch {
            role,
            expected: bounds.len(),
            actual: action.len(),
        });
    }
    if let Some(dim) = action.iter().position(|value| !value.is_finite()) {
        return Err(ActionError::NonFiniteComponent { role, dim });
    }
    if let Some(dim) = action
        .iter()
        .zip(bounds)
        .position(|(&value, &(lo, hi))| !(lo <= value && value <= hi))
    {
        return Err(ActionError::OutOfBounds { role, dim });
    }
    Ok(())
}

/// A violation of the contract on [`ContinuousGame::bounds`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundsViolation {
    /// The game has no roles.
    NoRoles,
    /// The bounds of `role` have no dimension.
    Empty { role: usize },
    /// An end point of dimension `dim` of `role` is NaN or infinite.
    NonFinite { role: usize, dim: usize },
    /// Dimension `dim` of `role` does not satisfy `lo < hi`.
    EmptyInterval { role: usize, dim: usize },
}

impl std::fmt::Display for BoundsViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRoles => write!(f, "the game has no roles"),
            Self::Empty { role } => write!(f, "the bounds of role {role} have no dimension"),
            Self::NonFinite { role, dim } => write!(
                f,
                "dimension {dim} of the bounds of role {role} has a non-finite end point"
            ),
            Self::EmptyInterval { role, dim } => write!(
                f,
                "dimension {dim} of the bounds of role {role} does not satisfy lo < hi"
            ),
        }
    }
}

/// Checks that every role has at least one dimension and that every dimension
/// has finite end points with `lo < hi`.
///
/// Projections with `f64::clamp` panic on NaN or inverted end points, so every
/// entry point that projects onto the bounds runs this check first.
pub(crate) fn check_bounds<G: ContinuousGame>(game: &G) -> Result<(), BoundsViolation> {
    let n_roles = game.n_roles();
    if n_roles == 0 {
        return Err(BoundsViolation::NoRoles);
    }
    for role in 0..n_roles {
        let bounds = game.bounds(role);
        if bounds.is_empty() {
            return Err(BoundsViolation::Empty { role });
        }
        for (dim, &(lo, hi)) in bounds.iter().enumerate() {
            if !lo.is_finite() || !hi.is_finite() {
                return Err(BoundsViolation::NonFinite { role, dim });
            }
            if lo >= hi {
                return Err(BoundsViolation::EmptyInterval { role, dim });
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct CournotRepr {
    n: usize,
    a: f64,
    b: f64,
    c: f64,
    q_max: f64,
}

/// Cournot quantity competition with linear inverse demand.
///
/// Each of `n` firms chooses a quantity `q_i` in `[0, q_max]`. The market price
/// is `P = a - b Q` with `Q` the total quantity (the price is not truncated at
/// zero), and firm `i` earns `q_i (P - c)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CournotRepr")]
pub struct Cournot {
    n: usize,
    a: f64,
    b: f64,
    c: f64,
    q_max: f64,
    #[serde(skip)]
    bounds: [(f64, f64); 1],
}

impl TryFrom<CournotRepr> for Cournot {
    type Error = CatalogError;

    fn try_from(value: CournotRepr) -> Result<Self, Self::Error> {
        Self::new(value.n, value.a, value.b, value.c, value.q_max)
    }
}

impl Cournot {
    /// Constructs a Cournot game satisfying `n >= 2`, `b > 0`, `a > c >= 0`,
    /// and `q_max > 0`, with all parameters finite.
    pub fn new(n: usize, a: f64, b: f64, c: f64, q_max: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[a, b, c, q_max])?;
        if !(n >= 2 && b > 0.0 && a > c && c >= 0.0 && q_max > 0.0) {
            return Err(CatalogError::Condition(
                "n >= 2, b > 0, a > c >= 0, and q_max > 0".into(),
            ));
        }
        Ok(Self {
            n,
            a,
            b,
            c,
            q_max,
            bounds: [(0.0, q_max)],
        })
    }

    /// Returns the quantity of the unique symmetric equilibrium,
    /// `min((a - c) / ((n + 1) b), q_max)`.
    pub fn equilibrium_quantity(&self) -> f64 {
        ((self.a - self.c) / ((self.n as f64 + 1.0) * self.b)).min(self.q_max)
    }

    fn total(profile: &[Vec<f64>]) -> f64 {
        profile.iter().map(|action| action[0]).sum()
    }

    fn others(role: usize, profile: &[Vec<f64>]) -> f64 {
        profile
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != role)
            .map(|(_, action)| action[0])
            .sum()
    }
}

impl StageGame for Cournot {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        self.n
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for Cournot {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let price = self.a - self.b * Self::total(profile);
        profile[role][0] * (price - self.c)
    }

    fn payoffs_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        let price = self.a - self.b * Self::total(profile);
        for (value, action) in out.iter_mut().zip(profile).take(self.n) {
            *value = action[0] * (price - self.c);
        }
    }
}

impl ContinuousGame for Cournot {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    /// Projects the unconstrained best response `(a - c - b Q_{-i}) / (2b)`
    /// onto `[0, q_max]`. `profile[role]` is not read.
    fn best_response(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        let others = Self::others(role, profile);
        let unconstrained = (self.a - self.c - self.b * others) / (2.0 * self.b);
        Some(vec![unconstrained.clamp(0.0, self.q_max)])
    }

    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unimodal1D
    }

    fn gradient(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        let own = profile[role][0];
        let total = Self::total(profile);
        Some(vec![self.a - self.c - self.b * total - self.b * own])
    }
}

impl AggregateGame for Cournot {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        static SPEC: OnceLock<[AggregateSpec; 2]> = OnceLock::new();
        SPEC.get_or_init(|| {
            [
                builtin_spec("total_output", AggregateReduce::Sum),
                builtin_spec("price", AggregateReduce::Mean),
            ]
        })
    }

    fn aggregate_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        let total = Self::total(profile);
        out[0] = total;
        out[1] = self.a - self.b * total;
    }
}

impl DescribeGame for Cournot {
    fn describe(&self, role: Option<usize>) -> String {
        let mut text = format!(
            "Cournot competition among {} firms. Each firm chooses a quantity in [0, {}]. \
             The market price is {} - {} * Q, where Q is the total quantity of all firms; \
             the price can become negative. A firm producing quantity q earns q * (price - {}).",
            self.n,
            number(self.q_max),
            number(self.a),
            number(self.b),
            number(self.c),
        );
        push_role(&mut text, "firm", role);
        text
    }
}

impl CommonActionSpace for Cournot {}

#[derive(Deserialize)]
struct BertrandDifferentiatedRepr {
    n: usize,
    a: f64,
    b: f64,
    d: f64,
    c: f64,
    p_max: f64,
}

/// Price competition with differentiated products and linear demand.
///
/// Each of `n` firms chooses a price `p_i` in `[0, p_max]`. Firm `i` sells
/// `q_i = a - b p_i + d * sum_{j != i} p_j` (the demand is not truncated at
/// zero) and earns `(p_i - c) q_i`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BertrandDifferentiatedRepr")]
pub struct BertrandDifferentiated {
    n: usize,
    a: f64,
    b: f64,
    d: f64,
    c: f64,
    p_max: f64,
    #[serde(skip)]
    bounds: [(f64, f64); 1],
}

impl TryFrom<BertrandDifferentiatedRepr> for BertrandDifferentiated {
    type Error = CatalogError;

    fn try_from(value: BertrandDifferentiatedRepr) -> Result<Self, Self::Error> {
        Self::new(value.n, value.a, value.b, value.d, value.c, value.p_max)
    }
}

impl BertrandDifferentiated {
    /// Constructs a differentiated Bertrand game satisfying `n >= 2`, `b > 0`,
    /// `d >= 0`, `2b > (n - 1) d`, `c >= 0`, and `p_max > 0`, with all
    /// parameters finite.
    pub fn new(n: usize, a: f64, b: f64, d: f64, c: f64, p_max: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[a, b, d, c, p_max])?;
        let valid = n >= 2
            && b > 0.0
            && d >= 0.0
            && 2.0 * b > (n as f64 - 1.0) * d
            && c >= 0.0
            && p_max > 0.0;
        if !valid {
            return Err(CatalogError::Condition(
                "n >= 2, b > 0, d >= 0, 2b > (n - 1) d, c >= 0, and p_max > 0".into(),
            ));
        }
        Ok(Self {
            n,
            a,
            b,
            d,
            c,
            p_max,
            bounds: [(0.0, p_max)],
        })
    }

    /// Returns the price of the symmetric equilibrium.
    ///
    /// The interior value is `p* = (a + b c) / (2b - (n - 1) d)`. When
    /// `p* < c` the equilibrium demand would be negative, which lies outside
    /// the scope of this model, and `None` is returned. Otherwise the result is
    /// `min(p*, p_max)`.
    pub fn equilibrium_price(&self) -> Option<f64> {
        let interior = (self.a + self.b * self.c) / (2.0 * self.b - (self.n as f64 - 1.0) * self.d);
        if interior < self.c {
            None
        } else {
            Some(interior.min(self.p_max))
        }
    }

    fn others(role: usize, profile: &[Vec<f64>]) -> f64 {
        profile
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != role)
            .map(|(_, action)| action[0])
            .sum()
    }
}

impl StageGame for BertrandDifferentiated {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        self.n
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for BertrandDifferentiated {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        let own = profile[role][0];
        let demand = self.a - self.b * own + self.d * Self::others(role, profile);
        (own - self.c) * demand
    }
}

impl ContinuousGame for BertrandDifferentiated {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    /// Projects the unconstrained best response
    /// `(a + b c + d * sum_{j != i} p_j) / (2b)` onto `[0, p_max]`.
    /// `profile[role]` is not read.
    fn best_response(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        let others = Self::others(role, profile);
        let unconstrained = (self.a + self.b * self.c + self.d * others) / (2.0 * self.b);
        Some(vec![unconstrained.clamp(0.0, self.p_max)])
    }

    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unimodal1D
    }

    fn gradient(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        let own = profile[role][0];
        let others = Self::others(role, profile);
        Some(vec![
            self.a - 2.0 * self.b * own + self.b * self.c + self.d * others,
        ])
    }
}

impl AggregateGame for BertrandDifferentiated {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        static SPEC: OnceLock<[AggregateSpec; 2]> = OnceLock::new();
        SPEC.get_or_init(|| {
            [
                builtin_spec("mean_price", AggregateReduce::Mean),
                builtin_spec("total_demand", AggregateReduce::Sum),
            ]
        })
    }

    fn aggregate_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        let total_price: f64 = profile.iter().map(|action| action[0]).sum();
        out[0] = total_price / profile.len() as f64;
        out[1] = (0..profile.len())
            .map(|role| self.a - self.b * profile[role][0] + self.d * Self::others(role, profile))
            .sum();
    }
}

impl DescribeGame for BertrandDifferentiated {
    fn describe(&self, role: Option<usize>) -> String {
        let mut text = format!(
            "Price competition among {} firms selling differentiated products. Each firm \
             chooses a price in [0, {}]. A firm charging price p sells {} - {} * p + {} * S \
             units, where S is the sum of the other firms' prices; the demand can become \
             negative. The firm earns (p - {}) times its demand.",
            self.n,
            number(self.p_max),
            number(self.a),
            number(self.b),
            number(self.d),
            number(self.c),
        );
        push_role(&mut text, "firm", role);
        text
    }
}

impl CommonActionSpace for BertrandDifferentiated {}

#[derive(Deserialize)]
struct PublicGoodsContinuousRepr {
    k: usize,
    endowment: f64,
    r: f64,
}

/// A linear public-goods game with continuous contributions.
///
/// Each of `k` players contributes `x_i` in `[0, endowment]` and earns
/// `endowment - x_i + (r / k) * sum_j x_j`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PublicGoodsContinuousRepr")]
pub struct PublicGoodsContinuous {
    k: usize,
    endowment: f64,
    r: f64,
    #[serde(skip)]
    bounds: [(f64, f64); 1],
}

impl TryFrom<PublicGoodsContinuousRepr> for PublicGoodsContinuous {
    type Error = CatalogError;

    fn try_from(value: PublicGoodsContinuousRepr) -> Result<Self, Self::Error> {
        Self::new(value.k, value.endowment, value.r)
    }
}

impl PublicGoodsContinuous {
    /// Constructs a public-goods game satisfying `k >= 2`, `endowment > 0`, and
    /// `r >= 0`, with all parameters finite.
    pub fn new(k: usize, endowment: f64, r: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[endowment, r])?;
        if !(k >= 2 && endowment > 0.0 && r >= 0.0) {
            return Err(CatalogError::Condition(
                "k >= 2, endowment > 0, and r >= 0".into(),
            ));
        }
        Ok(Self {
            k,
            endowment,
            r,
            bounds: [(0.0, endowment)],
        })
    }

    /// Constructs a social dilemma, which additionally requires `1 < r < k`:
    /// contributing nothing is dominant while full contribution is socially
    /// optimal.
    pub fn social_dilemma(k: usize, endowment: f64, r: f64) -> Result<Self, CatalogError> {
        let game = Self::new(k, endowment, r)?;
        if !(1.0 < r && r < k as f64) {
            return Err(CatalogError::Condition("1 < r < k".into()));
        }
        Ok(game)
    }

    /// Returns the equilibrium contribution: `0` when `r < k` and the whole
    /// endowment when `r > k` (both dominant strategies). When `r = k` every
    /// contribution is a best response and `None` is returned.
    pub fn equilibrium_contribution(&self) -> Option<f64> {
        let k = self.k as f64;
        if self.r < k {
            Some(0.0)
        } else if self.r > k {
            Some(self.endowment)
        } else {
            None
        }
    }

    /// Returns the contribution that maximizes the total payoff
    /// `k * endowment + (r - 1) * sum_j x_j`: the whole endowment when `r > 1`
    /// and `0` when `r < 1`. When `r = 1` the total payoff does not depend on
    /// the contributions and `None` is returned.
    pub fn social_optimum_contribution(&self) -> Option<f64> {
        if self.r > 1.0 {
            Some(self.endowment)
        } else if self.r < 1.0 {
            Some(0.0)
        } else {
            None
        }
    }

    fn share(&self, profile: &[Vec<f64>]) -> f64 {
        let total: f64 = profile.iter().map(|action| action[0]).sum();
        self.r / self.k as f64 * total
    }
}

impl StageGame for PublicGoodsContinuous {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        self.k
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        check_continuous_action(self, role, action)
    }
}

impl NormalFormGame for PublicGoodsContinuous {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        self.endowment - profile[role][0] + self.share(profile)
    }

    fn payoffs_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        let share = self.share(profile);
        for (value, action) in out.iter_mut().zip(profile).take(self.k) {
            *value = self.endowment - action[0] + share;
        }
    }
}

impl ContinuousGame for PublicGoodsContinuous {
    fn bounds(&self, _role: usize) -> &[(f64, f64)] {
        &self.bounds
    }

    /// Returns `0` when `r <= k` and the whole endowment when `r > k`. When
    /// `r = k` every contribution is a best response and the smallest one is
    /// returned. `profile` is not read.
    fn best_response(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        if self.r > self.k as f64 {
            Some(vec![self.endowment])
        } else {
            Some(vec![0.0])
        }
    }

    fn numeric_br(&self) -> NumericBr {
        NumericBr::Unimodal1D
    }

    fn gradient(&self, _role: usize, _profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        Some(vec![self.r / self.k as f64 - 1.0])
    }
}

impl AggregateGame for PublicGoodsContinuous {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        static SPEC: OnceLock<[AggregateSpec; 1]> = OnceLock::new();
        SPEC.get_or_init(|| [builtin_spec("total_contribution", AggregateReduce::Sum)])
    }

    fn aggregate_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        out[0] = profile.iter().map(|action| action[0]).sum();
    }
}

impl DescribeGame for PublicGoodsContinuous {
    fn describe(&self, role: Option<usize>) -> String {
        let mut text = format!(
            "Public-goods game among {} players. Each player receives an endowment of {} and \
             contributes an amount in [0, {}] to a common pool. The pool is multiplied by {} \
             and shared equally among all {} players. A player contributing x earns \
             {} - x + ({} / {}) * T, where T is the total contribution of all players.",
            self.k,
            number(self.endowment),
            number(self.endowment),
            number(self.r),
            self.k,
            number(self.endowment),
            number(self.r),
            self.k,
        );
        push_role(&mut text, "player", role);
        text
    }
}

impl CommonActionSpace for PublicGoodsContinuous {}

fn builtin_spec(name: &str, reduce: AggregateReduce) -> AggregateSpec {
    let Ok(spec) = AggregateSpec::new(name, reduce) else {
        unreachable!("built-in aggregate names are valid")
    };
    spec
}

fn ensure_finite(values: &[f64]) -> Result<(), CatalogError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(CatalogError::NonFinite)
    }
}

fn push_role(text: &mut String, noun: &str, role: Option<usize>) {
    if let Some(role) = role {
        text.push_str(&format!(" You are {noun} {role}."));
    }
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}
