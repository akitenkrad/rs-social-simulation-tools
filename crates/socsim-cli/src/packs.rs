//! World-polymorphic module-pack registry for the `socsim` CLI.
//!
//! The CLI is no longer monomorphized over a single concrete world type.
//! Each module pack is exposed through the object-safe [`CliPack`] trait,
//! which erases the concrete world `W` behind world-agnostic runner types
//! ([`RunResult`], [`SweepPoint`], …).  A pack implementation builds its own
//! `WorldFactory<W>` + register closure internally and calls the generic
//! [`socsim_runner::run_seeds`] / [`socsim_runner::run_sweep`] specialized to
//! its concrete world.
//!
//! Add new packs by:
//! 1. Implementing a `struct FooCliPack;` that `impl CliPack`.
//! 2. Adding a Cargo feature `pack-foo = ["dep:socsim-foo"]`.
//! 3. Gating the impl + registry entry behind `#[cfg(feature = "pack-foo")]`.
//! 4. Pushing an entry into [`packs`].

use anyhow::{bail, Result};

use socsim_config::Scenario;
use socsim_runner::{RunResult, Summary, SweepAxis, SweepPoint};

// ── SweepRow ───────────────────────────────────────────────────────────────────

/// One grid combination of a `socsim sweep`, as consumed by the CLI.
///
/// This is the CLI-side result type of [`CliPack::run_sweep_rows`].  It carries
/// the same `params` and `summary` as a [`SweepPoint`], plus the effective
/// (patched) scenario for packs that can report it.
#[derive(Debug, Clone)]
pub struct SweepRow {
    /// Parameter values used for this grid point (in sweep axis order).
    pub params: Vec<(String, f64)>,
    /// Aggregated summary across all seeds for this grid combination.
    pub summary: Summary,
    /// The effective scenario after the sweep patch was applied.
    ///
    /// `None` for packs that run the legacy [`CliPack::run_sweep`] path, which
    /// does not report the patched scenario.  When `Some`, `socsim sweep`
    /// writes it next to the combination's CSV as
    /// `combo_{i:04}_{label}.scenario.toml`.
    pub effective: Option<Scenario>,
}

impl From<SweepPoint> for SweepRow {
    fn from(point: SweepPoint) -> Self {
        Self {
            params: point.params,
            summary: point.summary,
            effective: None,
        }
    }
}

// ── CliPack trait ──────────────────────────────────────────────────────────────

/// An object-safe, world-erased module pack for the CLI.
///
/// Implementations own a concrete world type internally but never expose it in
/// any signature, so `Box<dyn CliPack>` is object-safe and the CLI binary is
/// not generic over any one domain model.
pub trait CliPack: Send + Sync {
    /// Stable pack name as used in `[simulation] module_pack = "..."`.
    fn name(&self) -> &'static str;

    /// Starter scenario TOML emitted by `socsim init --module-pack <name>`.
    fn starter_toml(&self) -> &'static str;

    /// Sorted names of all mechanisms this pack registers.
    fn mechanism_names(&self) -> Vec<String>;

    /// Validate pack-specific scenario configuration.
    ///
    /// The CLI calls this after [`Scenario::validate`]. Packs without
    /// additional validation keep this no-op default.
    fn validate(&self, _scenario: &Scenario) -> Result<()> {
        Ok(())
    }

    /// Run the scenario over the given seeds, returning per-seed results.
    fn run_seeds(
        &self,
        scenario: &Scenario,
        seeds: &[u64],
        parallel: bool,
    ) -> Result<Vec<RunResult>>;

    /// Run a grid parameter sweep over the given axes and seeds.
    fn run_sweep(
        &self,
        scenario: &Scenario,
        axes: &[SweepAxis],
        seeds: &[u64],
        parallel: bool,
    ) -> Result<Vec<SweepPoint>>;

