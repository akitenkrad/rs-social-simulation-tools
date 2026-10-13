//! Run-time selectable built-in games.
//!
//! [`FiniteSpec`] and [`ContinuousSpec`] wrap the built-in games (and
//! [`MatrixGame`]) in one enum each, so that a game chosen at run time, for
//! example from the `kind` of a scenario file, has a single concrete type.
//! Every game trait is implemented by forwarding every method, including the
//! ones with a default body, to the wrapped game, so a spec returns exactly the
//! same payoffs, labels, bounds, best responses, gradients, and aggregates as
//! the game it wraps.
//!
//! The specs do not implement [`AnonymousGame`](super::AnonymousGame): not
//! every wrapped game is anonymous, and the counting play path needs the
//! property for the whole type.
//!
//! # Diagnostics
//!
//! Every trait implementation in this module carries
//! `#[diagnostic::do_not_recommend]`. The specs are wrappers, not games of
//! their own, so the compiler does not list them among the types that
//! implement a game trait when it reports a missing implementation.
//!
//! For the same reason, no variant shares its name with a game type: a public
//! variant named `HawkDove` would make the name `HawkDove` ambiguous, and
//! compiler diagnostics would then print the full path of the built-in game
//! (`socsim_games::catalog::HawkDove`) instead of its short name. Variants are
//! therefore named after the wrapped type with a `Game` suffix
//! ([`FiniteSpec::HawkDoveGame`]), or without one when the type name already
//! ends in `Game` ([`FiniteSpec::Minority`] wraps [`MinorityGame`]).

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::catalog::{
    BattleOfSexes, ElFarol, HawkDove, MatchingPennies, MinorityGame, PrisonersDilemma,
    PublicGoodsFinite, RockPaperScissors, StagHunt,
};
use super::continuous::{BertrandDifferentiated, Cournot, PublicGoodsContinuous};
use super::{
    ActionError, AggregateGame, AggregateSpec, CommonActionSpace, ContinuousGame, DescribeGame,
    FiniteGame, MatrixGame, NormalFormGame, NumericBr, StageGame,
};

/// A finite game selected at run time.
///
/// Each variant wraps one built-in finite game or a [`MatrixGame`]. The
/// implementations of [`StageGame`], [`NormalFormGame`], [`FiniteGame`],
/// [`CommonActionSpace`], and [`AggregateGame`] forward every method to the
/// wrapped game. A variant whose game has no public aggregates reports an
/// empty [`AggregateGame::aggregate_spec`] and writes nothing in
/// [`AggregateGame::aggregate_into`].
///
/// Serialized with serde's default externally tagged representation, for
/// example `{"HawkDoveGame": {"v": 2.0, "c": 3.0}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FiniteSpec {
    /// A game given by its payoff matrices.
    Matrix(MatrixGame),
    /// The prisoner's dilemma.
    PrisonersDilemmaGame(PrisonersDilemma),
    /// The battle of the sexes.
    BattleOfSexesGame(BattleOfSexes),
    /// Matching pennies.
    MatchingPenniesGame(MatchingPennies),
    /// Rock-paper-scissors.
    RockPaperScissorsGame(RockPaperScissors),
    /// The stag hunt.
    StagHuntGame(StagHunt),
    /// The hawk-dove game.
    HawkDoveGame(HawkDove),
    /// The El Farol bar problem.
    ElFarolGame(ElFarol),
    /// The minority game.
    Minority(MinorityGame),
    /// The finite public-goods game.
    PublicGoodsFiniteGame(PublicGoodsFinite),
}

/// A continuous game selected at run time.
///
/// Each variant wraps one built-in continuous game. The implementations of
/// [`StageGame`], [`NormalFormGame`], [`ContinuousGame`],
/// [`CommonActionSpace`], [`AggregateGame`], and [`DescribeGame`] forward every
/// method to the wrapped game.
///
/// Serialized with serde's default externally tagged representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ContinuousSpec {
    /// Cournot quantity competition.
    CournotGame(Cournot),
    /// Differentiated Bertrand price competition.
    BertrandDifferentiatedGame(BertrandDifferentiated),
    /// The continuous public-goods game.
    PublicGoodsContinuousGame(PublicGoodsContinuous),
}

