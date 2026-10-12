use socsim_games::continuous::Cournot;
use socsim_games::{DecisionRule, FictitiousPlay};

fn require_rule<R: DecisionRule<Cournot, ()>>(_rule: &R) {}

fn main() {
    require_rule(&FictitiousPlay::new());
}
