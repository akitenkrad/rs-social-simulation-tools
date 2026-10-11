use std::borrow::Cow;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    ActionError, AggregateGame, AggregateReduce, AggregateSpec, AnonymousGame, CommonActionSpace,
    FiniteGame, MatrixGame, NormalFormGame, StageGame,
};

macro_rules! impl_two_action_game {
    ($game:ty, $labels:expr, $payoff:expr) => {
        impl StageGame for $game {
            type Action = usize;

            fn n_roles(&self) -> usize {
                2
            }

            fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
                check_catalog_action(role, *action, 2)
            }
        }

        impl NormalFormGame for $game {
            fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
                ($payoff)(self, role, profile[0], profile[1])
            }
        }

        impl FiniteGame for $game {
            fn n_actions(&self, _role: usize) -> usize {
                2
            }

            fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
                Cow::Borrowed($labels[action])
            }

            fn is_symmetric(&self) -> bool {
                (0..2).all(|row| {
                    (0..2).all(|column| {
                        self.payoff(0, &[row, column]) == self.payoff(1, &[column, row])
                    })
                })
            }
        }
    };
}

/// An error constructing a built-in finite game.
#[derive(Clone, Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum CatalogError {
    /// At least one payoff parameter is not finite.
    #[error("non-finite payoff")]
    NonFinite,
    /// The supplied payoffs do not have the defining shape of the game.
    #[error("catalog condition failed: {0}")]
    Condition(String),
}

#[derive(Deserialize)]
struct PrisonersDilemmaRepr {
    temptation: f64,
    reward: f64,
    punishment: f64,
    sucker: f64,
}

/// The two-player prisoner's dilemma with actions `C` and `D`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PrisonersDilemmaRepr")]
pub struct PrisonersDilemma {
    temptation: f64,
    reward: f64,
    punishment: f64,
    sucker: f64,
}

impl TryFrom<PrisonersDilemmaRepr> for PrisonersDilemma {
    type Error = CatalogError;

    fn try_from(value: PrisonersDilemmaRepr) -> Result<Self, Self::Error> {
        Self::new(
            value.temptation,
            value.reward,
            value.punishment,
            value.sucker,
        )
    }
}

impl PrisonersDilemma {
    /// Constructs a game satisfying `T > R > P >= S`.
    pub fn new(
        temptation: f64,
        reward: f64,
        punishment: f64,
        sucker: f64,
    ) -> Result<Self, CatalogError> {
        ensure_finite(&[temptation, reward, punishment, sucker])?;
        if !(temptation > reward && reward > punishment && punishment >= sucker) {
            return Err(CatalogError::Condition("T > R > P >= S".into()));
        }
        Ok(Self {
            temptation,
            reward,
            punishment,
            sucker,
        })
    }

    /// Constructs the weak prisoner's dilemma with `R = 1` and `P = S = 0`.
    pub fn weak(b: f64) -> Result<Self, CatalogError> {
        Self::new(b, 1.0, 0.0, 0.0)
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        matrix_2x2(
            [
                [self.reward, self.sucker],
                [self.temptation, self.punishment],
            ],
            [
                [self.reward, self.temptation],
                [self.sucker, self.punishment],
            ],
            [["C", "D"], ["C", "D"]],
        )
    }
}

impl_two_action_game!(
    PrisonersDilemma,
    ["C", "D"],
    |game: &PrisonersDilemma, role: usize, row: usize, column: usize| {
        if role == 0 {
            [
                [game.reward, game.sucker],
                [game.temptation, game.punishment],
            ][row][column]
        } else {
            [
                [game.reward, game.temptation],
                [game.sucker, game.punishment],
            ][row][column]
        }
    }
);

#[derive(Deserialize)]
struct BimatrixRepr {
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
}

/// The battle of the sexes with actions `B` and `V`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BimatrixRepr")]
pub struct BattleOfSexes {
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
}

impl TryFrom<BimatrixRepr> for BattleOfSexes {
    type Error = CatalogError;

    fn try_from(value: BimatrixRepr) -> Result<Self, Self::Error> {
        Self::new(value.a, value.b)
    }
}

