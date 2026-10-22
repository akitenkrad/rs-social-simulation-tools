//! Mechanism `Registry`, TOML `Params`, and `Scenario` loader for `socsim`.
//!
//! This crate provides:
//!
//! 1. [`Params`] — a thin newtype around [`toml::Table`] with typed, defaulted
//!    getters.  Mechanism constructors receive a `&Params` so they can read
//!    calibrated values from a scenario TOML.
//!
//! 2. [`Registry`] — a string-keyed map from mechanism name to a boxed
//!    constructor function ([`MechanismCtor`]).  Researchers call
//!    [`Registry::register`] to make their mechanism available by name, then
//!    [`Registry::build`] to instantiate it from a `Params`.
//!
//! 3. [`ModulePack`] — a trait that groups related mechanisms and registers
//!    them into a `Registry` in one call.
//!
//! 4. [`Scenario`] — a deserialised scenario TOML (design §7).
//!    Use [`Scenario::from_path`] or [`Scenario::parse`] to load, then
//!    [`Scenario::validate`] to check consistency against a registry.
//!
//! # Dependency note
//!
//! This crate intentionally does **not** depend on `socsim-engine` to avoid a
//! dependency cycle (`engine` → `config` → `engine` is forbidden).

pub mod scenario;

pub use scenario::{
    metric_key_matches, MechanismEntry, OutputSection, RawParams, Scenario, SimulationSection,
};

use std::collections::HashMap;

use socsim_core::{Mechanism, Result, SocsimError, WorldState};

// ── Params ────────────────────────────────────────────────────────────────────

/// Typed parameter bag, backed by a [`toml::Table`].
///
/// Mechanism constructors receive a `&Params`.  Use the typed getters
/// (`get_f64`, `get_u64`, …) with a fallback default; these return the
/// default when the key is absent, making partial scenario files valid.
///
/// # Type mismatches fall back to the default
///
/// The typed getters do **not** report a value of the wrong TOML type: they
/// return the default exactly as if the key were absent.  In particular:
///
/// - [`get_f64`](Self::get_f64) reads only TOML floats, so an integer such as
///   `rate = 2` yields the default rather than `2.0`;
/// - [`get_u64`](Self::get_u64) and [`get_i64`](Self::get_i64) read only TOML
///   integers, so `count = 10.0` yields the default, and `get_u64` converts a
///   negative integer with `as u64` (so `-1` becomes `u64::MAX`);
/// - [`get_bool`](Self::get_bool) and [`get_str`](Self::get_str) likewise
///   return the default for a value of another type.
///
/// Callers that must reject such values should inspect the raw value with
/// [`get_value`](Self::get_value) instead.  Nested tables are read with
/// [`get_table`](Self::get_table).
#[derive(Debug, Clone, Default)]
pub struct Params {
    table: toml::Table,
}

impl Params {
    /// Create an empty parameter bag (all lookups return their defaults).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Return the value for `key` as `f64`, or `default` if absent.
    ///
    /// Only a TOML float is read: a value of any other type, including a
    /// TOML integer such as `2`, also yields `default`.
    pub fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.table
            .get(key)
            .and_then(|v| v.as_float())
            .unwrap_or(default)
    }

    /// Return the value for `key` as `u64`, or `default` if absent.
    ///
    /// Only a TOML integer is read: a value of any other type, including a
    /// TOML float such as `10.0`, also yields `default`.  A negative integer
    /// is converted with `as u64`, so `-1` becomes `u64::MAX`.
    pub fn get_u64(&self, key: &str, default: u64) -> u64 {
        self.table
            .get(key)
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
            .unwrap_or(default)
    }

    /// Return the value for `key` as `i64`, or `default` if absent.
    ///
    /// Only a TOML integer is read: a value of any other type, including a
    /// TOML float, also yields `default`.
    pub fn get_i64(&self, key: &str, default: i64) -> i64 {
        self.table
            .get(key)
            .and_then(|v| v.as_integer())
            .unwrap_or(default)
    }

    /// Return the value for `key` as `bool`, or `default` if absent.
    ///
    /// A value of any other type also yields `default`.
    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.table
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }

    /// Return the value for `key` as `&str`, or `default` if absent.
    ///
    /// A value of any other type also yields `default`.
    ///
    /// The returned string is borrowed from the TOML table so it lives as
    /// long as `self`.
    pub fn get_str<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.table
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
    }

    /// Return the value for `key` as a nested parameter bag.
    ///
    /// Returns a copy of the table stored under `key`, or `None` when the
    /// key is absent or its value is not a TOML table.  Unlike the typed
    /// getters above, a value of another type is reported as `None` rather
    /// than replaced by a default.
    pub fn get_table(&self, key: &str) -> Option<Params> {
        self.table
            .get(key)
            .and_then(|v| v.as_table())
            .map(|t| Params::from(t.clone()))
    }

    /// Return the raw TOML value stored under `key`, or `None` if absent.
    ///
    /// This performs no type conversion, so callers can distinguish an
    /// integer from a float, read arrays, and reject values of the wrong
    /// type instead of silently falling back to a default.
    pub fn get_value(&self, key: &str) -> Option<&toml::Value> {
        self.table.get(key)
    }
}