macro_rules! finite {
    ($spec:expr, $game:ident => $body:expr) => {
        match $spec {
            FiniteSpec::Matrix($game) => $body,
            FiniteSpec::PrisonersDilemmaGame($game) => $body,
            FiniteSpec::BattleOfSexesGame($game) => $body,
            FiniteSpec::MatchingPenniesGame($game) => $body,
            FiniteSpec::RockPaperScissorsGame($game) => $body,
            FiniteSpec::StagHuntGame($game) => $body,
            FiniteSpec::HawkDoveGame($game) => $body,
            FiniteSpec::ElFarolGame($game) => $body,
            FiniteSpec::Minority($game) => $body,
            FiniteSpec::PublicGoodsFiniteGame($game) => $body,
        }
    };
}

macro_rules! continuous {
    ($spec:expr, $game:ident => $body:expr) => {
        match $spec {
            ContinuousSpec::CournotGame($game) => $body,
            ContinuousSpec::BertrandDifferentiatedGame($game) => $body,
            ContinuousSpec::PublicGoodsContinuousGame($game) => $body,
        }
    };
}

#[diagnostic::do_not_recommend]
impl StageGame for FiniteSpec {
    type Action = usize;

    fn n_roles(&self) -> usize {
        finite!(self, game => game.n_roles())
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        finite!(self, game => game.check_action(role, action))
    }
}

#[diagnostic::do_not_recommend]
impl NormalFormGame for FiniteSpec {
    fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
        finite!(self, game => game.payoff(role, profile))
    }

    fn payoffs_into(&self, profile: &[usize], out: &mut [f64]) {
        finite!(self, game => game.payoffs_into(profile, out))
    }
}

#[diagnostic::do_not_recommend]
impl FiniteGame for FiniteSpec {
    fn n_actions(&self, role: usize) -> usize {
        finite!(self, game => game.n_actions(role))
    }

    fn action_label(&self, role: usize, action: usize) -> Cow<'_, str> {
        finite!(self, game => game.action_label(role, action))
    }

    fn is_symmetric(&self) -> bool {
        finite!(self, game => game.is_symmetric())
    }
}

#[diagnostic::do_not_recommend]
impl CommonActionSpace for FiniteSpec {}

#[diagnostic::do_not_recommend]
impl AggregateGame for FiniteSpec {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        match self {
            FiniteSpec::ElFarolGame(game) => game.aggregate_spec(),
            FiniteSpec::Minority(game) => game.aggregate_spec(),
            FiniteSpec::Matrix(_)
            | FiniteSpec::PrisonersDilemmaGame(_)
            | FiniteSpec::BattleOfSexesGame(_)
            | FiniteSpec::MatchingPenniesGame(_)
            | FiniteSpec::RockPaperScissorsGame(_)
            | FiniteSpec::StagHuntGame(_)
            | FiniteSpec::HawkDoveGame(_)
            | FiniteSpec::PublicGoodsFiniteGame(_) => &[],
        }
    }

    fn aggregate_into(&self, profile: &[usize], out: &mut [f64]) {
        match self {
            FiniteSpec::ElFarolGame(game) => game.aggregate_into(profile, out),
            FiniteSpec::Minority(game) => game.aggregate_into(profile, out),
            FiniteSpec::Matrix(_)
            | FiniteSpec::PrisonersDilemmaGame(_)
            | FiniteSpec::BattleOfSexesGame(_)
            | FiniteSpec::MatchingPenniesGame(_)
            | FiniteSpec::RockPaperScissorsGame(_)
            | FiniteSpec::StagHuntGame(_)
            | FiniteSpec::HawkDoveGame(_)
            | FiniteSpec::PublicGoodsFiniteGame(_) => {}
        }
    }
}