    /// Run a grid parameter sweep and return one [`SweepRow`] per combination.
    ///
    /// This is the entry point used by `socsim sweep`.  The default
    /// implementation delegates to [`CliPack::run_sweep`] and maps every
    /// [`SweepPoint`] to a row with `effective: None`, so packs that do not
    /// override it keep their existing sweep behaviour.  A pack that can report
    /// the effective (patched) scenario of each combination overrides this
    /// method and returns `effective: Some(..)`.
    fn run_sweep_rows(
        &self,
        scenario: &Scenario,
        axes: &[SweepAxis],
        seeds: &[u64],
        parallel: bool,
    ) -> Result<Vec<SweepRow>> {
        let points = self.run_sweep(scenario, axes, seeds, parallel)?;
        Ok(points.into_iter().map(SweepRow::from).collect())
    }
}

// ── Registry / dispatch ─────────────────────────────────────────────────────────

/// Return every pack compiled into this build (gated by Cargo features).
#[allow(clippy::vec_init_then_push)]
pub fn packs() -> Vec<Box<dyn CliPack>> {
    #[allow(unused_mut)]
    let mut v: Vec<Box<dyn CliPack>> = Vec::new();
    #[cfg(feature = "pack-hr-lifecycle")]
    v.push(Box::new(HrLifecycleCliPack));
    #[cfg(feature = "pack-opinion-dynamics")]
    v.push(Box::new(OpinionDynamicsCliPack));
    #[cfg(feature = "pack-organizational-silence")]
    v.push(Box::new(OrganizationalSilenceCliPack));
    #[cfg(feature = "pack-normal-form")]
    v.push(Box::new(NormalFormCliPack));
    v
}

/// Return the names of all known module packs.
pub fn known_packs() -> Vec<&'static str> {
    packs().iter().map(|p| p.name()).collect()
}

/// Look up a pack by name.
///
/// # Errors
///
/// Returns `Err` when `name` is not a known (compiled-in) pack.
pub fn dispatch(name: &str) -> Result<Box<dyn CliPack>> {
    if let Some(p) = packs().into_iter().find(|p| p.name() == name) {
        return Ok(p);
    }
    bail!(
        "unknown module pack '{name}'; known packs: {}",
        known_packs().join(", ")
    )
}

/// Return a starter scenario TOML for the named pack.
///
/// # Errors
///
/// Returns `Err` when the pack is unknown.
pub fn starter_toml(name: &str) -> Result<&'static str> {
    Ok(dispatch(name)?.starter_toml())
}

// ── hr-lifecycle pack ────────────────────────────────────────────────────────────

#[cfg(feature = "pack-hr-lifecycle")]
mod hr_lifecycle {
    use super::*;

    use socsim_config::{ModulePack, Params, Registry};
    use socsim_core::SimRng;
    use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
    use socsim_runner::WorldFactory;

    /// CLI-side wrapper exposing the `hr-lifecycle` pack through [`CliPack`].
    pub struct HrLifecycleCliPack;

    impl HrLifecycleCliPack {
        /// Build the world factory closure for `HrWorld`.
        fn world_factory() -> WorldFactory<HrWorld> {
            Box::new(|params: &Params, seed: u64| {
                let n_teams = params.get_u64("n_teams", 5) as usize;
                let team_size = params.get_u64("team_size_initial", 8) as usize;
                let ws_k = params.get_u64("network_k", 4) as usize;
                let ws_beta = params.get_f64("network_beta", 0.1);
                let mut rng = SimRng::from_seed(seed);
                let world = HrWorld::new(n_teams, team_size, ws_k, ws_beta, &mut rng);
                Ok(world)
            })
        }

        /// Register all `hr-lifecycle` mechanisms into a registry.
        fn register(reg: &mut Registry<HrWorld>) {
            HrLifecyclePack.register(reg);
        }
    }

    impl CliPack for HrLifecycleCliPack {
        fn name(&self) -> &'static str {
            "hr-lifecycle"
        }

