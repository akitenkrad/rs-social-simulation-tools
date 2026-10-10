use socsim_core::AgentId;
use socsim_games::{DecisionRule, ExperimentState, MatrixGame, TitForTat};

#[derive(Clone, Debug)]
struct StateWithoutTftSlot;

struct ExperimentWithoutTftSlot;

impl ExperimentState for ExperimentWithoutTftSlot {
    type PolicyState = StateWithoutTftSlot;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

fn require_rule<R: DecisionRule<MatrixGame, ExperimentWithoutTftSlot>>(_rule: &R) {}

fn main() {
    require_rule(&TitForTat::new(0));
}