#[diagnostic::do_not_recommend]
impl StageGame for ContinuousSpec {
    type Action = Vec<f64>;

    fn n_roles(&self) -> usize {
        continuous!(self, game => game.n_roles())
    }

    fn check_action(&self, role: usize, action: &Vec<f64>) -> Result<(), ActionError> {
        continuous!(self, game => game.check_action(role, action))
    }
}

#[diagnostic::do_not_recommend]
impl NormalFormGame for ContinuousSpec {
    fn payoff(&self, role: usize, profile: &[Vec<f64>]) -> f64 {
        continuous!(self, game => game.payoff(role, profile))
    }

    fn payoffs_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        continuous!(self, game => game.payoffs_into(profile, out))
    }
}

#[diagnostic::do_not_recommend]
impl ContinuousGame for ContinuousSpec {
    fn bounds(&self, role: usize) -> &[(f64, f64)] {
        continuous!(self, game => game.bounds(role))
    }

    fn best_response(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        continuous!(self, game => game.best_response(role, profile))
    }

    fn numeric_br(&self) -> NumericBr {
        continuous!(self, game => game.numeric_br())
    }

    fn gradient(&self, role: usize, profile: &[Vec<f64>]) -> Option<Vec<f64>> {
        continuous!(self, game => game.gradient(role, profile))
    }
}

#[diagnostic::do_not_recommend]
impl CommonActionSpace for ContinuousSpec {}

#[diagnostic::do_not_recommend]
impl AggregateGame for ContinuousSpec {
    fn aggregate_spec(&self) -> &[AggregateSpec] {
        continuous!(self, game => game.aggregate_spec())
    }

    fn aggregate_into(&self, profile: &[Vec<f64>], out: &mut [f64]) {
        continuous!(self, game => game.aggregate_into(profile, out))
    }
}