impl From<toml::Table> for Params {
    fn from(table: toml::Table) -> Self {
        Self { table }
    }
}

// ── MechanismCtor ─────────────────────────────────────────────────────────────

/// Type alias for a boxed mechanism constructor function.
///
/// Given a `&Params`, returns a heap-allocated [`Mechanism`] or an error.
pub type MechanismCtor<W> = Box<dyn Fn(&Params) -> Result<Box<dyn Mechanism<W>>>>;

// ── Registry ─────────────────────────────────────────────────────────────────

/// A name-to-constructor map for [`Mechanism`]s.
///
/// # Usage
///
/// ```ignore
/// let mut reg: Registry<MyWorld> = Registry::new();
/// reg.register("growth", |params| {
///     let rate = params.get_f64("rate", 1.0);
///     Ok(Box::new(GrowthMechanism { rate }))
/// });
/// let mechanism = reg.build("growth", &Params::empty())?;
/// ```
pub struct Registry<W: WorldState> {
    ctors: HashMap<String, MechanismCtor<W>>,
}

impl<W: WorldState> Registry<W> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
        }
    }

    /// Register a mechanism constructor under `name`.
    ///
    /// `ctor` must be `'static` (it will be boxed and stored).  Calling
    /// `register` a second time with the same name silently overwrites the
    /// previous constructor.
    pub fn register<F>(&mut self, name: &str, ctor: F)
    where
        F: Fn(&Params) -> Result<Box<dyn Mechanism<W>>> + 'static,
    {
        self.ctors.insert(name.to_owned(), Box::new(ctor));
    }

    /// Instantiate the mechanism registered under `name`.
    ///
    /// Returns [`SocsimError::UnknownMechanism`] if `name` has not been
    /// registered.
    pub fn build(&self, name: &str, params: &Params) -> Result<Box<dyn Mechanism<W>>> {
        match self.ctors.get(name) {
            Some(ctor) => ctor(params),
            None => Err(SocsimError::UnknownMechanism(name.to_owned())),
        }
    }

    /// Return the names of all registered mechanisms, in arbitrary order.
    pub fn names(&self) -> Vec<&str> {
        self.ctors.keys().map(|s| s.as_str()).collect()
    }
}

impl<W: WorldState> Default for Registry<W> {
    fn default() -> Self {
        Self::new()
    }
}

// ── ModulePack ────────────────────────────────────────────────────────────────

/// A bundle of related mechanisms that registers itself into a [`Registry`].
///
/// Research modules implement this trait so that users can activate an entire
/// body of work with a single call.
///
/// # Example
///
/// ```ignore
/// struct HrLifecyclePack;
///
/// impl ModulePack<HrWorld> for HrLifecyclePack {
///     fn pack_name(&self) -> &str { "hr-lifecycle" }
///     fn register(&self, reg: &mut Registry<HrWorld>) {
///         reg.register("hiring", |p| Ok(Box::new(HiringMechanism::from_params(p))));
///         reg.register("turnover", |p| Ok(Box::new(TurnoverMechanism::from_params(p))));
///         // …
///     }
/// }
/// ```
pub trait ModulePack<W: WorldState> {
    /// Human-readable name for this pack (used in CLI `list packs`).
    fn pack_name(&self) -> &str;

    /// Register all mechanisms in this pack into `reg`.
    fn register(&self, reg: &mut Registry<W>);
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_empty_returns_defaults() {
        let p = Params::empty();
        assert!((p.get_f64("x", 2.71) - 2.71).abs() < 1e-9);
        assert_eq!(p.get_u64("n", 7), 7);
        assert_eq!(p.get_i64("i", -1), -1);
        assert!(p.get_bool("flag", true));
        assert_eq!(p.get_str("name", "default"), "default");
    }

    #[test]
    fn params_from_toml_table() {
        let table: toml::Table = toml::from_str("rate = 0.5\ncount = 10").unwrap();
        let p = Params::from(table);
        assert!((p.get_f64("rate", 0.0) - 0.5).abs() < 1e-9);
        assert_eq!(p.get_u64("count", 0), 10);
    }