        fn starter_toml(&self) -> &'static str {
            socsim_packs::hr_lifecycle::HR_LIFECYCLE_STARTER
        }

        fn mechanism_names(&self) -> Vec<String> {
            let mut reg: Registry<HrWorld> = Registry::new();
            Self::register(&mut reg);
            let mut names: Vec<String> = reg.names().into_iter().map(|s| s.to_owned()).collect();
            names.sort();
            names
        }

        fn run_seeds(
            &self,
            scenario: &Scenario,
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<RunResult>> {
            let factory = Self::world_factory();
            let results = socsim_runner::run_seeds::<HrWorld>(
                scenario,
                &factory,
                &Self::register,
                seeds.iter().copied(),
                parallel,
            )?;
            Ok(results)
        }

        fn run_sweep(
            &self,
            scenario: &Scenario,
            axes: &[SweepAxis],
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            let factory = Self::world_factory();
            let points = socsim_runner::run_sweep::<HrWorld>(
                scenario,
                axes,
                &factory,
                &Self::register,
                seeds.to_vec(),
                parallel,
            )?;
            Ok(points)
        }
    }
}

#[cfg(feature = "pack-hr-lifecycle")]
pub use hr_lifecycle::HrLifecycleCliPack;

// ── opinion-dynamics pack ──────────────────────────────────────────────────────

#[cfg(feature = "pack-opinion-dynamics")]
mod opinion_dynamics {
    use super::*;

    use socsim_config::{Params, Registry};
    use socsim_packs::opinion::{self, OpinionWorld};
    use socsim_runner::WorldFactory;

    /// CLI-side wrapper exposing the `opinion-dynamics` pack through [`CliPack`].
    pub struct OpinionDynamicsCliPack;

    impl OpinionDynamicsCliPack {
        /// Build the world factory closure for `OpinionWorld`.
        fn world_factory() -> WorldFactory<OpinionWorld> {
            Box::new(|params: &Params, seed: u64| Ok(OpinionWorld::new(params, seed)))
        }

        /// Register all `opinion-dynamics` mechanisms into a registry.
        fn register(reg: &mut Registry<OpinionWorld>) {
            opinion::register(reg);
        }
    }

    impl CliPack for OpinionDynamicsCliPack {
        fn name(&self) -> &'static str {
            "opinion-dynamics"
        }

        fn starter_toml(&self) -> &'static str {
            opinion::OPINION_DYNAMICS_STARTER
        }

        fn mechanism_names(&self) -> Vec<String> {
            let mut reg: Registry<OpinionWorld> = Registry::new();
            Self::register(&mut reg);
            let mut names: Vec<String> = reg.names().into_iter().map(|s| s.to_owned()).collect();
            names.sort();
            names
        }

        fn run_seeds(
            &self,
            scenario: &Scenario,
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<RunResult>> {
            let factory = Self::world_factory();
            let results = socsim_runner::run_seeds::<OpinionWorld>(
                scenario,
                &factory,
                &Self::register,
                seeds.iter().copied(),
                parallel,
            )?;
            Ok(results)
        }

        fn run_sweep(
            &self,
            scenario: &Scenario,
            axes: &[SweepAxis],
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            let factory = Self::world_factory();
            let points = socsim_runner::run_sweep::<OpinionWorld>(
                scenario,
                axes,
                &factory,
                &Self::register,
                seeds.to_vec(),
                parallel,
            )?;
            Ok(points)
        }
    }
}

#[cfg(feature = "pack-opinion-dynamics")]
pub use opinion_dynamics::OpinionDynamicsCliPack;

// ── organizational-silence pack ────────────────────────────────────────────────

#[cfg(feature = "pack-organizational-silence")]
mod organizational_silence {
    use super::*;

    use socsim_config::{ModulePack, Params, Registry};
    use socsim_core::SimRng;
    use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
    use socsim_runner::WorldFactory;

    /// CLI-side wrapper exposing the `organizational-silence` pack through
    /// [`CliPack`].
    pub struct OrganizationalSilenceCliPack;

    impl OrganizationalSilenceCliPack {
        /// Build the world factory closure for [`SilenceWorld`].
        fn world_factory() -> WorldFactory<SilenceWorld> {
            Box::new(|params: &Params, seed: u64| {
                let n_teams = params.get_u64("n_teams", 5) as usize;
                let team_size = params.get_u64("team_size_initial", 8) as usize;
                let n_levels = params.get_u64("n_levels", 3) as u8;
                let ws_k = params.get_u64("network_k", 6) as usize;
                let ws_beta = params.get_f64("network_beta", 0.1);
                let supervisor_homogeneity = params.get_f64("supervisor_homogeneity", 0.5);
                let mut rng = SimRng::from_seed(seed);
                let world = SilenceWorld::new(
                    n_teams,
                    team_size,
                    n_levels,
                    ws_k,
                    ws_beta,
                    supervisor_homogeneity,
                    &mut rng,
                );
                Ok(world)
            })
        }

