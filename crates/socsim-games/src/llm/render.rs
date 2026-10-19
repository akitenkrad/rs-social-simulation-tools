use std::fmt::Write;

use super::{ActionCounts, OwnHistory, Violation};
use crate::{ContinuousGame, FiniteGame, StageGame};

pub use crate::game::DescribeGame;

/// Renders game rules, one observation, and retry feedback into prompt text.
///
/// Renderer authors implement this trait and pair it with a compatible
/// [`ObservationProvider`](super::ObservationProvider) inside an
/// [`LlmPolicy`](super::LlmPolicy). Methods must be deterministic functions of
/// their arguments because rendered text participates in replay-cache keys.
pub trait GameRenderer<G: StageGame, Obs>: Send + Sync {
    /// Builds the stable system-level rules for a game and optional role.
    fn rules(&self, game: &G, role: Option<usize>) -> String;
    /// Builds the per-decision prompt fragment from an observation.
    fn turn(&self, obs: &Obs, game: &G) -> String;
    /// Builds corrective text appended after a rejected reply.
    fn feedback(&self, violation: &Violation) -> String;
}

#[derive(Clone, Debug)]
/// Renderer that presents finite normal-form payoffs as Markdown tables.
pub struct MarkdownTableRenderer {
    heading: String,
}

impl Default for MarkdownTableRenderer {
    fn default() -> Self {
        Self {
            heading: "Game payoffs".to_owned(),
        }
    }
}

impl<G: FiniteGame> GameRenderer<G, OwnHistory<usize>> for MarkdownTableRenderer {
    fn rules(&self, game: &G, role: Option<usize>) -> String {
        let mut rendered = format!("# {}\n\n", self.heading);
        if let Some(role) = role {
            let _ = writeln!(rendered, "You are role {role}.\n");
        }
        if game.n_roles() == 2 {
            render_bimatrix(game, &mut rendered);
        } else {
            render_role_tables(game, &mut rendered);
        }
        rendered.push_str(
            "\nReply with JSON: {\"action\": \"<action label>\", \"reason\": \"<reason>\"}.",
        );
        rendered
    }

    fn turn(&self, obs: &OwnHistory<usize>, game: &G) -> String {
        let role = obs.role.unwrap_or(0);
        let mut rendered = format!("t={}\n", obs.t);
        rendered.push_str("my history (oldest first):\n");
        if obs.mine.is_empty() {
            rendered.push_str("- none\n");
        }
        for record in &obs.mine {
            let action = render_action(game, role, record.action);
            let _ = writeln!(
                rendered,
                "- action: {action}; payoff: {}; encounters: {}",
                format_number(record.payoff),
                record.n_played
            );
        }
        rendered.push_str("opponents' last actions: ");
        if obs.opponents_last.is_empty() {
            rendered.push_str("none");
        } else {
            let values: Vec<_> = obs
                .opponents_last
                .iter()
                .copied()
                .map(|action| render_action(game, role, action))
                .collect();
            rendered.push_str(&values.join(", "));
        }
        rendered
    }

    fn feedback(&self, violation: &Violation) -> String {
        format!(
            "Previous response violated {:?}: {}. Return valid JSON using an available action label.",
            violation.kind, violation.message
        )
    }
}

#[derive(Clone, Debug)]
/// Compact renderer that lists available labels and observed action counts.
pub struct CountsRenderer {
    heading: String,
}

impl Default for CountsRenderer {
    fn default() -> Self {
        Self {
            heading: "Observed action counts".to_owned(),
        }
    }
}

impl<G: FiniteGame> GameRenderer<G, ActionCounts> for CountsRenderer {
    fn rules(&self, game: &G, role: Option<usize>) -> String {
        let role = role.unwrap_or(0);
        let labels: Vec<_> = (0..game.n_actions(role))
            .map(|action| escape_cell(&game.action_label(role, action)))
            .collect();
        format!(
            "# {}\n\nYou are role {role}. Available actions: {}. Reply with JSON: {{\"action\": \"<action label>\", \"reason\": \"<reason>\"}}.",
            self.heading,
            labels.join(", ")
        )
    }

    fn turn(&self, obs: &ActionCounts, game: &G) -> String {
        let role = obs.role.unwrap_or(0);
        let my_last = render_action(game, role, obs.my_last);
        let mut rendered = format!("my last action: {my_last}\naction counts:\n");
        for (action, count) in obs.counts.iter().enumerate() {
            let label = if action < game.n_actions(role) {
                game.action_label(role, action).into_owned()
            } else {
                format!("a{action}")
            };
            let _ = writeln!(rendered, "- {label}: {count}");
        }
        rendered
    }

    fn feedback(&self, violation: &Violation) -> String {
        format!(
            "Previous response violated {:?}: {}. Return valid JSON using an available action label.",
            violation.kind, violation.message
        )
    }
}

/// Renders a continuous game from its [`DescribeGame`] text and its bounds,
/// and the agent's own history of real-valued actions.
///
/// `field` is the JSON field the model is asked to reply with; it should be
/// the field the paired parser reads.
#[derive(Clone, Debug)]
pub struct DescribedRenderer {
    heading: String,
    field: String,
}

impl DescribedRenderer {
    /// Constructs a renderer that asks for the reply field `field`, under the
    /// heading `"Game rules"`.
    ///
    /// For an action of one component the system prompt asks for a single
    /// number in the role's bounds. The wording for actions of several
    /// components is provisional: no built-in parser reads such replies yet.
    pub fn new(field: &str) -> Self {
        Self {
            heading: "Game rules".to_owned(),
            field: field.to_owned(),
        }
    }

