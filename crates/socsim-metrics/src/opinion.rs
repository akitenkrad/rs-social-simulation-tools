//! Opinion-world metric adapters (feature `core`).
//!
//! Bridges the [`stats`](crate::stats) primitives to any world that exposes the
//! [`ScalarOpinions`] capability, plus a generic [`MetricsMechanism`] that
//! records named extractors every `PostStep`.  Everything here is read-only:
//! the helpers borrow `&W`, and the mechanism only writes to the
//! [`Recorder`](socsim_core::Recorder).

use socsim_core::{
    AgentId, Mechanism, Phase, Result, ScalarOpinions, StepContext, WorldState,
};

use crate::stats;

/// Collect every agent's scalar opinion into a `Vec<f64>` in **sorted
/// `AgentId` order**, so the result is deterministic regardless of the world's
/// internal storage.
pub fn collect_opinions<W: ScalarOpinions>(world: &W) -> Vec<f64> {
    let mut ids: Vec<AgentId> = world.agent_ids();
    ids.sort();
    ids.into_iter().map(|id| world.opinion(id)).collect()
}

/// Mean opinion over all agents (see [`stats::mean`]).
pub fn opinion_mean<W: ScalarOpinions>(world: &W) -> f64 {
    stats::mean(&collect_opinions(world))
}

/// Population variance of opinions (see [`stats::variance`]).
pub fn opinion_variance<W: ScalarOpinions>(world: &W) -> f64 {
    stats::variance(&collect_opinions(world))
}

/// Standard deviation of opinions (see [`stats::std_dev`]).
pub fn opinion_std_dev<W: ScalarOpinions>(world: &W) -> f64 {
    stats::std_dev(&collect_opinions(world))
}

/// Spread `max − min` of opinions (see [`stats::spread`]).
pub fn opinion_spread<W: ScalarOpinions>(world: &W) -> f64 {
    stats::spread(&collect_opinions(world))
}

/// Number of distinct opinion clusters at tolerance `tol`
/// (see [`stats::distinct_clusters`]).
pub fn opinion_clusters<W: ScalarOpinions>(world: &W, tol: f64) -> f64 {
    stats::distinct_clusters(&collect_opinions(world), tol) as f64
}

/// Polarization (dispersion convention) of opinions (see [`stats::polarization`]).
pub fn opinion_polarization<W: ScalarOpinions>(world: &W) -> f64 {
    stats::polarization(&collect_opinions(world))
}

/// Extremeness of opinions about `center` (see [`stats::extremeness`]).
pub fn opinion_extremeness<W: ScalarOpinions>(world: &W, center: f64) -> f64 {
    stats::extremeness(&collect_opinions(world), center)
}

/// Bimodality coefficient of opinions (see [`stats::bimodality_coefficient`]).
pub fn opinion_bimodality<W: ScalarOpinions>(world: &W) -> f64 {
    stats::bimodality_coefficient(&collect_opinions(world))
}

// ── MetricsMechanism ──────────────────────────────────────────────────────────

/// Type of a named, read-only metric extractor over a world `W`.
type Extractor<W> = Box<dyn Fn(&W) -> f64 + Send + Sync>;

/// A `PostStep` [`Mechanism`] that records a configurable list of named scalar
/// metrics every step.
///
/// Holds `Vec<(name, extractor)>` where each extractor is a
/// `Fn(&W) -> f64 + Send + Sync`.  On `apply` it evaluates every extractor
/// against the (immutably borrowed) world and records the result via
/// `ctx.recorder.record_metric(ctx.clock.t(), name, value)`.  It never mutates
/// the world, so it has no effect on the simulation trajectory.
///
/// # Example
/// ```ignore
/// use socsim_metrics::opinion::{MetricsMechanism, opinion_mean, opinion_variance};
/// let metrics = MetricsMechanism::new()
///     .with("mean", |w| opinion_mean(w))
///     .with("variance", |w| opinion_variance(w));
/// builder.add_mechanism(metrics);
/// ```
pub struct MetricsMechanism<W: WorldState> {
    extractors: Vec<(String, Extractor<W>)>,
}

