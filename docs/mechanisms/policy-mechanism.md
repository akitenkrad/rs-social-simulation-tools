**English** | [日本語](policy-mechanism.ja.md)

# Policy mechanism (`policy`)

> A generic Decision-phase wrapper that replaces any fixed heuristic with a
> learnable policy, enabling multi-agent reinforcement learning inside the
> standard simulation loop.
> **Phase:** Decision. **Source:** MARL (§14.1). **Kind:** learnable.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`PolicyMechanism<W, P, E, A>` is a generic Decision-phase mechanism from the
`socsim-marl` crate that wraps a shared `Policy` together with an
`ObsEncoder` (world → observation) and an `ActionApplier` (action → world
mutation). It is a drop-in replacement for any fixed Decision-phase heuristic:
the simulation engine calls `apply()` exactly as it would for any other
mechanism, and the engine requires no changes.

Two operating modes allow the same type to serve both inference and training:

- **Inference mode** — greedy action selection (`policy.act`), no RNG consumed,
  deterministic given a frozen policy.
- **Collect mode** — stochastic sampling (`policy.sample`, using `ctx.rng`) plus
  trajectory recording into a shared `TrajectoryBuffer`, feeding the
  `MarlTrainer` (REINFORCE, `DiscretePolicyNet` burn MLP, CPU).

`PolicyMechanism` is **library-only**: it is not registered in `HrLifecyclePack`
and is not available via the `socsim` binary or scenario TOML files. It must be
constructed in Rust code and added to a `SimulationBuilder` directly. It is
gated behind the `marl` feature flag in `socsim-hr-lifecycle`.

## 2. Theory & source

MARL (§14.1 of the socsim design document) treats each simulation agent as a
reinforcement-learning actor. At each Decision phase the encoder maps the
current world state to a per-agent observation vector; the policy outputs a
discrete action index; the applier translates that index into a world mutation.
Training uses REINFORCE with a `DiscretePolicyNet` (a shallow MLP built with the
`burn` framework, running on CPU).

The two modes are:

```text
Inference mode:
    obs    ← encoder.encode(world, agent_id)       (skip agent if None)
    action ← policy.act(obs)                        (greedy, no RNG)
    applier.apply(world, agent_id, action, ctx.rng)

Collect mode:
    obs    ← encoder.encode(world, agent_id)       (skip agent if None)
    action ← policy.sample(obs, ctx.rng)            (stochastic, uses ctx.rng)
    applier.apply(world, agent_id, action, ctx.rng)
    buffer.begin_decision(agent_id, obs, action)    (record for trainer)
```

The policy is shared via `Rc<RefCell<Policy>>` so the `MarlTrainer` can update
weights between episodes while the mechanism holds the same reference during a
run.

## 3. Data flow

![policy data flow](../assets/mech-policy-mechanism.svg)

State read and written depends entirely on the `ObsEncoder` and `ActionApplier`
implementations provided by the caller. The mechanism itself does not directly
touch any named world fields; all world access flows through the generic
encoder/applier pair.

## 4. Position in the 6-phase loop

Runs in **Decision**, the third phase. This positions it alongside `fit`,
`turnover`, and `hiring` in the HR lifecycle pack. The Declaration order within
Decision determines execution order; `PolicyMechanism` should be placed to
respect whatever dependencies its `ActionApplier` introduces (for example, if
the applier modifies `satisfaction`, it should run after `fit` has updated that
field, or before, depending on design intent).

## 5. State read/write contract

The contract is **generic**: it depends on the concrete `ObsEncoder<W>` and
`ActionApplier<W>` types supplied by the caller.

| Operation | Actor | Notes |
|---|---|---|
| Read world state | `encoder.encode(world, aid)` | Returns `Option<obs>`; `None` skips agent. |
| Write world state | `applier.apply(world, aid, action, ctx.rng)` | Any mutation allowed. |
| Write trajectory | `buffer.begin_decision(aid, obs, action)` | Collect mode only. |