impl BattleOfSexes {
    /// Constructs a battle-of-the-sexes game with two strict coordination equilibria.
    pub fn new(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Result<Self, CatalogError> {
        ensure_bimatrix_finite(&a, &b)?;
        let valid = a[0][0] > a[1][0]
            && a[1][1] > a[0][1]
            && b[0][0] > b[0][1]
            && b[1][1] > b[1][0]
            && a[0][0] > a[1][1]
            && b[1][1] > b[0][0];
        if !valid {
            return Err(CatalogError::Condition(
                "two strict coordination equilibria with opposite preferred outcomes".into(),
            ));
        }
        Ok(Self { a, b })
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        matrix_2x2(self.a, self.b, [["B", "V"], ["B", "V"]])
    }
}

impl Default for BattleOfSexes {
    fn default() -> Self {
        Self {
            a: [[2.0, -1.0], [-1.0, 1.0]],
            b: [[1.0, -1.0], [-1.0, 2.0]],
        }
    }
}

impl_two_action_game!(
    BattleOfSexes,
    ["B", "V"],
    |game: &BattleOfSexes, role: usize, row: usize, column: usize| {
        if role == 0 {
            game.a[row][column]
        } else {
            game.b[row][column]
        }
    }
);

/// Matching pennies with actions `H` and `T`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BimatrixRepr")]
pub struct MatchingPennies {
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
}

impl TryFrom<BimatrixRepr> for MatchingPennies {
    type Error = CatalogError;

    fn try_from(value: BimatrixRepr) -> Result<Self, Self::Error> {
        Self::new(value.a, value.b)
    }
}

impl MatchingPennies {
    /// Constructs a zero-sum two-action game with no pure Nash equilibrium.
    pub fn new(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Result<Self, CatalogError> {
        ensure_bimatrix_finite(&a, &b)?;
        if (0..2).any(|row| (0..2).any(|column| b[row][column] != -a[row][column])) {
            return Err(CatalogError::Condition("zero-sum payoffs".into()));
        }
        let has_pure_equilibrium = (0..2).any(|row| {
            (0..2).any(|column| {
                a[row][column] >= a[1 - row][column] && b[row][column] >= b[row][1 - column]
            })
        });
        if has_pure_equilibrium {
            return Err(CatalogError::Condition("no pure Nash equilibrium".into()));
        }
        Ok(Self { a, b })
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        matrix_2x2(self.a, self.b, [["H", "T"], ["H", "T"]])
    }
}

impl Default for MatchingPennies {
    fn default() -> Self {
        Self {
            a: [[-1.0, 1.0], [1.0, -1.0]],
            b: [[1.0, -1.0], [-1.0, 1.0]],
        }
    }
}

impl_two_action_game!(
    MatchingPennies,
    ["H", "T"],
    |game: &MatchingPennies, role: usize, row: usize, column: usize| {
        if role == 0 {
            game.a[row][column]
        } else {
            game.b[row][column]
        }
    }
);

/// The stag hunt with actions `stag` and `hare`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BimatrixRepr")]
pub struct StagHunt {
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
}

impl TryFrom<BimatrixRepr> for StagHunt {
    type Error = CatalogError;

    fn try_from(value: BimatrixRepr) -> Result<Self, Self::Error> {
        Self::new(value.a, value.b)
    }
}

impl StagHunt {
    /// Constructs a stag hunt with two strict equilibria, where `(stag, stag)`
    /// payoff-dominates `(hare, hare)`.
    pub fn new(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Result<Self, CatalogError> {
        ensure_bimatrix_finite(&a, &b)?;
        let valid = a[0][0] > a[1][0]
            && a[1][1] > a[0][1]
            && b[0][0] > b[0][1]
            && b[1][1] > b[1][0]
            && a[0][0] > a[1][1]
            && b[0][0] > b[1][1];
        if !valid {
            return Err(CatalogError::Condition(
                "two strict equilibria with (stag, stag) payoff-dominant".into(),
            ));
        }
        Ok(Self { a, b })
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        matrix_2x2(self.a, self.b, [["stag", "hare"], ["stag", "hare"]])
    }
}

impl Default for StagHunt {
    fn default() -> Self {
        Self {
            a: [[9.0, 0.0], [8.0, 7.0]],
            b: [[9.0, 8.0], [0.0, 7.0]],
        }
    }
}

impl_two_action_game!(
    StagHunt,
    ["stag", "hare"],
    |game: &StagHunt, role: usize, row: usize, column: usize| {
        if role == 0 {
            game.a[row][column]
        } else {
            game.b[row][column]
        }
    }
);

#[derive(Deserialize)]
struct RockPaperScissorsRepr {
    win: f64,
    loss: f64,
}

/// Rock-paper-scissors with actions `R`, `P`, and `S`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RockPaperScissorsRepr")]
pub struct RockPaperScissors {
    win: f64,
    loss: f64,
}

impl TryFrom<RockPaperScissorsRepr> for RockPaperScissors {
    type Error = CatalogError;

