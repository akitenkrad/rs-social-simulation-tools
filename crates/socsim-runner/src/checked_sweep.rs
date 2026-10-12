//! Checked parameter sweeps over a [`TrialFactory`]: [`run_trial_sweep`] and
//! [`CheckedSweepPoint`].

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use socsim_config::Scenario;
use socsim_core::{Result, SocsimError, WorldState};

use crate::trial::{run_validated_trials, TrialFactory};
use crate::{cartesian_product, summarize, Summary, SweepAxis};

// ── CheckedSweepPoint ─────────────────────────────────────────────────────────

/// The result of one grid point of [`run_trial_sweep`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CheckedSweepPoint {
    /// Parameter values of this grid point, in sweep axis order.
    pub params: Vec<(String, f64)>,
    /// The effective scenario after patching the base scenario with `params`.
    /// This is exactly the scenario the trials of this point ran on.
    pub effective: Scenario,
    /// Aggregated summary across all seeds of this grid point.
    pub summary: Summary,
}

// ── sweep keys (private) ──────────────────────────────────────────────────────

/// A dotted path into the `[world]` table (`world.<a>.<b>…` without the
/// leading `world`).  At least one non-empty segment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorldPath(Vec<String>);

/// A `<mechanism>.<param>` key: both parts non-empty, `param` without `.`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MechanismParam {
    mechanism: String,
    param: String,
}

/// The parsed target of one sweep axis.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SweepTarget {
    World(WorldPath),
    Mechanism(MechanismParam),
}

/// `SocsimError::Config("sweep axis '<key>': <reason>")`.
fn axis_error(key: &str, reason: impl std::fmt::Display) -> SocsimError {
    SocsimError::Config(format!("sweep axis '{key}': {reason}"))
}

/// Parse a sweep key into a [`SweepTarget`].
fn parse_key(key: &str) -> Result<SweepTarget> {
    if let Some(rest) = key.strip_prefix("world.") {
        let segments: Vec<String> = rest.split('.').map(str::to_owned).collect();
        if segments.iter().any(String::is_empty) {
            return Err(axis_error(
                key,
                "a 'world.' key needs one or more non-empty dot-separated segments after 'world.'",
            ));
        }
        return Ok(SweepTarget::World(WorldPath(segments)));
    }

    let Some(dot) = key.find('.') else {
        return Err(axis_error(
            key,
            "key must be 'world.<path>' or '<mechanism>.<param>'",
        ));
    };
    let mechanism = &key[..dot];
    let param = &key[dot + 1..];
    if mechanism.is_empty() {
        return Err(axis_error(key, "empty mechanism name before '.'"));
    }
    if param.is_empty() {
        return Err(axis_error(key, "empty parameter name after '.'"));
    }
    if param.contains('.') {
        return Err(axis_error(
            key,
            format!("parameter name '{param}' must not contain '.'"),
        ));
    }
    Ok(SweepTarget::Mechanism(MechanismParam {
        mechanism: mechanism.to_owned(),
        param: param.to_owned(),
    }))
}

// ── patching (private) ────────────────────────────────────────────────────────

/// Largest magnitude bound for an exact `i64` conversion: 2^63.
const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;

/// Overwrite an existing value with `value`, keeping its type.
///
/// Integer slots accept only integral values within `[-2^63, 2^63)` and are
/// written back as integers; float slots are written as floats; any other
/// type is an error.  `value` is already known to be finite.
fn write_existing(slot: &mut toml::Value, value: f64) -> std::result::Result<(), String> {
    match slot {
        toml::Value::Integer(_) => {
            if value.fract() == 0.0 && (-TWO_POW_63..TWO_POW_63).contains(&value) {
                *slot = toml::Value::Integer(value as i64);
                Ok(())
            } else {
                Err(format!(
                    "existing value is an integer; {value} is not an integral value representable as i64"
                ))
            }
        }
        toml::Value::Float(_) => {
            *slot = toml::Value::Float(value);
            Ok(())
        }
        other => Err(format!(
            "existing value is a {}; only integer and float values can be swept",
            other.type_str()
        )),
    }
}

/// Render a table location such as `[world]` or `[world.game]`.
fn table_label(segments: &[String]) -> String {
    if segments.is_empty() {
        "[world]".to_owned()
    } else {
        format!("[world.{}]", segments.join("."))
    }
}

