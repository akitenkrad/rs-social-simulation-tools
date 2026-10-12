use socsim_games::{
    ActionError, FermiUpdate, FiniteGame, GameSimFactory, NormalFormGame, StageGame, Update,
};

/// A finite game whose roles are not declared to share an action space.
struct Asymmetric;

impl StageGame for Asymmetric {
    type Action = usize;

    fn n_roles(&self) -> usize {
        2
    }

    fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
        if role >= 2 {
            return Err(ActionError::InvalidRole { role });
        }
        if *action >= self.n_actions(role) {
            return Err(ActionError::InvalidAction {
                role,
                action: *action,
            });
        }
        Ok(())
    }
}

impl NormalFormGame for Asymmetric {
    fn payoff(&self, _role: usize, _profile: &[usize]) -> f64 {
        0.0
    }
}

impl FiniteGame for Asymmetric {
    fn n_actions(&self, role: usize) -> usize {
        role + 2
    }
}

fn main() {
    let _ = GameSimFactory::new(Asymmetric)
        .dynamics(FermiUpdate::new(1.0, Update::Synchronous).unwrap());
}