impl<W: WorldState> MetricsMechanism<W> {
    /// Create an empty metrics mechanism (records nothing until extractors are
    /// added via [`with`](Self::with)).
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    /// Add a named extractor (builder style).
    pub fn with<F>(mut self, name: impl Into<String>, extractor: F) -> Self
    where
        F: Fn(&W) -> f64 + Send + Sync + 'static,
    {
        self.extractors.push((name.into(), Box::new(extractor)));
        self
    }

    /// Number of registered extractors.
    pub fn len(&self) -> usize {
        self.extractors.len()
    }

    /// Whether no extractors are registered.
    pub fn is_empty(&self) -> bool {
        self.extractors.is_empty()
    }
}

impl<W: WorldState> Default for MetricsMechanism<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: WorldState> Mechanism<W> for MetricsMechanism<W> {
    fn name(&self) -> &str {
        "metrics"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let t = ctx.clock.t();
        for (name, extractor) in &self.extractors {
            let value = extractor(ctx.world);
            ctx.recorder.record_metric(t, name, value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::{Recorder, SimClock, SimRng};

    /// A minimal world: a vector of scalar opinions in agent-id order.
    struct TestWorld {
        clock: SimClock,
        opinions: Vec<f64>,
    }

    impl WorldState for TestWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            (0..self.opinions.len() as u64).map(AgentId).collect()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }

    impl ScalarOpinions for TestWorld {
        fn opinion(&self, id: AgentId) -> f64 {
            self.opinions[id.0 as usize]
        }
        fn set_opinion(&mut self, id: AgentId, value: f64) {
            self.opinions[id.0 as usize] = value;
        }
    }

    /// Recorder that captures (t, key, value) tuples.
    #[derive(Default)]
    struct CaptureRecorder {
        rows: Vec<(u64, String, f64)>,
    }
    impl Recorder for CaptureRecorder {
        fn record_metric(&mut self, t: u64, key: &str, value: f64) {
            self.rows.push((t, key.to_string(), value));
        }
        fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}
    }

    #[test]
    fn collect_is_sorted_by_agent_id() {
        let w = TestWorld {
            clock: SimClock::new(10),
            opinions: vec![0.0, 0.5, 1.0],
        };
        assert_eq!(collect_opinions(&w), vec![0.0, 0.5, 1.0]);
        assert!((opinion_mean(&w) - 0.5).abs() < 1e-12);
        assert_eq!(opinion_clusters(&w, 0.01), 3.0);
    }

    #[test]
    fn metrics_mechanism_records_all_extractors() {
        let mut world = TestWorld {
            clock: SimClock::new(10),
            opinions: vec![0.0, 1.0],
        };
        let mut mech = MetricsMechanism::new()
            .with("mean", |w: &TestWorld| opinion_mean(w))
            .with("variance", |w: &TestWorld| opinion_variance(w));
        assert_eq!(mech.len(), 2);

        let mut rec = CaptureRecorder::default();
        let mut rng = SimRng::from_seed(0);
        let mut scratch = socsim_core::Blackboard::new();
        let mut stop = false;
        let order: Vec<AgentId> = world.agent_ids();
        {
            let mut ctx = StepContext {
                world: &mut world,
                clock: SimClock::new(10),
                rng: &mut rng,
                recorder: &mut rec,
                agent_order: &order,
                scratch: &mut scratch,
                stop: &mut stop,
            };
            mech.apply(Phase::PostStep, &mut ctx).unwrap();
        }
        // mean of [0,1] = 0.5, variance = 0.25, both recorded at t=0.
        assert_eq!(rec.rows.len(), 2);
        assert_eq!(rec.rows[0], (0, "mean".to_string(), 0.5));
        assert_eq!(rec.rows[1], (0, "variance".to_string(), 0.25));
    }
}