    #[test]
    fn params_get_table_reads_nested_table() {
        let table: toml::Table =
            toml::from_str("[game]\nkind = \"matrix\"\n[game.inner]\nx = 1.5").unwrap();
        let p = Params::from(table);
        let game = p.get_table("game").expect("nested table");
        assert_eq!(game.get_str("kind", ""), "matrix");
        let inner = game.get_table("inner").expect("doubly nested table");
        assert!((inner.get_f64("x", 0.0) - 1.5).abs() < 1e-12);
        // The original bag is left unchanged by the copy.
        assert_eq!(
            p.get_value("game")
                .and_then(|v| v.as_table())
                .map(|t| t.len()),
            Some(2)
        );
    }

    #[test]
    fn params_get_table_none_for_missing_or_non_table() {
        let table: toml::Table = toml::from_str("x = 1\nxs = [1, 2]\ns = \"t\"").unwrap();
        let p = Params::from(table);
        assert!(p.get_table("missing").is_none());
        assert!(p.get_table("x").is_none());
        assert!(p.get_table("xs").is_none());
        assert!(p.get_table("s").is_none());
    }

    #[test]
    fn params_get_value_reads_raw_values() {
        let table: toml::Table =
            toml::from_str("n = 2\nf = 2.0\nxs = [[1, 2], [3.5, 4]]\n[t]\na = true").unwrap();
        let p = Params::from(table);
        assert_eq!(p.get_value("n"), Some(&toml::Value::Integer(2)));
        assert_eq!(p.get_value("f"), Some(&toml::Value::Float(2.0)));
        let xs = p.get_value("xs").and_then(|v| v.as_array()).expect("array");
        assert_eq!(xs.len(), 2);
        let first = xs[0].as_array().expect("nested array");
        assert_eq!(first[1].as_integer(), Some(2));
        assert_eq!(xs[1].as_array().unwrap()[0].as_float(), Some(3.5));
        assert!(p.get_value("t").map(|v| v.is_table()).unwrap_or(false));
        assert!(p.get_value("missing").is_none());
    }

    #[test]
    fn params_typed_getters_fall_back_on_type_mismatch() {
        // Documented behaviour: a value of the wrong type yields the default.
        let table: toml::Table = toml::from_str("i = 2\nf = 2.0\nneg = -1\ns = \"x\"").unwrap();
        let p = Params::from(table);
        assert_eq!(p.get_f64("i", 9.0), 9.0);
        assert_eq!(p.get_u64("f", 7), 7);
        assert_eq!(p.get_u64("neg", 0), u64::MAX);
        assert!(p.get_bool("s", true));
        assert_eq!(p.get_str("i", "d"), "d");
    }

    // Minimal WorldState for testing Registry without pulling in engine.
    struct FakeWorld;
    impl socsim_core::WorldState for FakeWorld {
        fn agent_ids(&self) -> Vec<socsim_core::AgentId> {
            vec![]
        }
        fn clock(&self) -> &socsim_core::SimClock {
            unimplemented!()
        }
        fn clock_mut(&mut self) -> &mut socsim_core::SimClock {
            unimplemented!()
        }
    }

    struct NoopMechanism;
    impl socsim_core::Mechanism<FakeWorld> for NoopMechanism {
        fn name(&self) -> &str {
            "noop"
        }
        fn phases(&self) -> &'static [socsim_core::Phase] {
            &[]
        }
        fn apply(
            &mut self,
            _phase: socsim_core::Phase,
            _ctx: &mut socsim_core::StepContext<'_, FakeWorld>,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn registry_register_and_build() {
        let mut reg: Registry<FakeWorld> = Registry::new();
        reg.register("noop", |_params| Ok(Box::new(NoopMechanism)));
        let m = reg.build("noop", &Params::empty()).unwrap();
        assert_eq!(m.name(), "noop");
    }

    #[test]
    fn registry_unknown_mechanism_error() {
        let reg: Registry<FakeWorld> = Registry::new();
        let result = reg.build("missing", &Params::empty());
        assert!(
            matches!(result, Err(SocsimError::UnknownMechanism(_))),
            "expected UnknownMechanism error"
        );
    }

    #[test]
    fn registry_names_lists_registered() {
        let mut reg: Registry<FakeWorld> = Registry::new();
        reg.register("a", |_| Ok(Box::new(NoopMechanism)));
        reg.register("b", |_| Ok(Box::new(NoopMechanism)));
        let mut names = reg.names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }
}
