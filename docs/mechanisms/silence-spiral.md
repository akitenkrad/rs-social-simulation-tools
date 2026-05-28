**English** | [日本語](silence-spiral.ja.md)

# Silence spiral (`silence_spiral`)

> Snapshot each employee's neighbour silence ratio $\rho_i$ at the end of the
> Interaction phase, and apply a small downward nudge to perceived
> psychological safety proportional to that ratio — the Noelle-Neumann (1974)
> spiral expressed as a per-step erosion of $\psi$.
> **Phase:** Interaction. **Source:** Noelle-Neumann (1974). **Kind:** empirical ($\epsilon$ spiral magnitude).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`silence_spiral` is the carrier of the spiral-of-silence effect across steps.
It runs in Interaction, after the Decision phase has set every agent's new
`Expression`, and does two things in lockstep for every agent:

1. **Snapshot** $\rho_i$ — the fraction of the agent's network neighbours
   currently in `Silence` — and write it into the agent's per-step field
   `Employee.neighbor_silence_ratio`. This snapshot is the value that *next
   step's* `voice_decision_rule` consumes as the $-\beta_C \cdot \rho_i$
   term of its logit.
2. **Erode** $\psi_i$ by a small amount proportional to $\rho_i$: a high
   local silence ratio nudges perceived psychological safety downward.

The first action turns the within-step Expression state into a cross-step
signal; the second is the *mechanism* by which the spiral makes voicing
progressively harder over time.

## 2. Theory & source

Noelle-Neumann (1974) frames the spiral of silence as the dynamic by which
agents who perceive themselves in a silent (or dissenting) minority become
progressively less willing to voice their views. The local-perception
operationalisation is the neighbour silence ratio:

$$\rho_i(t) = \frac{|\{ j \in N(i) : \text{Expression}_j = \text{Silence}\}|}{|N(i)|}$$

socsim writes $\rho_i$ into the per-agent field at the end of the Interaction
phase and applies a small per-step erosion to $\psi$:

$$\psi_i \leftarrow \operatorname{clip}_{[0,1]}\!\left(\psi_i - \epsilon \cdot \rho_i \cdot 0.05\right)$$

- $\epsilon$ (`epsilon`, default 0.25, the constant `EPSILON_SPIRAL` in
  `calibration.rs`) — the spiral perception magnitude.
- The factor `0.05` is a fixed per-step scale that, multiplied by $\epsilon$,
  gives a maximum per-step erosion of `0.25 · 1.0 · 0.05 = 0.0125` (1.25
  percentage points of $\psi$ per step when every neighbour is silent).
- The result is clamped to $[0, 1]$.

If an agent has no neighbours, `neighbor_silence_ratio(id)` returns 0 (the
helper guards against division by zero), and the agent's $\psi$ is left
unchanged that step.

## 3. Data flow

Reads `Employee.expression` for every agent and `SilenceWorld.network`'s
adjacency lists to compute $\rho_i$. Writes back the new $\rho_i$ into
`Employee.neighbor_silence_ratio` and the updated `Employee.psych_safety` for
every agent. No events are recorded.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the fourth phase. Two ordering invariants apply:

1. **After** the Decision phase, so the $\rho_i$ snapshot reflects this
   step's freshly-drawn `Expression`s rather than the previous step's.
2. **Before** `prefalse_cascade` (also Interaction), because the cascade
   reads agents' `Expression` and `voice_threshold`, not the $\rho_i$
   snapshot — and any change in $\rho_i$ between the two would not be
   visible to the cascade anyway. The bundled scenario declares
   `silence_spiral` before `prefalse_cascade` to make this ordering
   explicit.

The snapshot then survives unchanged into the next step's Decision phase,
where `voice_decision_rule` reads it via `Employee.neighbor_silence_ratio`.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | | Read for every employee in `BTreeMap` order; the count of `Silence` among neighbours produces $\rho_i$. |
| `SilenceWorld.network` | ✓ | | Adjacency list for every agent. |
| `Employee.neighbor_silence_ratio` | | ✓ | Per-step snapshot overwritten in place. |
| `Employee.psych_safety` | ✓ | ✓ | Eroded by $\epsilon \cdot \rho \cdot 0.05$; clamped to [0, 1]. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `voice_decision_rule` (or `voice_decision`) must
  have written this step's `Expression`s before this mechanism runs;
  otherwise $\rho_i$ would reflect the previous step's silence pattern.
- **Downstream (same step):** `prefalse_cascade` reads `Expression`,
  `voice_threshold`, and `private_concern` — none of which this mechanism
  writes. The two Interaction mechanisms are independent in their write set.
- **Downstream (next step):** `voice_decision_rule` reads
  `Employee.neighbor_silence_ratio` as the $-\beta_C \cdot \rho_i$ term of
  its logit. The snapshot is the canonical cross-step carrier of the spiral
  effect.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `epsilon` | `0.25` | empirical (spiral perception magnitude) | Noelle-Neumann (1974) — `EPSILON_SPIRAL` |

The `0.05` per-step scale inside the formula is a compile-time constant; it
is not exposed as a scenario parameter.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "silence_spiral"
phase = "interaction"
[mechanism.params]
epsilon = 0.25                # noelle-neumann:1974
```

Setting `epsilon = 0.0` disables the $\psi$ erosion but still keeps the
$\rho_i$ snapshot (so `voice_decision_rule` can still read it through the
$-\beta_C$ term).

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("silence_spiral", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. To guarantee a deterministic f64 accumulation when
computing $\rho_i$ — which is itself an order-independent count — the
mechanism collects `employees.keys()`, sorts the resulting `Vec<AgentId>`,
and walks the sorted list. The pre-computed `(id, rho)` pairs are then
applied in the same sorted order, so even an implementation change in
`BTreeMap` iteration order would not affect the output.

## 10. Expected behaviour

When most agents are voicing, $\rho_i$ stays near 0 for every agent and
$\psi$ drifts at most marginally — the spiral does little. When silence
becomes locally dense, $\rho_i$ approaches 1 in those neighbourhoods and
$\psi$ erodes by up to a percentage point per step, which feeds back into
next step's voice logit through both the $\beta_\psi \cdot \psi_i$ term
(directly) and the $-\beta_C \cdot \rho_i$ term (via the snapshot). Together
they produce the runaway spiral the design targets: a region of silence
becomes a worse-perceived climate, which makes voicing harder, which keeps
the region silent.

The mechanism's only counter-force in the pack is `prefalse_cascade`, which
can break the spiral in one Interaction phase when neighbour voice ratios
exceed agents' personal thresholds.

## 11. References

- Noelle-Neumann, E. (1974). The spiral of silence: A theory of public
  opinion. *Journal of Communication*, 24(2), 43–51.