/// Write `value` at an existing key under `[world]`.  No table or key is
/// created.
fn patch_world(
    world: &mut toml::Table,
    path: &WorldPath,
    value: f64,
) -> std::result::Result<(), String> {
    let (leaf, parents) = path
        .0
        .split_last()
        .expect("a parsed world path has at least one segment");

    let mut table = world;
    for (i, seg) in parents.iter().enumerate() {
        let here = table_label(&parents[..i]);
        match table.get_mut(seg) {
            Some(toml::Value::Table(t)) => table = t,
            Some(other) => {
                return Err(format!(
                    "'{seg}' in {here} is a {}, not a table",
                    other.type_str()
                ))
            }
            None => return Err(format!("no key '{seg}' in {here}")),
        }
    }

    match table.get_mut(leaf) {
        Some(slot) => write_existing(slot, value),
        None => Err(format!("no key '{leaf}' in {}", table_label(parents))),
    }
}

/// Write `value` into `param` of every `[[mechanism]]` entry named
/// `mechanism`.  An existing param keeps its type (see [`write_existing`]); a
/// missing param is added as a float, as the legacy sweep does.
fn patch_mechanism(
    scenario: &mut Scenario,
    target: &MechanismParam,
    value: f64,
) -> std::result::Result<(), String> {
    let mut matched = 0usize;
    for (index, entry) in scenario.mechanisms.iter_mut().enumerate() {
        if entry.name != target.mechanism {
            continue;
        }
        matched += 1;
        match entry.params.0.get_mut(&target.param) {
            Some(slot) => write_existing(slot, value).map_err(|reason| {
                format!(
                    "param '{}' of [[mechanism]] #{index} ('{}'): {reason}",
                    target.param, target.mechanism
                )
            })?,
            None => entry.params.set_f64(&target.param, value),
        }
    }
    if matched == 0 {
        return Err(format!(
            "mechanism '{}' is not in the scenario",
            target.mechanism
        ));
    }
    Ok(())
}

/// Apply one axis value to `scenario`.
fn patch(scenario: &mut Scenario, key: &str, target: &SweepTarget, value: f64) -> Result<()> {
    if !value.is_finite() {
        return Err(axis_error(key, format!("value {value} is not finite")));
    }
    let outcome = match target {
        SweepTarget::World(path) => patch_world(&mut scenario.world.0, path, value),
        SweepTarget::Mechanism(mp) => patch_mechanism(scenario, mp, value),
    };
    outcome.map_err(|reason| axis_error(key, reason))
}