        /// Register all `organizational-silence` mechanisms into a registry.
        fn register(reg: &mut Registry<SilenceWorld>) {
            OrganizationalSilencePack.register(reg);
        }
    }

    impl CliPack for OrganizationalSilenceCliPack {
        fn name(&self) -> &'static str {
            "organizational-silence"
        }

        fn starter_toml(&self) -> &'static str {
            socsim_packs::organizational_silence::ORGANIZATIONAL_SILENCE_STARTER
        }

        fn mechanism_names(&self) -> Vec<String> {
            let mut reg: Registry<SilenceWorld> = Registry::new();
            Self::register(&mut reg);
            let mut names: Vec<String> = reg.names().into_iter().map(|s| s.to_owned()).collect();
            names.sort();
            names
        }

        fn run_seeds(
            &self,
            scenario: &Scenario,
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<RunResult>> {
            let factory = Self::world_factory();
            let results = socsim_runner::run_seeds::<SilenceWorld>(
                scenario,
                &factory,
                &Self::register,
                seeds.iter().copied(),
                parallel,
            )?;
            Ok(results)
        }

        fn run_sweep(
            &self,
            scenario: &Scenario,
            axes: &[SweepAxis],
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            let factory = Self::world_factory();
            let points = socsim_runner::run_sweep::<SilenceWorld>(
                scenario,
                axes,
                &factory,
                &Self::register,
                seeds.to_vec(),
                parallel,
            )?;
            Ok(points)
        }
    }
}

#[cfg(feature = "pack-organizational-silence")]
pub use organizational_silence::OrganizationalSilenceCliPack;

// ── normal-form pack ───────────────────────────────────────────────────────────

#[cfg(feature = "pack-normal-form")]
mod normal_form {
    use super::*;

    use socsim_packs::normal_form::{NormalFormPack, NORMAL_FORM_STARTER};

    /// CLI-side wrapper exposing the `normal-form` pack through [`CliPack`].
    pub struct NormalFormCliPack;

    impl CliPack for NormalFormCliPack {
        fn name(&self) -> &'static str {
            "normal-form"
        }

        fn starter_toml(&self) -> &'static str {
            NORMAL_FORM_STARTER
        }

        fn mechanism_names(&self) -> Vec<String> {
            let mut names = NormalFormPack.mechanism_names();
            names.sort();
            names
        }

        fn validate(&self, scenario: &Scenario) -> Result<()> {
            NormalFormPack.validate(scenario)?;
            Ok(())
        }

        fn run_seeds(
            &self,
            scenario: &Scenario,
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<RunResult>> {
            Ok(NormalFormPack.run_trials(scenario, seeds, parallel)?)
        }

        fn run_sweep(
            &self,
            scenario: &Scenario,
            axes: &[SweepAxis],
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            let points = NormalFormPack.run_sweep(scenario, axes, seeds, parallel)?;
            Ok(points
                .into_iter()
                .map(|point| SweepPoint {
                    params: point.params,
                    summary: point.summary,
                })
                .collect())
        }

        fn run_sweep_rows(
            &self,
            scenario: &Scenario,
            axes: &[SweepAxis],
            seeds: &[u64],
            parallel: bool,
        ) -> Result<Vec<SweepRow>> {
            let points = NormalFormPack.run_sweep(scenario, axes, seeds, parallel)?;
            Ok(points
                .into_iter()
                .map(|point| SweepRow {
                    params: point.params,
                    summary: point.summary,
                    effective: Some(point.effective),
                })
                .collect())
        }
    }
}

#[cfg(feature = "pack-normal-form")]
pub use normal_form::NormalFormCliPack;
