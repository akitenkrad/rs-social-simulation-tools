use socsim_core::AgentId;
use socsim_games::{BuiltinPolicyState, DecisionContext, ExperimentState, MatrixGame};

struct PrivateExperiment {
    private_types: Vec<u8>,
}

struct MyPrivateView {
    mine: u8,
}

impl ExperimentState for PrivateExperiment {
    type PolicyState = BuiltinPolicyState;
    type View<'a> = MyPrivateView;

    fn view_for(&self, me: AgentId) -> Self::View<'_> {
        MyPrivateView {
            mine: self.private_types[me.0 as usize],
        }
    }
}

fn leak_all_private_types(ctx: &DecisionContext<'_, MatrixGame, PrivateExperiment>) {
    let _ = ctx.view().private_types;
}

fn main() {}