/// Render grid-point params as `k1=v1, k2=v2`.
fn format_params(params: &[(String, f64)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ── run_trial_sweep ───────────────────────────────────────────────────────────

/// Run a checked grid sweep over the Cartesian product of `axes` (the last
/// axis varies fastest), using `factory` for every trial.
///
/// # Keys
///
/// Each `SweepAxis::param_key` is either
///
/// - `world.<a>[.<b>…]` — a dotted path into `[world]`, one or more non-empty
///   segments after `world.`; or
/// - `<mechanism>.<param>` — split at the first `.`, both parts non-empty and
///   `<param>` free of `.`.  The value is written into `<param>` of **every**
///   `[[mechanism]]` entry named `<mechanism>`.
///
/// The `world.` prefix is reserved by this key syntax only; registries may
/// still register a mechanism called `world`, but its parameters cannot be
/// swept with a `<mechanism>.<param>` key.  Keys without `.`, empty segments
/// (`world.`, `world..x`, `.x`, `m.`) and `<param>` containing `.` are errors.
///
/// # Value rules
///
/// - Every value must be finite.
/// - A `world.` path must name an existing key whose parent tables all exist;
///   no table or key is created.
/// - An existing integer value accepts only integral values within
///   `[-2^63, 2^63)` and stays an integer; an existing float stays a float;
///   booleans, strings, arrays, tables and datetimes cannot be swept.
/// - `<mechanism>` must name at least one `[[mechanism]]` entry.  An existing
///   `<param>` follows the rules above; a missing `<param>` is added as a
///   float (so parameters left at their defaults can be swept).  A typo in
///   `<param>` is therefore not caught here; [`TrialFactory::validate`] is the
///   place to reject unknown parameters.
/// - Two axes with the same key, and an axis without values, are errors.  No
///   axes at all yields one grid point with empty `params` whose effective
///   scenario equals `scenario`.
///
/// # Validation and execution
///
/// Every grid point is patched and passed to [`TrialFactory::validate`]
/// **before any trial runs**; if any point fails, no trial runs at all.  Then
/// the points run in grid order, sequentially; within a point, the seeds run
/// as in [`run_trials`](crate::run_trials) (in parallel when `parallel` is
/// set).  Each point's `summary` is [`summarize`] over its seeds and its
/// `effective` field holds the patched scenario.
///
/// # Errors
///
/// All errors are `SocsimError::Config`: key, value and patch errors read
/// `sweep axis '<key>': <reason>`, validation failures read
/// `sweep combination [<k>=<v>, …]: <error>`, and trial failures read
/// `trial seed <s>: <error>`.
///
/// # Difference from [`run_sweep`](crate::run_sweep)
///
/// The legacy `run_sweep` patches only `[[mechanism]]` params, always writes
/// floats, silently ignores keys without `.` and unknown mechanism names, and
/// never validates.  `run_trial_sweep` rejects all of these instead.
pub fn run_trial_sweep<W: WorldState>(
    scenario: &Scenario,
    axes: &[SweepAxis],
    factory: &dyn TrialFactory<W>,
    seeds: &[u64],
    parallel: bool,
) -> Result<Vec<CheckedSweepPoint>> {
    // Parse and check the axes.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut targets: Vec<SweepTarget> = Vec::with_capacity(axes.len());
    for axis in axes {
        let key = axis.param_key.as_str();
        let target = parse_key(key)?;
        if !seen.insert(key) {
            return Err(axis_error(key, "duplicate axis; each key may appear once"));
        }
        if axis.values.is_empty() {
            return Err(axis_error(key, "axis has no values"));
        }
        targets.push(target);
    }

    // Patch and validate every grid point before running any trial.
    let mut planned: Vec<(Vec<(String, f64)>, Scenario)> = Vec::new();
    for combo in cartesian_product(axes) {
        let mut effective = scenario.clone();
        for ((key, value), target) in combo.iter().zip(&targets) {
            patch(&mut effective, key, target, *value)?;
        }
        factory.validate(&effective).map_err(|e| {
            SocsimError::Config(format!(
                "sweep combination [{}]: {e}",
                format_params(&combo)
            ))
        })?;
        planned.push((combo, effective));
    }

    // Run the grid points in order.
    let mut points = Vec::with_capacity(planned.len());
    for (params, effective) in planned {
        let results = run_validated_trials(&effective, factory, seeds, parallel)?;
        let summary = summarize(&results);
        points.push(CheckedSweepPoint {
            params,
            effective,
            summary,
        });
    }
    Ok(points)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn world(path: &[&str]) -> SweepTarget {
        SweepTarget::World(WorldPath(path.iter().map(|s| (*s).to_owned()).collect()))
    }

    #[test]
    fn parse_key_world_paths() {
        assert_eq!(parse_key("world.n").unwrap(), world(&["n"]));
        assert_eq!(
            parse_key("world.game.temptation").unwrap(),
            world(&["game", "temptation"])
        );
    }

    #[test]
    fn parse_key_mechanism_param() {
        assert_eq!(
            parse_key("record_level.init").unwrap(),
            SweepTarget::Mechanism(MechanismParam {
                mechanism: "record_level".to_owned(),
                param: "init".to_owned(),
            })
        );
        // `world` without the trailing dot is not the reserved prefix.
        assert!(parse_key("worldly.x").is_ok());
    }

    #[test]
    fn parse_key_rejects_bad_grammar() {
        for key in [
            "alpha", "world.", "world..x", "world.x.", ".x", "m.", "m.a.b", "world",
        ] {
            match parse_key(key) {
                Err(SocsimError::Config(msg)) => {
                    assert!(msg.contains(&format!("'{key}'")), "{key}: {msg}")
                }
                other => panic!("{key}: expected a config error, got {other:?}"),
            }
        }
    }

    #[test]
    fn write_existing_integer_bounds() {
        let mut v = toml::Value::Integer(0);
        assert!(write_existing(&mut v, -TWO_POW_63).is_ok());
        assert_eq!(v, toml::Value::Integer(i64::MIN));
        assert!(write_existing(&mut v, TWO_POW_63).is_err());
        assert!(write_existing(&mut v, 2.5).is_err());
        assert!(write_existing(&mut v, 4.0).is_ok());
        assert_eq!(v, toml::Value::Integer(4));
    }
}
