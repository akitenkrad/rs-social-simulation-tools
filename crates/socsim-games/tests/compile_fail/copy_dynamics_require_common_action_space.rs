use socsim_games::{
    assemble_with, build_world, ActionError, AssignGroup, AssignKind, Assignment, FermiUpdate,
    FiniteGame, InitialActions, NormalFormGame, NormalFormOutcome, Pairwise, PlayOptions,
    RandomPairs, RuleBook, StageGame, Update,
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
    let rules = RuleBook::new();
    let world = build_world(
        Asymmetric,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 2,
        }]),
        1,
        1,
        7,
    )
    .unwrap();

    let _ = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        FermiUpdate::new(1.0, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        7,
    );
}