    fn try_from(value: RockPaperScissorsRepr) -> Result<Self, Self::Error> {
        Self::new(value.win, value.loss)
    }
}

impl RockPaperScissors {
    /// Constructs a game whose winner receives `win` and loser receives `-loss`.
    pub fn new(win: f64, loss: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[win, loss])?;
        if !(win > 0.0 && loss > 0.0) {
            return Err(CatalogError::Condition("win > 0 and loss > 0".into()));
        }
        Ok(Self { win, loss })
    }

    fn role_zero(&self) -> [[f64; 3]; 3] {
        [
            [0.0, -self.loss, self.win],
            [self.win, 0.0, -self.loss],
            [-self.loss, self.win, 0.0],
        ]
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        let a = self.role_zero();
        let b = transpose_3x3(a);
        let a = a.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
        let b = b.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
        let Ok(matrix) = MatrixGame::bimatrix(&a, &b) else {
            unreachable!("validated rock-paper-scissors matrix has a fixed finite shape")
        };
        let labels = vec![
            vec!["R".into(), "P".into(), "S".into()],
            vec!["R".into(), "P".into(), "S".into()],
        ];
        let Ok(matrix) = matrix.with_labels(labels) else {
            unreachable!("rock-paper-scissors labels have the fixed action shape")
        };
        matrix
    }
}

impl Default for RockPaperScissors {
    fn default() -> Self {
        Self {
            win: 1.0,
            loss: 1.0,
        }
    }
}

impl StageGame for RockPaperScissors {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        check_catalog_action(role, *action, 3)
    }
}

impl NormalFormGame for RockPaperScissors {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        let a = self.role_zero();
        if role == 0 {
            a[profile[0]][profile[1]]
        } else {
            a[profile[1]][profile[0]]
        }
    }
}

impl FiniteGame for RockPaperScissors {
    fn n_actions(&self, _role: usize) -> usize {
        3
    }

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Borrowed(["R", "P", "S"][action])
    }

    fn is_symmetric(&self) -> bool {
        true
    }
}

impl CommonActionSpace for PrisonersDilemma {}
impl CommonActionSpace for BattleOfSexes {}
impl CommonActionSpace for MatchingPennies {}
impl CommonActionSpace for StagHunt {}
impl CommonActionSpace for RockPaperScissors {}

#[derive(Deserialize)]
struct HawkDoveRepr {
    v: f64,
    c: f64,
}

/// The two-player hawk-dove game with actions `H` and `D`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "HawkDoveRepr")]
pub struct HawkDove {
    v: f64,
    c: f64,
}

impl TryFrom<HawkDoveRepr> for HawkDove {
    type Error = CatalogError;

    fn try_from(value: HawkDoveRepr) -> Result<Self, Self::Error> {
        Self::new(value.v, value.c)
    }
}

impl HawkDove {
    /// Constructs a hawk-dove game with positive value and conflict cost.
    pub fn new(v: f64, c: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[v, c])?;
        if !(v > 0.0 && c > 0.0) {
            return Err(CatalogError::Condition("V > 0 and C > 0".into()));
        }
        Ok(Self { v, c })
    }

    /// Returns the ESS share of hawks.
    pub fn hawk_share_ess(&self) -> f64 {
        if self.v < self.c {
            self.v / self.c
        } else {
            1.0
        }
    }

    /// Converts this game to its matrix representation.
    pub fn to_matrix(&self) -> MatrixGame {
        let a = [[(self.v - self.c) / 2.0, self.v], [0.0, self.v / 2.0]];
        matrix_2x2(
            a,
            [[a[0][0], a[1][0]], [a[0][1], a[1][1]]],
            [["H", "D"], ["H", "D"]],
        )
    }

    fn payoff_for(&self, own: usize, other: usize) -> f64 {
        [[(self.v - self.c) / 2.0, self.v], [0.0, self.v / 2.0]][own][other]
    }
}

impl StageGame for HawkDove {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        check_catalog_action(role, *action, 2)
    }
}

impl NormalFormGame for HawkDove {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        self.payoff_for(profile[role], profile[1 - role])
    }
}

