use socsim_games::{ActionError, ExperimentState, NormalFormOutcome, OutcomeRule, StageGame};

struct StageOnly;

impl StageGame for StageOnly {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, _role: usize, _action: &usize) -> Result<(), ActionError> {
        Ok(())
    }
}

fn require_outcome<G, X, O>(_outcome: &O)
where
    G: StageGame,
    X: ExperimentState,
    O: OutcomeRule<G, X>,
{
}

fn main() {
    require_outcome::<StageOnly, (), _>(&NormalFormOutcome);
}
