use socsim_games::{
    ActionError, DecisionRule, NormalFormGame, StageGame, TitForTat,
};

struct ContinuousLike;

impl StageGame for ContinuousLike {
    type Action = f64;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, _role: usize, _action: &f64) -> Result<(), ActionError> {
        Ok(())
    }
}

impl NormalFormGame for ContinuousLike {
    fn payoff(&self, _role: usize, _profile: &[f64]) -> f64 {
        0.0
    }
}

fn require_rule<R: DecisionRule<ContinuousLike, ()>>(_rule: &R) {}

fn main() {
    require_rule(&TitForTat::new(0));
}
