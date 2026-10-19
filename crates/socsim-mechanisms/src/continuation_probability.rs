//! Probabilistic termination with a private random-number stream.
//!
//! [`ContinuationProbability`] implements the continuation convention used in
//! indefinitely repeated games and random-lifetime experiments: after each
//! completed step the run continues with probability `delta` and stops with
//! probability `1 - delta`.

use rand::Rng;
use socsim_core::{
    derive_seed, Mechanism, Phase, Result, SimRng, SocsimError, StepContext, WorldState,
};

/// Fixed label used to derive the mechanism's private random-number stream.
///
/// The hexadecimal bytes spell `continue`. The label is deliberately outside
/// the small integer labels used by the engine's world, scheduler, and decision
/// streams.
const CONTINUATION_STREAM_LABEL: u64 = 0x636f_6e74_696e_7565;

/// Stops a simulation after each step with probability `1 - delta`.
///
/// The mechanism runs in [`Phase::PostStep`]. It draws one uniform variate
/// `u` per completed step from a private [`SimRng`] and calls
/// [`StepContext::request_stop`] when `u >= delta`. A stop is recorded as a
/// `continuation_stop` event whose timestamp is the completed step and whose
/// payload contains `delta` and `u`.
///
/// # Random-stream isolation
///
/// [`ContinuationProbability::new`] accepts the run's root seed and derives a
/// private stream with [`derive_seed`] and a fixed label. It never consumes
/// [`StepContext::rng`], so adding this mechanism cannot change the trajectory
/// produced by other mechanisms before the stopping step.
///
/// # Snapshots
///
/// Engine snapshots deliberately do not serialize mechanisms. To make the
/// private stream resumable anyway, this mechanism tracks how many variates it
/// has consumed and synchronizes that count with the world's clock before each
/// draw. A freshly rebuilt mechanism advances to the saved clock position; a
/// mechanism restored to an older snapshot rewinds by rebuilding its stream.
/// Rebuilding the simulation with the same root seed therefore resumes the
/// next continuation draw exactly.
#[derive(Clone, Debug)]
pub struct ContinuationProbability {
    delta: f64,
    stream_seed: u64,
    rng: SimRng,
    draws: u64,
}

impl ContinuationProbability {
    /// Creates a continuation mechanism for probability `delta` and run
    /// `seed`.
    ///
    /// `delta` must be finite and lie in the closed interval `[0, 1]`.
    /// `delta = 0` stops after the first step; `delta = 1` never requests a
    /// stop.
    pub fn new(delta: f64, seed: u64) -> Result<Self> {
        if !delta.is_finite() || !(0.0..=1.0).contains(&delta) {
            return Err(SocsimError::Config(format!(
                "continuation probability must be finite and in [0, 1], got {delta}"
            )));
        }
        let stream_seed = derive_seed(seed, &[CONTINUATION_STREAM_LABEL]);
        Ok(Self {
            delta,
            stream_seed,
            rng: SimRng::from_seed(stream_seed),
            draws: 0,
        })
    }

    /// Returns the probability of continuing to the next step.
    pub fn delta(&self) -> f64 {
        self.delta
    }

    /// Draws the variate assigned to completed step `t`.
    ///
    /// Immediately before step `t`'s draw, exactly `t - 1` earlier draws must
    /// have been consumed. The clock therefore provides the stream position
    /// omitted from engine snapshots.
    fn draw_for_step(&mut self, t: u64) -> f64 {
        let earlier_draws = t.saturating_sub(1);
        if self.draws > earlier_draws {
            self.rng = SimRng::from_seed(self.stream_seed);
            self.draws = 0;
        }
        while self.draws < earlier_draws {
            let _ = self.rng.gen::<f64>();
            self.draws += 1;
        }
        let u = self.rng.gen::<f64>();
        self.draws += 1;
        u
    }
}

impl<W: WorldState> Mechanism<W> for ContinuationProbability {
    fn name(&self) -> &str {
        "continuation_probability"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let u = self.draw_for_step(ctx.clock.t());
        if u >= self.delta {
            ctx.recorder.record_event(
                ctx.clock.t(),
                "continuation_stop",
                serde_json::json!({ "delta": self.delta, "u": u }),
            );
            ctx.request_stop();
        }
        Ok(())
    }
}
