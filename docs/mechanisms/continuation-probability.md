**English** | [日本語](continuation-probability.ja.md)

# Continuation probability (`continuation_probability`)

> After each completed step, continue with probability δ and stop with
> probability 1 − δ. **Phase:** PostStep. **Source:** standard indefinitely
> repeated-game / random-lifetime convention. **Kind:** stochastic run control.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`ContinuationProbability` is a domain-independent mechanism from
`socsim-mechanisms`. It works with every `WorldState`: after each step it draws
one uniform value $u$ and calls `StepContext::request_stop()` when $u \geq
\delta$. The current step is therefore always completed, and the probability
of reaching the next step is exactly δ.

The number of completed steps before a probabilistic stop has the geometric
distribution on $1,2,\ldots$:

$$
P(T=t) = \delta^{t-1}(1-\delta),
\qquad E[T] = \frac{1}{1-\delta}\quad (0 \leq \delta < 1).
$$

The simulation clock still imposes `t_max`, so an actual run completes
`min(T, t_max)` steps. At δ = 0 it stops after step 1; at δ = 1 it never
requests a stop.

## 2. Position in the 6-phase loop

The mechanism runs in **PostStep**, after the Environment, Decision,
Interaction and Reward work of the current step. It therefore implements
“continue to another round” rather than “play this round.” Other PostStep
mechanisms run in insertion order. Add it after recorders such as
`game_metrics` when the stopping step must also be recorded; the normal-form
scenario factory does this automatically.

## 3. State read/write contract

| State | Read | Write | Notes |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | Selects the private stream position and timestamps the event. |
| stop flag | | ✓ | Set through `request_stop()` only when $u \geq \delta$. |
| recorder | | ✓ | Emits `continuation_stop` on the stopping step. |
| world domain state | | | Never read or changed. |
| `ctx.rng` | | | Never consumed. |

The event payload is `{"delta": <δ>, "u": <draw>}`. Its event timestamp is
the completed step at which the run stopped.

## 4. Parameters

| Parameter | Type | Default | Meaning |
|---|---|---|---|
| `delta` | finite `f64` in `[0, 1]` | none | Probability that another step follows the completed step. |
| `seed` | `u64` | none | Root run seed used to derive the private stream. |

`ContinuationProbability::new(delta, seed)` returns a configuration error for
values outside `[0, 1]`, NaN and infinities.

## 5. Library use

Pass the same root run seed used to derive the other simulation streams. The
engine can still use its usual derived seed:

```rust,ignore
use socsim_core::derive_seed;
use socsim_engine::SimulationBuilder;
use socsim_mechanisms::ContinuationProbability;

let seed = 42;
let continuation = ContinuationProbability::new(0.9, seed)?;
let mut simulation = SimulationBuilder::new(world)
    .seed(derive_seed(seed, &[1]))
    .add_mechanism(Box::new(domain_mechanism))
    .add_mechanism(Box::new(continuation))
    .build();
simulation.run()?;
```

## 6. Normal-form scenario use

Only the `normal-form` pack exposes this mechanism through scenario TOML:

```toml
[[mechanism]]
name = "continuation_probability"
[mechanism.params]
delta = 0.9
```

It is a `[[mechanism]]` entry, rather than a `[simulation]` key, because it is
an optional composable PostStep mechanism with its own event and RNG stream.
The other packs do not register it.

## 7. Determinism and RNG isolation

The mechanism derives a private `SimRng` from the root seed and the fixed
64-bit label `0x636f6e74696e7565` (the bytes `continue`). It draws exactly one
uniform value per completed step and never uses `ctx.rng`. Consequently, for a
fixed seed, adding continuation probability leaves the world trajectory and
all ordinary records bit-identical to a run without it through the stopping
step.

## 8. Snapshots

Engine snapshots save the world clock, the engine RNG and the stop flag, but
not mechanism objects. `ContinuationProbability` therefore keeps its private
RNG and consumed-draw count synchronized with `clock.t()`. A newly rebuilt
mechanism advances the stream to the saved step before drawing; restoring an
older snapshot rewinds and advances from the derived stream seed. Rebuild with
the same root seed and attach the mechanism once for every step, as in the
original simulation.

## 9. Repeated games and the shadow of the future

For a constant per-round payoff stream, a payoff $x_t$ in round $t$ is reached
with probability $\delta^{t-1}$. Expected lifetime payoffs therefore have the
same weights as conventional δ-discounted repeated-game payoffs. Larger δ
gives later consequences more weight: the “shadow of the future” is longer.

## 10. Testing expectations

A correct integration should check same-seed stopping determinism, δ = 0 and
δ = 1, invalid probabilities, the geometric mean lifetime, private-stream
forward equivalence, and snapshot resume. A stochastic game rule is useful for
the forward-equivalence check because any accidental consumption of the engine
stream becomes visible immediately.
