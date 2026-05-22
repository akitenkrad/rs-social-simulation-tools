//! Scenario TOML schema and loader for `socsim`.
//!
//! A [`Scenario`] captures everything needed to reproduce a simulation run:
//! the module-pack name, the world parameters, the ordered mechanism list
//! (with per-mechanism params), output paths, and requested metrics.
//!
//! # Example TOML
//!
//! ```toml
//! [simulation]
//! name        = "hr_baseline"
//! module_pack = "hr-lifecycle"
//! t_max       = 60
//! seed        = 42
//! scheduler   = "random_activation"
//!
//! [world]
//! n_teams    = 5
//! team_size_initial = 8
//!
//! [[mechanism]]
//! name  = "learning_curve"
//! phase = "environment"
//! [mechanism.params]
//! lambda_learn = 0.15
//!
//! [output]
//! log_path = "runs/{name}_{seed}.jsonl"
//! metrics  = ["org_performance", "avg_tenure"]
//! ```

use std::path::Path;

use serde::Deserialize;

use socsim_core::{Result, SocsimError};

use crate::Params;

// ── SimulationSection ─────────────────────────────────────────────────────────

/// The `[simulation]` table of a scenario TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct SimulationSection {
    /// Human-readable run name (used in output path substitution).
    pub name: String,

    /// The module-pack string (e.g. `"hr-lifecycle"`).
    pub module_pack: String,

    /// Maximum number of time steps (exclusive).  Must be > 0.
    pub t_max: u64,

    /// Root RNG seed for this run.
    pub seed: u64,

    /// Scheduler string: `"sequential"` or `"random_activation"`.
    pub scheduler: String,
}

// ── MechanismEntry ────────────────────────────────────────────────────────────

/// One entry in the `[[mechanism]]` array.
///
/// Order in the TOML array is preserved and used as the composition order.
#[derive(Debug, Clone, Deserialize)]
pub struct MechanismEntry {
    /// Registry name of the mechanism (must be registered in the pack's
    /// [`Registry`][crate::Registry]).
    pub name: String,

    /// Informational phase label (not enforced at runtime; the mechanism's
    /// own [`phases()`][socsim_core::Mechanism::phases] governs which phases
    /// it actually runs in).
    pub phase: Option<String>,

    /// Per-mechanism parameters forwarded to its constructor.
    #[serde(default)]
    pub params: RawParams,
}

// ── RawParams ─────────────────────────────────────────────────────────────────

/// A thin serde-deserializable wrapper around a TOML table, used for the
/// `[mechanism.params]` sub-table.  Converts into [`Params`] on demand.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawParams(#[serde(default)] pub toml::Table);

impl From<RawParams> for Params {
    fn from(r: RawParams) -> Self {
        Params::from(r.0)
    }
}

impl RawParams {
    /// Convert to a [`Params`] by cloning the inner table.
    pub fn to_params(&self) -> Params {
        Params::from(self.0.clone())
    }

    /// Insert or overwrite a `f64` parameter by key.
    pub fn set_f64(&mut self, key: &str, value: f64) {
        self.0.insert(key.to_owned(), toml::Value::Float(value));
    }
}

// ── OutputSection ─────────────────────────────────────────────────────────────

/// The `[output]` table of a scenario TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputSection {
    /// Path template for the JSONL log file.
    ///
    /// Supports `{name}` and `{seed}` substitutions, e.g.
    /// `"runs/{name}_{seed}.jsonl"`.
    pub log_path: String,

    /// Metric keys to include in run summaries.
    #[serde(default)]
    pub metrics: Vec<String>,
}

impl OutputSection {
    /// Expand `{name}` and `{seed}` in `log_path`.
    pub fn resolve_log_path(&self, name: &str, seed: u64) -> String {
        self.log_path
            .replace("{name}", name)
            .replace("{seed}", &seed.to_string())
    }
}

// ── Scenario ──────────────────────────────────────────────────────────────────

/// A fully-parsed simulation scenario.
///
/// Deserialises from a TOML file conforming to design §7.  The `[[mechanism]]`
/// array is **order-preserving**: the composition order equals the declaration
/// order in the TOML file.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// `[simulation]` section.
    pub simulation: SimulationSection,

    /// `[world]` free-form parameter table (forwarded to the world factory).
    #[serde(default)]
    pub world: RawParams,

    /// `[[mechanism]]` ordered array.
    #[serde(default, rename = "mechanism")]
    pub mechanisms: Vec<MechanismEntry>,

    /// `[output]` section.
    pub output: OutputSection,
}