    /// Replaces the heading of the system prompt.
    pub fn with_heading(mut self, heading: &str) -> Self {
        self.heading = heading.to_owned();
        self
    }
}

impl<G: ContinuousGame + DescribeGame> GameRenderer<G, OwnHistory<Vec<f64>>> for DescribedRenderer {
    fn rules(&self, game: &G, role: Option<usize>) -> String {
        let mut rendered = format!("# {}\n\n", self.heading);
        if let Some(role) = role {
            let _ = writeln!(rendered, "You are role {role}.\n");
        }
        let _ = writeln!(rendered, "{}\n", game.describe(role));
        let bounds = game.bounds(role.unwrap_or(0));
        let value = if let [(lo, hi)] = bounds {
            let _ = writeln!(
                rendered,
                "Your action is a single number between {} and {} (inclusive).",
                format_number(*lo),
                format_number(*hi)
            );
            "<number>".to_owned()
        } else {
            let _ = writeln!(
                rendered,
                "Your action is a list of {} numbers:",
                bounds.len()
            );
            for (dim, (lo, hi)) in bounds.iter().enumerate() {
                let _ = writeln!(
                    rendered,
                    "- component {dim}: between {} and {} (inclusive)",
                    format_number(*lo),
                    format_number(*hi)
                );
            }
            format!("[{}]", vec!["<number>"; bounds.len()].join(", "))
        };
        let _ = write!(
            rendered,
            "\nReply with JSON: {{\"{}\": {value}, \"reason\": \"<reason>\"}}.",
            self.field
        );
        rendered
    }

    fn turn(&self, obs: &OwnHistory<Vec<f64>>, _game: &G) -> String {
        let mut rendered = format!("t={}\n", obs.t);
        rendered.push_str("my history (oldest first):\n");
        if obs.mine.is_empty() {
            rendered.push_str("- none\n");
        }
        for record in &obs.mine {
            let action = render_vector(record.action.as_deref());
            let _ = writeln!(
                rendered,
                "- action: {action}; payoff: {}; encounters: {}",
                format_number(record.payoff),
                record.n_played
            );
        }
        rendered.push_str("opponents' last actions: ");
        if obs.opponents_last.is_empty() {
            rendered.push_str("none");
        } else {
            let values: Vec<_> = obs
                .opponents_last
                .iter()
                .map(|action| render_vector(action.as_deref()))
                .collect();
            rendered.push_str(&values.join(", "));
        }
        rendered
    }

    fn feedback(&self, violation: &Violation) -> String {
        format!(
            "Previous response violated {:?}: {}. Return valid JSON with a number in the {:?} field within the allowed range.",
            violation.kind, violation.message, self.field
        )
    }
}

fn render_vector(action: Option<&[f64]>) -> String {
    match action {
        Some([value]) => format_number(*value),
        Some(values) => {
            let values: Vec<_> = values.iter().copied().map(format_number).collect();
            format!("[{}]", values.join(", "))
        }
        None => "none".to_owned(),
    }
}

fn render_bimatrix<G: FiniteGame>(game: &G, out: &mut String) {
    out.push_str("| role 0 \\ role 1 | ");
    for action in 0..game.n_actions(1) {
        let _ = write!(out, "{} | ", escape_cell(&game.action_label(1, action)));
    }
    out.push_str("\n|---|");
    for _ in 0..game.n_actions(1) {
        out.push_str("---|");
    }
    out.push('\n');
    for row in 0..game.n_actions(0) {
        let _ = write!(out, "| {} | ", escape_cell(&game.action_label(0, row)));
        for column in 0..game.n_actions(1) {
            let profile = [row, column];
            let _ = write!(
                out,
                "({}, {}) | ",
                format_number(game.payoff(0, &profile)),
                format_number(game.payoff(1, &profile))
            );
        }
        out.push('\n');
    }
}

fn render_role_tables<G: FiniteGame>(game: &G, out: &mut String) {
    let profiles = action_profiles(game);
    for role in 0..game.n_roles() {
        let _ = writeln!(out, "## Payoffs for role {role}\n");
        out.push_str("| action profile | payoff |\n|---|---|\n");
        for profile in &profiles {
            let labels: Vec<_> = profile
                .iter()
                .enumerate()
                .map(|(profile_role, &action)| {
                    escape_cell(&game.action_label(profile_role, action))
                })
                .collect();
            let _ = writeln!(
                out,
                "| {} | {} |",
                labels.join(", "),
                format_number(game.payoff(role, profile))
            );
        }
        out.push('\n');
    }
}

fn action_profiles<G: FiniteGame>(game: &G) -> Vec<Vec<usize>> {
    fn extend<G: FiniteGame>(
        game: &G,
        role: usize,
        profile: &mut Vec<usize>,
        profiles: &mut Vec<Vec<usize>>,
    ) {
        if role == game.n_roles() {
            profiles.push(profile.clone());
            return;
        }
        for action in 0..game.n_actions(role) {
            profile.push(action);
            extend(game, role + 1, profile, profiles);
            profile.pop();
        }
    }

    let mut profiles = Vec::new();
    extend(game, 0, &mut Vec::new(), &mut profiles);
    profiles
}

fn render_action<G: FiniteGame>(game: &G, role: usize, action: Option<usize>) -> String {
    match action {
        Some(action) if role < game.n_roles() && action < game.n_actions(role) => {
            game.action_label(role, action).into_owned()
        }
        Some(action) => format!("a{action}"),
        None => "none".to_owned(),
    }
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn format_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}
