use super::FiniteGame;

/// The naming scheme used for action metric identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetricScheme {
    /// All roles share one action identifier for each action index.
    Pooled,
    /// Each role and action index has a distinct identifier.
    PerRole,
}

/// An action and its stable metric identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricAction {
    /// The identifier appended to an action metric name.
    pub id: String,
    /// The role for a role-specific identifier, or `None` for a pooled identifier.
    pub role: Option<usize>,
    /// The zero-based action index.
    pub action: usize,
}

/// Stable action identifiers for one finite game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricActionIds {
    scheme: MetricScheme,
    actions: Vec<MetricAction>,
}

impl MetricActionIds {
    /// Returns the naming scheme used by these identifiers.
    pub fn scheme(&self) -> MetricScheme {
        self.scheme
    }

    /// Returns the identifiers in metric column order.
    pub fn actions(&self) -> &[MetricAction] {
        &self.actions
    }

    /// Returns the metric identifier for a role and action index.
    ///
    /// For pooled identifiers, all roles share the same identifier for an action index.
    pub fn id(&self, role: usize, action: usize) -> Option<&str> {
        self.actions
            .iter()
            .find(|entry| {
                entry.action == action
                    && match self.scheme {
                        MetricScheme::Pooled => true,
                        MetricScheme::PerRole => entry.role == Some(role),
                    }
            })
            .map(|entry| entry.id.as_str())
    }
}

/// Builds stable metric identifiers from role and action indices.
///
/// Actions are pooled when every role has the same action count and the label at
/// each action index exactly matches role zero. Otherwise, identifiers are role-specific.
pub fn metric_action_ids<G: FiniteGame>(game: &G) -> MetricActionIds {
    let n_roles = game.n_roles();
    let pooled = n_roles == 0
        || (1..n_roles).all(|role| {
            game.n_actions(role) == game.n_actions(0)
                && (0..game.n_actions(0))
                    .all(|action| game.action_label(role, action) == game.action_label(0, action))
        });

    if pooled {
        let n_actions = if n_roles == 0 { 0 } else { game.n_actions(0) };
        MetricActionIds {
            scheme: MetricScheme::Pooled,
            actions: (0..n_actions)
                .map(|action| MetricAction {
                    id: format!("a{action}"),
                    role: None,
                    action,
                })
                .collect(),
        }
    } else {
        MetricActionIds {
            scheme: MetricScheme::PerRole,
            actions: (0..n_roles)
                .flat_map(|role| {
                    (0..game.n_actions(role)).map(move |action| MetricAction {
                        id: format!("r{role}-a{action}"),
                        role: Some(role),
                        action,
                    })
                })
                .collect(),
        }
    }
}