impl Scenario {
    /// Load a scenario from a TOML file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`SocsimError::Config`] on I/O or parse failure.
    pub fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            SocsimError::Config(format!(
                "cannot read scenario file '{}': {e}",
                path.display()
            ))
        })?;
        Self::parse(&text)
    }

    /// Parse a scenario from a TOML string.
    ///
    /// This is the canonical parse entry point; consider also the
    /// [`std::str::FromStr`] implementation for ergonomic use.
    ///
    /// # Errors
    ///
    /// Returns [`SocsimError::Config`] on parse failure.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| SocsimError::Config(format!("scenario parse error: {e}")))
    }

    /// Validate this scenario against a list of known mechanism names and
    /// a list of known scheduler strings.
    ///
    /// # Errors
    ///
    /// Returns [`SocsimError::Config`] with a descriptive message when:
    /// - `t_max == 0`
    /// - `scheduler` is not one of `"sequential"` or `"random_activation"`
    /// - Any `[[mechanism]].name` is not in `registry_names`
    pub fn validate(&self, registry_names: &[&str]) -> Result<()> {
        if self.simulation.t_max == 0 {
            return Err(SocsimError::Config(
                "simulation.t_max must be > 0".to_owned(),
            ));
        }

        let valid_schedulers = ["sequential", "random_activation"];
        if !valid_schedulers.contains(&self.simulation.scheduler.as_str()) {
            return Err(SocsimError::Config(format!(
                "unknown scheduler '{}'; valid values: sequential, random_activation",
                self.simulation.scheduler
            )));
        }

        for entry in &self.mechanisms {
            if !registry_names.contains(&entry.name.as_str()) {
                return Err(SocsimError::Config(format!(
                    "mechanism '{}' is not registered in the '{}' pack; available: [{}]",
                    entry.name,
                    self.simulation.module_pack,
                    registry_names.join(", ")
                )));
            }
        }

        Ok(())
    }
}

impl std::str::FromStr for Scenario {
    type Err = socsim_core::SocsimError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Scenario::parse(s)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    const MINIMAL_TOML: &str = r#"
[simulation]
name        = "test"
module_pack = "hr-lifecycle"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_teams = 5

[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15

[[mechanism]]
name = "org_performance"

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["org_performance", "avg_tenure"]
"#;

    #[test]
    fn parse_minimal_scenario() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        assert_eq!(s.simulation.name, "test");
        assert_eq!(s.simulation.module_pack, "hr-lifecycle");
        assert_eq!(s.simulation.t_max, 60);
        assert_eq!(s.simulation.seed, 42);
        assert_eq!(s.simulation.scheduler, "random_activation");
        assert_eq!(s.mechanisms.len(), 2);
        assert_eq!(s.mechanisms[0].name, "learning_curve");
        assert_eq!(s.mechanisms[1].name, "org_performance");
    }

    #[test]
    fn mechanism_order_preserved() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        let names: Vec<&str> = s.mechanisms.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["learning_curve", "org_performance"]);
    }

    #[test]
    fn raw_params_to_params() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        let p = s.mechanisms[0].params.to_params();
        let v = p.get_f64("lambda_learn", 0.0);
        assert!((v - 0.15).abs() < 1e-9);
    }

    #[test]
    fn validate_ok() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        let names = &["learning_curve", "org_performance"];
        s.validate(names).expect("should be valid");
    }

    #[test]
    fn validate_unknown_mechanism() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        let result = s.validate(&["learning_curve"]); // org_performance missing
        assert!(
            matches!(result, Err(SocsimError::Config(_))),
            "expected Config error for unknown mechanism"
        );
    }

    #[test]
    fn validate_bad_scheduler() {
        let bad = MINIMAL_TOML.replace("random_activation", "broken_scheduler");
        let s = Scenario::from_str(&bad).expect("should parse");
        let result = s.validate(&["learning_curve", "org_performance"]);
        assert!(matches!(result, Err(SocsimError::Config(_))));
    }

    #[test]
    fn validate_t_max_zero() {
        let bad = MINIMAL_TOML.replace("t_max       = 60", "t_max = 0");
        let s = Scenario::from_str(&bad).expect("should parse");
        let result = s.validate(&["learning_curve", "org_performance"]);
        assert!(matches!(result, Err(SocsimError::Config(_))));
    }

    #[test]
    fn output_resolve_log_path() {
        let s = Scenario::parse(MINIMAL_TOML).expect("should parse");
        let resolved = s
            .output
            .resolve_log_path(&s.simulation.name, s.simulation.seed);
        assert_eq!(resolved, "runs/test_42.jsonl");
    }
}
