//! Memory-one and trigger automata for two-action repeated games.

use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::{sample_probabilities, LearnError};
use crate::FiniteGame;

/// A built-in repeated-game automaton.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomatonKind {
    AllC,
    AllD,
    Grim,
    Wsls,
    Tft,
    Gtft { generosity: f64 },
}

/// One automaton's persistent state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomatonState {
    kind: AutomatonKind,
    coop: usize,
    grim_triggered: bool,
    own_last: Option<usize>,
    opponent_last: Option<usize>,
}

impl AutomatonState {
    /// Constructs an automaton with a fixed cooperation action index.
    pub fn new(kind: AutomatonKind, coop: usize) -> Result<Self, LearnError> {
        if coop >= 2 {
            return Err(LearnError::InvalidCooperationAction { coop });
        }
        if let AutomatonKind::Gtft { generosity } = &kind {
            if !generosity.is_finite() || !(0.0..=1.0).contains(generosity) {
                return Err(LearnError::InvalidParameter {
                    parameter: "generosity",
                    value: *generosity,
                });
            }
        }
        Ok(Self {
            kind,
            coop,
            grim_triggered: false,
            own_last: None,
            opponent_last: None,
        })
    }

    /// Returns the automaton kind.
    pub fn kind(&self) -> &AutomatonKind {
        &self.kind
    }

    /// Returns the fixed cooperation action index.
    pub fn coop(&self) -> usize {
        self.coop
    }

    /// Selects the next action.
    ///
    /// Every kind initially cooperates except `AllD`. WSLS follows this table,
    /// where `C` is `coop` and `D` is `1 - coop`:
    ///
    /// | previous `(own, opponent)` | next |
    /// | --- | --- |
    /// | `(C, C)` | `C` |
    /// | `(C, D)` | `D` |
    /// | `(D, C)` | `D` |
    /// | `(D, D)` | `C` |
    ///
    /// GTFT uses [`super::sample_probabilities`] after an opponent defection,
    /// except at generosity zero or one, where the result is deterministic and
    /// no randomness is consumed. Other automata never consume randomness.
    pub fn next_action(&self, rng: &mut dyn RngCore) -> Result<usize, LearnError> {
        let defect = 1 - self.coop;
        match self.kind {
            AutomatonKind::AllC => Ok(self.coop),
            AutomatonKind::AllD => Ok(defect),
            AutomatonKind::Grim => Ok(if self.grim_triggered {
                defect
            } else {
                self.coop
            }),
            AutomatonKind::Wsls => match (self.own_last, self.opponent_last) {
                (Some(own), Some(opponent)) if opponent == self.coop => Ok(own),
                (Some(own), Some(_)) => Ok(if own == self.coop { defect } else { self.coop }),
                _ => Ok(self.coop),
            },
            AutomatonKind::Tft => Ok(self.opponent_last.unwrap_or(self.coop)),
            AutomatonKind::Gtft { generosity } => match self.opponent_last {
                None => Ok(self.coop),
                Some(action) if action == self.coop => Ok(self.coop),
                Some(_) if generosity == 0.0 => Ok(defect),
                Some(_) if generosity == 1.0 => Ok(self.coop),
                Some(_) => {
                    let mut probabilities = [0.0; 2];
                    probabilities[self.coop] = generosity;
                    probabilities[defect] = 1.0 - generosity;
                    sample_probabilities(&probabilities, rng)
                }
            },
        }
    }

    /// Records the previous action pair for the next transition.
    pub fn observe(&mut self, own_action: usize, opponent_action: usize) -> Result<(), LearnError> {
        for action in [own_action, opponent_action] {
            if action >= 2 {
                return Err(LearnError::InvalidAction {
                    action,
                    n_actions: 2,
                });
            }
        }
        if opponent_action != self.coop {
            self.grim_triggered = true;
        }
        self.own_last = Some(own_action);
        self.opponent_last = Some(opponent_action);
        Ok(())
    }
}

/// Requires every role in a finite game to have exactly two actions.
pub fn validate_two_action_game<G: FiniteGame>(game: &G) -> Result<(), LearnError> {
    for role in 0..game.n_roles() {
        let actual = game.n_actions(role);
        if actual != 2 {
            return Err(LearnError::InvalidActionCount {
                role,
                expected: 2,
                actual,
            });
        }
    }
    Ok(())
}