There is no fixed field contract at the `PolicyMechanism` level. Document the
contract in the encoder and applier implementations.

## 6. Dependencies & ordering constraints

- **Upstream:** whatever the `ObsEncoder` reads must be current. If the encoder
  reads `Employee.productivity`, then `learning_curve` and `peer_effect` must
  have run first — but those are Environment and Interaction mechanisms, which
  fire before Decision, so the phase ordering handles this automatically.
- **Downstream:** whatever the `ActionApplier` writes will be consumed by
  subsequent mechanisms. Declare `PolicyMechanism` before those consumers within
  the Decision phase if needed.
- **Training loop:** in collect mode, `MarlTrainer` must call
  `buffer.close_step(rewards)` after the step and run a training update between
  episodes. See `library.md#learnable-policies-marl` for the full
  training-loop pattern.
- **Feature flag:** add `socsim-marl` (with `marl` feature) to `Cargo.toml`.

## 7. Parameters

`PolicyMechanism` has no scenario-TOML parameters. The policy weights, network
architecture, and training hyper-parameters are managed by the `MarlTrainer`
and the `Policy` implementation, not by the mechanism registry.

## 8. How to apply

`PolicyMechanism` is **library-only** — there is no `[[mechanism]]` TOML block.
Construct it in Rust and add it to `SimulationBuilder` directly.

### Library mode

```rust
use std::cell::RefCell;
use std::rc::Rc;

use socsim_marl::{PolicyMechanism, TrajectoryBuffer};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// Your encoder and applier implementations.
let encoder = MyObsEncoder::new();
let applier = MyActionApplier::new();

// Shared policy (e.g. a trained DiscretePolicyNet loaded from disk).
let policy = Rc::new(RefCell::new(my_policy));

// --- Inference mode (frozen policy, no RNG, bit-reproducible) ---
let infer_mech = PolicyMechanism::inference(
    Rc::clone(&policy),
    encoder.clone(),
    applier.clone(),
);

// --- Collect mode (stochastic, records trajectories for MarlTrainer) ---
let buffer = Rc::new(RefCell::new(TrajectoryBuffer::new()));
let collect_mech = PolicyMechanism::collecting(
    Rc::clone(&policy),
    encoder,
    applier,
    Rc::clone(&buffer),
);

// Add to SimulationBuilder like any other mechanism.
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(collect_mech)
    .build();
sim.run()?;
```

For the full training loop (REINFORCE update, reward assignment, episode reset)
see [Learnable policies (MARL)](../library.md#learnable-policies-marl).

## 9. Determinism & RNG

**Inference mode:** draws **no** randomness (`policy.act` is greedy and
deterministic). A run with a frozen policy is bit-reproducible given the same
world state.

**Collect mode:** draws from `ctx.rng` via `policy.sample(obs, ctx.rng)` once
per actionable agent per step. The iteration order is `ctx.agent_order`, which
is determined by the simulation scheduler (typically a random permutation drawn
earlier in the step setup, before any mechanism fires). Reproducibility in
collect mode therefore requires the same seed **and** the same agent-order
schedule.

The `ActionApplier` may also consume `ctx.rng` if it requires stochastic world
mutations; document this in the applier implementation.

## 10. Expected behaviour

In inference mode with a well-trained policy the simulation should produce
higher `org_performance` or lower `turnover_rate` (depending on the reward
signal) than a fixed-heuristic baseline, reflecting the policy's learned
strategy. In collect mode during early training, behaviour is essentially
random; as REINFORCE converges the policy should shift toward the trained
objective.

## 11. References

- socsim design document, §14.1 — Learnable policies (MARL).
- Williams, R. J. (1992). Simple statistical gradient-following algorithms for
  connectionist reinforcement learning. *Machine Learning*, 8(3–4), 229–256.
  (REINFORCE algorithm)