impl FiniteGame for HawkDove {
    fn n_actions(&self, _role: usize) -> usize {
        2
    }

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Borrowed(["H", "D"][action])
    }

    fn is_symmetric(&self) -> bool {
        true
    }
}

impl AnonymousGame for HawkDove {
    fn payoff_by_counts(&self, a: usize, counts_others: &[usize]) -> f64 {
        counts_others
            .iter()
            .enumerate()
            .map(|(other, &count)| count as f64 * self.payoff_for(a, other))
            .sum()
    }
}

impl CommonActionSpace for HawkDove {}

#[derive(Deserialize)]
struct ElFarolRepr {
    n: usize,
    capacity: usize,
}

/// The El Farol attendance game with actions `attend` and `stay`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ElFarolRepr")]
pub struct ElFarol {
    n: usize,
    capacity: usize,
}

impl TryFrom<ElFarolRepr> for ElFarol {
    type Error = CatalogError;

    fn try_from(value: ElFarolRepr) -> Result<Self, Self::Error> {
        Self::new(value.n, value.capacity)
    }
}

impl ElFarol {
    /// Constructs an attendance game with a non-empty, non-universal capacity.
    pub fn new(n: usize, capacity: usize) -> Result<Self, CatalogError> {
        if n < 2 || capacity == 0 || capacity >= n {
            return Err(CatalogError::Condition(
                "n >= 2 and 0 < capacity < n".into(),
            ));
        }
        Ok(Self { n, capacity })
    }
}

impl StageGame for ElFarol {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.n
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        check_population_action(role, *action, self.n)
    }
}

impl NormalFormGame for ElFarol {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        let own = profile[role];
        if own == 1 {
            return 0.0;
        }
        if profile.iter().filter(|&&action| action == 0).count() <= self.capacity {
            1.0
        } else {
            -1.0
        }
    }
}

impl FiniteGame for ElFarol {
    fn n_actions(&self, _role: usize) -> usize {
        2
    }

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Borrowed(["attend", "stay"][action])
    }

    fn is_symmetric(&self) -> bool {
        true
    }
}

impl AnonymousGame for ElFarol {
    fn payoff_by_counts(&self, a: usize, counts_others: &[usize]) -> f64 {
        if a == 1 {
            0.0
        } else if counts_others[0] < self.capacity {
            1.0
        } else {
            -1.0
        }
    }
}

impl AggregateGame for ElFarol {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        attendance_spec()
    }

    fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
        out[0] = profile.iter().filter(|&&action| action == 0).count() as f64;
    }
}

impl CommonActionSpace for ElFarol {}

#[derive(Deserialize)]
struct MinorityGameRepr {
    n: usize,
}

/// An odd-player minority game with two unlabeled sides.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MinorityGameRepr")]
pub struct MinorityGame {
    n: usize,
}

impl TryFrom<MinorityGameRepr> for MinorityGame {
    type Error = CatalogError;

    fn try_from(value: MinorityGameRepr) -> Result<Self, Self::Error> {
        Self::new(value.n)
    }
}

impl MinorityGame {
    /// Constructs a two-action minority game for an odd population.
    pub fn new(n: usize) -> Result<Self, CatalogError> {
        if n < 3 || n.is_multiple_of(2) {
            return Err(CatalogError::Condition("n >= 3 and n is odd".into()));
        }
        Ok(Self { n })
    }
}

impl StageGame for MinorityGame {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.n
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        check_population_action(role, *action, self.n)
    }
}

impl NormalFormGame for MinorityGame {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        let own = profile[role];
        let own_count = profile.iter().filter(|&&action| action == own).count();
        if own_count < self.n - own_count {
            1.0
        } else {
            0.0
        }
    }
}

impl FiniteGame for MinorityGame {
    fn n_actions(&self, _role: usize) -> usize {
        2
    }

    fn is_symmetric(&self) -> bool {
        true
    }
}

impl AnonymousGame for MinorityGame {
    fn payoff_by_counts(&self, a: usize, counts_others: &[usize]) -> f64 {
        if counts_others[a] + 1 < counts_others[1 - a] {
            1.0
        } else {
            0.0
        }
    }
}

impl AggregateGame for MinorityGame {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        attendance_spec()
    }

    fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
        out[0] = profile.iter().filter(|&&action| action == 0).count() as f64;
    }
}

impl CommonActionSpace for MinorityGame {}

#[derive(Deserialize)]
struct PublicGoodsFiniteRepr {
    k: usize,
    r: f64,
    cost: f64,
}

