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
use socsim_runner::{RunResult, SweepAxis, SweepPoint};

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
}

// ── Registry / dispatch ─────────────────────────────────────────────────────────

/// Return every pack compiled into this build (gated by Cargo features).
pub fn packs() -> Vec<Box<dyn CliPack>> {
    #[allow(unused_mut)]
    let mut v: Vec<Box<dyn CliPack>> = Vec::new();
    #[cfg(feature = "pack-hr-lifecycle")]
    v.push(Box::new(HrLifecycleCliPack));
    #[cfg(feature = "pack-opinion-dynamics")]
    v.push(Box::new(OpinionDynamicsCliPack));
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
    use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
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
            HR_LIFECYCLE_STARTER
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

    pub(super) const HR_LIFECYCLE_STARTER: &str = r#"# HR Lifecycle Scenario — generated by `socsim init`
# Edit parameters as needed, then run with:
#   socsim run <this-file>

[simulation]
name        = "hr_lifecycle_baseline"
module_pack = "hr-lifecycle"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_teams           = 5
team_size_initial = 8
network_model     = "watts_strogatz"
network_k         = 4
network_beta      = 0.1

[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15

[[mechanism]]
name  = "peer_effect"
phase = "interaction"
[mechanism.params]
alpha_peer = 0.17

[[mechanism]]
name  = "ocb"
phase = "interaction"
[mechanism.params]
alpha_k = 0.30

[[mechanism]]
name  = "fit"
phase = "decision"
[mechanism.params]
rho_pj = 0.20
rho_po = 0.07

[[mechanism]]
name  = "turnover"
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   = 1.0
quit_sat_sens     = 0.8
quit_cascade_bump = 0.30

[[mechanism]]
name  = "knowledge_loss"
phase = "post_step"
[mechanism.params]
phi_tacit  = 0.85
beta_loss  = 1.0
kappa_loss = 0.40

[[mechanism]]
name  = "toxic_spread"
phase = "interaction"
[mechanism.params]
p_toxic  = 0.04
p_spread = 0.46

[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04

[[mechanism]]
name  = "socialization"
phase = "post_step"

[[mechanism]]
name  = "org_performance"
phase = "reward"

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["org_performance", "avg_tenure", "turnover_rate", "knowledge_stock"]
"#;
}

#[cfg(feature = "pack-hr-lifecycle")]
pub use hr_lifecycle::HrLifecycleCliPack;

// ── opinion-dynamics pack ──────────────────────────────────────────────────────

#[cfg(feature = "pack-opinion-dynamics")]
mod opinion_dynamics {
    use super::*;

    use socsim_config::{Params, Registry};
    use socsim_mechanisms::{
        parse_mean, ConvergenceMechanism, DeffuantMechanism, HegselmannKrauseMechanism,
        LorenzMechanism, MeanOperator, SocialJudgementMechanism,
    };
    use socsim_runner::WorldFactory;

    use crate::opinion_world::{OpinionMetricsMechanism, OpinionWorld};

    /// CLI-side wrapper exposing the `opinion-dynamics` pack through [`CliPack`].
    pub struct OpinionDynamicsCliPack;

    impl OpinionDynamicsCliPack {
        /// Build the world factory closure for `OpinionWorld`.
        fn world_factory() -> WorldFactory<OpinionWorld> {
            Box::new(|params: &Params, seed: u64| Ok(OpinionWorld::new(params, seed)))
        }

        /// Register all `opinion-dynamics` mechanisms into a registry.
        ///
        /// Mechanisms (all generic over `ScalarOpinions + Neighbors`, which
        /// `OpinionWorld` implements):
        /// - `hegselmann_krause` — HK bounded confidence (params: `epsilon`,
        ///   `mean` = `A`/`G`/`H`/`P<p>`/`R`, `p` fallback for bare `P`).
        /// - `deffuant` — Deffuant pairwise (params: `epsilon`, `mu`,
        ///   `pairs_per_step`).
        /// - `social_judgement` — assimilation/repulsion (params: `epsilon`,
        ///   `alpha`, `rejection`, `repulsion`).
        /// - `lorenz` — assimilation + polarisation (params: `epsilon`,
        ///   `alpha`, `repulsion`).
        /// - `convergence` — `PostStep` stop on `max|Δx| < tol` (param: `tol`).
        /// - `opinion_metrics` — `PostStep` metrics recorder (param: `tol`).
        fn register(reg: &mut Registry<OpinionWorld>) {
            reg.register("hegselmann_krause", |p: &Params| {
                let epsilon = p.get_f64("epsilon", 0.2);
                let p_fallback = p.get_f64("p", 1.0);
                let mean = parse_mean(p.get_str("mean", "A"), p_fallback)
                    .map_err(socsim_core::SocsimError::Config)?;
                Ok(Box::new(HegselmannKrauseMechanism::new(epsilon, mean))
                    as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
            });
            reg.register("deffuant", |p: &Params| {
                let epsilon = p.get_f64("epsilon", 0.2);
                let mu = p.get_f64("mu", 0.5);
                let pairs = p.get_u64("pairs_per_step", 1) as usize;
                Ok(Box::new(DeffuantMechanism::new(epsilon, mu, pairs))
                    as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
            });
            reg.register("social_judgement", |p: &Params| {
                let epsilon = p.get_f64("epsilon", 0.4);
                let alpha = p.get_f64("alpha", 0.5);
                let rejection = p.get_f64("rejection", 0.8);
                let repulsion = p.get_f64("repulsion", 0.2);
                Ok(
                    Box::new(SocialJudgementMechanism::new(
                        epsilon, alpha, rejection, repulsion,
                    )) as Box<dyn socsim_core::Mechanism<OpinionWorld>>,
                )
            });
            reg.register("lorenz", |p: &Params| {
                let epsilon = p.get_f64("epsilon", 0.4);
                let alpha = p.get_f64("alpha", 0.5);
                let repulsion = p.get_f64("repulsion", 0.2);
                Ok(Box::new(LorenzMechanism::new(epsilon, alpha, repulsion))
                    as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
            });
            reg.register("convergence", |p: &Params| {
                let tol = p.get_f64("tol", 1e-4);
                Ok(Box::new(ConvergenceMechanism::new(tol))
                    as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
            });
            reg.register("opinion_metrics", |p: &Params| {
                let tol = p.get_f64("tol", 0.01);
                Ok(Box::new(OpinionMetricsMechanism::new(tol))
                    as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
            });
            // Silence the unused-import warning when no other path uses it.
            let _ = MeanOperator::Arithmetic;
        }
    }

    impl CliPack for OpinionDynamicsCliPack {
        fn name(&self) -> &'static str {
            "opinion-dynamics"
        }

        fn starter_toml(&self) -> &'static str {
            OPINION_DYNAMICS_STARTER
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

    pub(super) const OPINION_DYNAMICS_STARTER: &str = r#"# Opinion Dynamics Scenario — generated by `socsim init`
# Bounded-confidence consensus (Hegselmann–Krause) on a small-world network.
# Opinions live in [0, 1]; with ε large enough relative to the spread, agents
# coalesce into a small number of clusters (consensus). Run with:
#   socsim run <this-file>

[simulation]
name        = "opinion_dynamics_baseline"
module_pack = "opinion-dynamics"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_agents          = 200
network_model     = "watts_strogatz"
network_k         = 6
network_beta      = 0.1
init_distribution = "uniform"

[[mechanism]]
name  = "hegselmann_krause"
phase = "interaction"
[mechanism.params]
epsilon = 0.25
mean    = "A"

[[mechanism]]
name  = "opinion_metrics"
phase = "post_step"
[mechanism.params]
tol = 0.01

[[mechanism]]
name  = "convergence"
phase = "post_step"
[mechanism.params]
tol = 0.0001

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["clusters", "variance", "spread", "mean"]
"#;
}

#[cfg(feature = "pack-opinion-dynamics")]
pub use opinion_dynamics::OpinionDynamicsCliPack;
