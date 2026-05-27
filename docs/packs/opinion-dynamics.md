**English** | [日本語](opinion-dynamics.ja.md)

# `opinion-dynamics` pack

> **Bounded-confidence opinion formation** on a social network: agents hold a scalar opinion in `[0, 1]` and pull toward the opinions of neighbours that are close enough to their own. Depending on the mechanism and ε, the population reaches consensus, settles into clusters, or polarises.
> **World:** `OpinionWorld`. **Mechanisms:** four opinion updates + two utilities. **Cargo feature:** `pack-opinion-dynamics` (on by default).

[← Back to the pack catalog](../packs.md)

## 1. Overview

The `opinion-dynamics` pack runs the classic **bounded-confidence** family of
opinion models. Every agent carries one scalar opinion; on each step it moves
toward the opinions of network neighbours whose views fall within a confidence
bound ε, ignoring those too far away. That single rule, applied repeatedly,
reproduces the three canonical outcomes of the literature — global consensus,
fragmentation into a few stable clusters, and persistent polarisation.

Unlike [`hr-lifecycle`](hr-lifecycle.md), this pack owns almost no bespoke
logic. Its world implements two small **capability traits**, and the actual
update rules are the domain-agnostic mechanisms from the
[`socsim-mechanisms`](../mechanisms.md) catalog — so the same mechanism code
could run on any other world that exposes the same capabilities.

## 2. The world: `OpinionWorld`

![OpinionWorld and capability-trait decoupling](../assets/pack-opinion-dynamics-world.svg)

`OpinionWorld` is deliberately tiny:

| Field | Type | Models |
|---|---|---|
| `opinions` | `Vec<f64>` | per-agent scalar opinion, indexed by `AgentId` |
| `net` | `SocialNetwork` | the tie graph opinions diffuse across |
| `clock` | `SimClock` | the simulation clock |
| `last_max_delta` | `f64` | the largest single-step opinion change (a convergence diagnostic) |

It is built by `OpinionWorld::new(params, seed)` from the scenario's `[world]`
block, which chooses:

- **`n_agents`** — population size (default 100; the starter uses 200).
- **`network_model`** — `watts_strogatz` (default), `erdos_renyi`, or
  `barabasi_albert`, with the usual per-model parameters (`network_k`,
  `network_beta`, `network_p`, `network_m`).
- **`init_distribution`** — `uniform` (default), `normal` (triangular, centred
  at 0.5), or `polarized` (bimodal near 0 and 1). All initial opinions live in
  `[0, 1]`.

### Capability-trait decoupling

`OpinionWorld` implements three traits from [`socsim-core`](../library.md):

- **`WorldState`** — the base contract (`agent_ids`, `clock`).
- **`ScalarOpinions`** — `opinion(id) -> f64` and `set_opinion(id, value)`.
- **`Neighbors`** — `neighbors_of(id) -> Vec<AgentId>`.

The opinion mechanisms are written generically as
`impl<W: ScalarOpinions + Neighbors> Mechanism<W>`, so they touch the world
*only* through these methods and never see that opinions happen to live in a
`Vec<f64>`. This is the same trait-based decoupling described in the
[architecture overview](../architecture.md): the
[`socsim-mechanisms`](../mechanisms.md) catalog also defines `BinaryState`
(contagion), `CultureVectors` (Axelrod), and `GroupMembership` (group dynamics)
for worlds in those domains — this pack simply doesn't implement them.

## 3. Mechanisms in the pack

The pack's [`register`](../tutorials/05-scenario-pack.md) wires in **four**
interchangeable opinion-update mechanisms (enable whichever you want to study)
plus **two** PostStep utilities.

| Mechanism | Phase | Behaviour | Catalog page |
|---|---|---|---|
| `hegselmann_krause` | Interaction | Synchronous update toward the mean of all opinions within ε | [→](../mechanisms/hegselmann-krause.md) |
| `deffuant` | Interaction | Pairwise: two agents within ε converge by a rate μ | [→](../mechanisms/deffuant.md) |
| `social_judgement` | Interaction | Assimilate inside ε, *repel* in the rejection region → polarisation | [→](../mechanisms/social-judgement.md) |
| `lorenz` | Interaction | Assimilation plus a self-reinforcing term that amplifies extremes | [→](../mechanisms/lorenz.md) |
| `convergence` | PostStep | Utility: stops the run when `max|Δx| < tol` | — |
| `opinion_metrics` | PostStep | Pack-specific: records the per-step metrics (§4) | — |

The four opinion mechanisms are members of the `socsim-mechanisms`
opinion-dynamics feature family (which also offers the A/G/H/P/R
[`MeanOperator`](../mechanisms/hegselmann-krause.md) family for Hegselmann–Krause).
The pack does **not** register the contagion, cultural, or group-dynamics
mechanisms — those need world capabilities `OpinionWorld` doesn't provide.

## 4. The pipeline & metrics

![opinion-dynamics mechanism pipeline](../assets/pack-opinion-dynamics-pipeline.svg)

A step is simple: the chosen opinion mechanism runs in **Interaction**, then two
utilities run in **PostStep**. `opinion_metrics` records five scalars every
step (computed via the [`socsim-metrics`](../architecture.md) stats helpers),
and `convergence` halts the run early once opinions stop moving:

| Metric | Meaning |
|---|---|
| `clusters` | Number of distinct opinion groups within a tolerance `tol`. |
| `variance` | Population variance of opinions. |
| `spread` | `max − min` of opinions. |
| `mean` | Arithmetic mean of opinions. |
| `max_delta` | Largest single-step opinion change (also cached on `world.last_max_delta`). |

`clusters` collapsing toward 1 signals consensus; a stable count above 1 is
fragmentation; a rising `variance`/`spread` under `social_judgement` or `lorenz`
is polarisation.

## 5. How to apply

### Scenario / CLI

```sh
socsim init --module-pack opinion-dynamics --out scenarios/op.toml
socsim run scenarios/op.toml
```

The starter scenario runs Hegselmann–Krause consensus on a 200-agent
small-world network:

```toml
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
```

To study a different model, swap the Interaction mechanism — e.g. replace the
`hegselmann_krause` block with a `deffuant` (add `mu`, `pairs_per_step`),
`social_judgement` (`alpha`, `rejection`, `repulsion`), or `lorenz` block. To
explore the consensus→fragmentation transition, sweep ε:

```sh
socsim sweep scenarios/op.toml --axis hegselmann_krause.epsilon=0.05..0.4:8 --seeds 0..20
```

### Library

```rust
use socsim_config::{Params, Registry};
use socsim_packs::opinion::{self, OpinionWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<OpinionWorld> = Registry::new();
opinion::register(&mut reg);

let world = OpinionWorld::new(&world_params, 42);
let mut hk_params = Params::empty();
hk_params.set("epsilon", 0.25_f64);

let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(reg.build("hegselmann_krause", &hk_params)?)
    .add_mechanism(reg.build("opinion_metrics", &Params::empty())?)
    .build();
sim.run()?;
```

The [T2 — Opinion network](../tutorials/02-opinion-network.md) tutorial builds an
opinion model step by step, and the [use-cases page](../usecases.md) shows the
expected metric series for the baseline run.

## 6. See also

- [Mechanism catalog](../mechanisms.md) — the opinion-dynamics mechanism family in full (theory, equations, diagrams).
- [hr-lifecycle pack](hr-lifecycle.md) — the other bundled pack.
- [T2 — Opinion network](../tutorials/02-opinion-network.md) — a guided opinion-dynamics build.
- [Use cases & recipes](../usecases.md) · [CLI reference](../cli.md) · [Architecture](../architecture.md)
