use socsim_core::AgentId;
use socsim_games::{DecisionContext, MatrixGame};

fn read_hidden_state(ctx: &DecisionContext<'_, MatrixGame, ()>) {
    let _ = ctx.held(AgentId(0));
    let _ = ctx.x();
}

fn main() {}