#[diagnostic::do_not_recommend]
impl DescribeGame for ContinuousSpec {
    fn describe(&self, role: Option<usize>) -> String {
        continuous!(self, game => game.describe(role))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finite_specs() -> Vec<FiniteSpec> {
        vec![
            FiniteSpec::Matrix(
                MatrixGame::bimatrix(
                    &[vec![3.0, 0.0], vec![5.0, 1.0]],
                    &[vec![3.0, 5.0], vec![0.0, 1.0]],
                )
                .unwrap(),
            ),
            FiniteSpec::PrisonersDilemmaGame(PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap()),
            FiniteSpec::BattleOfSexesGame(BattleOfSexes::default()),
            FiniteSpec::MatchingPenniesGame(MatchingPennies::default()),
            FiniteSpec::RockPaperScissorsGame(RockPaperScissors::default()),
            FiniteSpec::StagHuntGame(StagHunt::default()),
            FiniteSpec::HawkDoveGame(HawkDove::new(2.0, 3.0).unwrap()),
            FiniteSpec::ElFarolGame(ElFarol::new(5, 3).unwrap()),
            FiniteSpec::Minority(MinorityGame::new(5).unwrap()),
            FiniteSpec::PublicGoodsFiniteGame(PublicGoodsFinite::new(3, 1.5, 1.0).unwrap()),
        ]
    }

    #[test]
    fn finite_spec_round_trips_through_serde() {
        for spec in finite_specs() {
            let json = serde_json::to_string(&spec).unwrap();
            let back: FiniteSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(back, spec);
        }
    }

    #[test]
    fn finite_spec_forwards_to_the_wrapped_game() {
        let inner = ElFarol::new(4, 2).unwrap();
        let spec = FiniteSpec::ElFarolGame(inner.clone());
        assert_eq!(spec.n_roles(), inner.n_roles());
        assert_eq!(spec.is_symmetric(), inner.is_symmetric());
        assert_eq!(spec.aggregate_spec(), inner.aggregate_spec());
        for bits in 0..16_usize {
            let profile: Vec<usize> = (0..4).map(|i| (bits >> i) & 1).collect();
            for role in 0..4 {
                assert_eq!(
                    spec.payoff(role, &profile).to_bits(),
                    inner.payoff(role, &profile).to_bits()
                );
            }
            let (mut a, mut b) = ([0.0; 4], [0.0; 4]);
            spec.payoffs_into(&profile, &mut a);
            inner.payoffs_into(&profile, &mut b);
            assert_eq!(a, b);
            let (mut a, mut b) = ([0.0; 1], [0.0; 1]);
            spec.aggregate_into(&profile, &mut a);
            inner.aggregate_into(&profile, &mut b);
            assert_eq!(a, b);
        }
        for action in 0..2 {
            assert_eq!(spec.action_label(0, action), inner.action_label(0, action));
        }

        let inner = MatrixGame::bimatrix(
            &[vec![3.0, 0.0], vec![5.0, 1.0]],
            &[vec![3.0, 5.0], vec![0.0, 1.0]],
        )
        .unwrap()
        .with_labels(vec![
            vec!["C".to_owned(), "D".to_owned()],
            vec!["C".to_owned(), "D".to_owned()],
        ])
        .unwrap();
        let spec = FiniteSpec::Matrix(inner.clone());
        assert_eq!(spec.is_symmetric(), inner.is_symmetric());
        assert!(spec.aggregate_spec().is_empty());
        for role in 0..2 {
            for action in 0..2 {
                assert_eq!(
                    spec.action_label(role, action),
                    inner.action_label(role, action)
                );
                for other in 0..2 {
                    let profile = [action, other];
                    assert_eq!(spec.payoff(role, &profile), inner.payoff(role, &profile));
                }
            }
        }
    }

    #[test]
    fn continuous_spec_forwards_to_the_wrapped_game() {
        let inner = Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap();
        let spec = ContinuousSpec::CournotGame(inner.clone());
        let profile = vec![vec![0.0], vec![50.0], vec![100.0]];
        assert_eq!(spec.n_roles(), inner.n_roles());
        assert_eq!(spec.numeric_br(), inner.numeric_br());
        assert_eq!(spec.aggregate_spec(), inner.aggregate_spec());
        for role in 0..3 {
            assert_eq!(spec.bounds(role), inner.bounds(role));
            assert_eq!(spec.payoff(role, &profile), inner.payoff(role, &profile));
            assert_eq!(
                spec.best_response(role, &profile),
                inner.best_response(role, &profile)
            );
            assert_eq!(
                spec.gradient(role, &profile),
                inner.gradient(role, &profile)
            );
            assert_eq!(
                spec.check_action(role, &vec![101.0]),
                inner.check_action(role, &vec![101.0])
            );
        }
        let (mut a, mut b) = ([0.0; 2], [0.0; 2]);
        spec.aggregate_into(&profile, &mut a);
        inner.aggregate_into(&profile, &mut b);
        assert_eq!(a, b);

        let inner = PublicGoodsContinuous::new(4, 10.0, 1.6).unwrap();
        let spec = ContinuousSpec::PublicGoodsContinuousGame(inner.clone());
        let json = serde_json::to_string(&spec).unwrap();
        let back: ContinuousSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
        assert_eq!(spec.numeric_br(), inner.numeric_br());
        let profile = vec![vec![1.0], vec![2.0], vec![3.0], vec![4.0]];
        for role in 0..4 {
            assert_eq!(
                spec.best_response(role, &profile),
                inner.best_response(role, &profile)
            );
            assert_eq!(
                spec.gradient(role, &profile),
                inner.gradient(role, &profile)
            );
        }
    }

    #[test]
    fn finite_spec_has_the_shape_of_the_wrapped_game() {
        for spec in finite_specs() {
            let n_roles = spec.n_roles();
            assert!(n_roles >= 2);
            for role in 0..n_roles {
                let n = spec.n_actions(role);
                assert!(spec.check_action(role, &(n - 1)).is_ok());
                assert!(spec.check_action(role, &n).is_err());
            }
        }
    }
}