/// A finite public-goods game with actions `C` and `D`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PublicGoodsFiniteRepr")]
pub struct PublicGoodsFinite {
    k: usize,
    r: f64,
    cost: f64,
}

impl TryFrom<PublicGoodsFiniteRepr> for PublicGoodsFinite {
    type Error = CatalogError;

    fn try_from(value: PublicGoodsFiniteRepr) -> Result<Self, Self::Error> {
        Self::new(value.k, value.r, value.cost)
    }
}

impl PublicGoodsFinite {
    /// Constructs a finite public-goods game.
    pub fn new(k: usize, r: f64, cost: f64) -> Result<Self, CatalogError> {
        ensure_finite(&[r, cost])?;
        if k < 2 || !(r > 0.0 && cost > 0.0) {
            return Err(CatalogError::Condition(
                "k >= 2, r > 0, and cost > 0".into(),
            ));
        }
        Ok(Self { k, r, cost })
    }

    fn payoff_with_contributors(&self, action: usize, contributors: usize) -> f64 {
        let share = self.r * self.cost * contributors as f64 / self.k as f64;
        if action == 0 {
            share - self.cost
        } else {
            share
        }
    }
}

impl StageGame for PublicGoodsFinite {
    type Action = usize;

    fn n_roles(&self) -> usize {
        self.k
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        check_population_action(role, *action, self.k)
    }
}

impl NormalFormGame for PublicGoodsFinite {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        self.payoff_with_contributors(
            profile[role],
            profile.iter().filter(|&&action| action == 0).count(),
        )
    }
}

impl FiniteGame for PublicGoodsFinite {
    fn n_actions(&self, _role: usize) -> usize {
        2
    }

    fn action_label(&self, _role: usize, action: usize) -> Cow<'_, str> {
        Cow::Borrowed(["C", "D"][action])
    }

    fn is_symmetric(&self) -> bool {
        true
    }
}

impl AnonymousGame for PublicGoodsFinite {
    fn payoff_by_counts(&self, a: usize, counts_others: &[usize]) -> f64 {
        self.payoff_with_contributors(a, counts_others[0] + usize::from(a == 0))
    }
}

impl CommonActionSpace for PublicGoodsFinite {}

fn attendance_spec() -> &'static [AggregateSpec] {
    static SPEC: OnceLock<[AggregateSpec; 1]> = OnceLock::new();
    SPEC.get_or_init(|| {
        let Ok(spec) = AggregateSpec::new("attendance", AggregateReduce::Sum) else {
            unreachable!("the built-in attendance aggregate name is valid")
        };
        [spec]
    })
}

fn ensure_finite(values: &[f64]) -> Result<(), CatalogError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(CatalogError::NonFinite)
    }
}

fn ensure_bimatrix_finite(a: &[[f64; 2]; 2], b: &[[f64; 2]; 2]) -> Result<(), CatalogError> {
    if a.iter()
        .flatten()
        .chain(b.iter().flatten())
        .all(|x| x.is_finite())
    {
        Ok(())
    } else {
        Err(CatalogError::NonFinite)
    }
}

fn check_catalog_action(role: usize, action: usize, n_actions: usize) -> Result<(), ActionError> {
    if role >= 2 {
        return Err(ActionError::InvalidRole { role });
    }
    if action >= n_actions {
        return Err(ActionError::InvalidAction { role, action });
    }
    Ok(())
}

fn check_population_action(role: usize, action: usize, n_roles: usize) -> Result<(), ActionError> {
    if role >= n_roles {
        return Err(ActionError::InvalidRole { role });
    }
    if action >= 2 {
        return Err(ActionError::InvalidAction { role, action });
    }
    Ok(())
}

fn matrix_2x2(a: [[f64; 2]; 2], b: [[f64; 2]; 2], labels: [[&str; 2]; 2]) -> MatrixGame {
    let a = a.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
    let b = b.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
    let Ok(matrix) = MatrixGame::bimatrix(&a, &b) else {
        unreachable!("validated catalog bimatrix has a fixed finite shape")
    };
    let labels = labels
        .into_iter()
        .map(|role| role.into_iter().map(String::from).collect())
        .collect();
    let Ok(matrix) = matrix.with_labels(labels) else {
        unreachable!("catalog labels have the fixed action shape")
    };
    matrix
}

fn transpose_3x3(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|row| std::array::from_fn(|column| matrix[column][row]))
}
