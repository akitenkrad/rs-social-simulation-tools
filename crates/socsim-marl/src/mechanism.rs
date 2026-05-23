//! [`PolicyMechanism`] — wraps a [`Policy`] as a `Decision`-phase
//! [`Mechanism`], the drop-in replacement for a fixed decision heuristic (§14.1).
//!
//! Two modes:
//!
//! - **Inference** ([`PolicyMechanism::inference`]) — greedy actions, no RNG
//!   consumed, no trajectory recording.  A frozen policy behaves like any other
//!   deterministic mechanism and keeps the run bit-reproducible.
//! - **Collect** ([`PolicyMechanism::collecting`]) — samples actions from the
//!   policy and records each `(obs, action)` into a shared
//!   [`TrajectoryBuffer`] for the [`MarlTrainer`](crate::MarlTrainer) to reward.

use std::cell::RefCell;
use std::rc::Rc;

use socsim_core::{Mechanism, Phase, Result, StepContext, WorldState};

use crate::buffer::TrajectoryBuffer;
use crate::policy::{ActionApplier, ObsEncoder, Policy};

/// Decision-phase mechanism backed by a shared [`Policy`].
///
/// Generic over the world `W`, policy `P`, observation encoder `E`, and action
/// applier `A`.  The policy is shared via `Rc<RefCell<_>>` so the trainer can
/// update it between episodes while this mechanism reads it during a run.
pub struct PolicyMechanism<W, P, E, A>
where
    W: WorldState,
    P: Policy,
    E: ObsEncoder<W>,
    A: ActionApplier<W>,
{
    policy: Rc<RefCell<P>>,
    encoder: E,
    applier: A,
    /// `Some` in collect mode: the shared buffer recording trajectories.
    buffer: Option<Rc<RefCell<TrajectoryBuffer>>>,
    _world: std::marker::PhantomData<fn(&W)>,
}

impl<W, P, E, A> PolicyMechanism<W, P, E, A>
where
    W: WorldState,
    P: Policy,
    E: ObsEncoder<W>,
    A: ActionApplier<W>,
{
    /// Inference mode: greedy actions from a (typically frozen) policy.
    pub fn inference(policy: Rc<RefCell<P>>, encoder: E, applier: A) -> Self {
        Self {
            policy,
            encoder,
            applier,
            buffer: None,
            _world: std::marker::PhantomData,
        }
    }

    /// Collect mode: sample actions and record trajectories into `buffer`.
    pub fn collecting(
        policy: Rc<RefCell<P>>,
        encoder: E,
        applier: A,
        buffer: Rc<RefCell<TrajectoryBuffer>>,
    ) -> Self {
        Self {
            policy,
            encoder,
            applier,
            buffer: Some(buffer),
            _world: std::marker::PhantomData,
        }
    }
}

impl<W, P, E, A> Mechanism<W> for PolicyMechanism<W, P, E, A>
where
    W: WorldState,
    P: Policy,
    E: ObsEncoder<W>,
    A: ActionApplier<W>,
{
    fn name(&self) -> &str {
        "policy"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let order: Vec<_> = ctx.agent_order.to_vec();
        for aid in order {
            // Skip agents the encoder reports as non-actionable this step.
            let Some(obs) = self.encoder.encode(ctx.world, aid) else {
                continue;
            };
            let action = match &self.buffer {
                Some(_) => self.policy.borrow().sample(&obs, ctx.rng),
                None => self.policy.borrow().act(&obs),
            };
            self.applier.apply(ctx.world, aid, action, ctx.rng);
            if let Some(buf) = &self.buffer {
                buf.borrow_mut().begin_decision(aid, obs, action);
            }
        }
        Ok(())
    }
}
