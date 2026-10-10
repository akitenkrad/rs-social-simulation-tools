use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ActionError, FiniteGame, MatrixGame, NormalFormGame, StageGame};

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
