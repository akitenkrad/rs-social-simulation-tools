use socsim_games::{DampedBestResponse, DecisionRule, MatrixGame};

fn require_rule<R: DecisionRule<MatrixGame, ()>>(_rule: &R) {}

fn main() {
    let rule = DampedBestResponse::new(0.5).unwrap();
    require_rule(&rule);
}
