**English** | [日本語](architecture.ja.md)

# Architecture

---

## Crate workspace

The workspace contains eleven crates organised in three layers:

```
socsim-cli          ← binary (entry point)
    └── socsim-runner      ← multi-seed runs, sweeps, summarise
            ├── socsim-engine      ← Simulation, SimulationBuilder, schedulers
            │       └── socsim-log         ← InMemoryRecorder, JsonlRecorder, CsvRecorder
            ├── socsim-config      ← Params, Registry, ModulePack, Scenario loader
            │       └── socsim-core        ← traits (Mechanism, WorldState, …), AgentId, Phase, Blackboard
            ├── socsim-hr-lifecycle ← reference module (10 mechanisms)
            │       └── socsim-net         ← SocialNetwork (ER, WS, BA generators)
            ├── socsim-grid        ← Grid, GridIndex, neighbourhoods, distances (spatial models)
            ├── socsim-marl        ← learnable (MARL) policies: Policy, PolicyMechanism, MarlTrainer (burn; library-only)
            └── socsim-rng         ← SimRng (ChaCha20), derive_seed
```

Dependency rules:

- `socsim-core` and `socsim-rng` have **no internal dependencies** — they are the foundation.
- `socsim-config` depends on `socsim-core` but **not** on `socsim-engine` (avoiding a cycle).
- `socsim-engine` depends on `socsim-core`, `socsim-log`, and `socsim-config`.
- `socsim-runner` depends on all of the above and adds `rayon` for parallelism.
- `socsim-cli` wires everything together into the `socsim` binary.
- `socsim-hr-lifecycle`, `socsim-net`, and `socsim-grid` sit beside the engine layer and are orthogonal to it; `socsim-grid` depends only on `socsim-core`.
- `socsim-marl` (Phase 6) depends on `socsim-engine` and `socsim-core`. It is **library-only** — not part of the `socsim` binary — and pulls in the `burn` neural-network framework, so the hr-lifecycle integration gates it behind a `marl` feature.

---

## The 6-phase tick loop

Each discrete time step executes six phases in a fixed order defined by `Phase::ORDER`:

```
PreStep → Environment → Decision → Interaction → Reward → PostStep
```

The engine's `Simulation::step` method:

1. Ticks the clock (`t += 1`).
2. Asks the `Scheduler` for the agent activation order.
3. For each phase in `Phase::ORDER`, invokes every mechanism that registered that phase, in insertion order.

The activation order computed in step 2 is passed to all phases as `StepContext::agent_order`, ensuring that mechanisms in the same step see the same ordering.

A mechanism registers its phases by returning a `'static` slice from `Mechanism::phases`. It will be called once per registered phase per step. The typical assignment of phases by the HR lifecycle mechanisms is:

| Mechanism | Phase |
|---|---|
| `learning_curve` | Environment |
| `peer_effect` | Interaction |
| `ocb` | Interaction |
| `fit` | Decision |
| `turnover` | Decision |
| `hiring` | Decision |
| `knowledge_loss` | PostStep |
| `socialization` | PostStep |
| `toxic_spread` | Interaction |
| `org_performance` | Reward |

---

## Deterministic RNG

`socsim-rng` wraps `rand_chacha::ChaCha20Rng` to provide reproducible streams. The key API:

- `SimRng::from_seed(seed: u64)` — create the root RNG.
- `SimRng::derive(&[u64])` — derive a child RNG from a label (agent ID, phase index, etc.) without mutating the parent. Uses a FNV-1a–style hash mix.

The engine seeds the root RNG from the scenario's `seed` field. The same seed always produces the same agent trajectories, regardless of machine architecture or Rust version.

Agents and team aggregates are always iterated in sorted `AgentId` order to eliminate hash-map iteration non-determinism.

---

## Snapshots: save & resume

A simulation's **mutable state** can be captured and restored — the analogue of a PyTorch `state_dict` (state) versus model architecture (code). `Snapshot<W>` holds the world (which owns the `SimClock`), the exact `SimRng` stream position (serialised via `rand_chacha`'s `serde1`), and the early-stop flag. It deliberately omits mechanisms, the scheduler, and the recorder: those are *code*, supplied when the simulation is rebuilt.

- `Simulation::snapshot()` / `restore(snapshot)` — in-memory capture/restore (`snapshot()` requires `W: Clone`).
- `Snapshot::save(path)` / `Snapshot::load(path)` — JSON persistence, version-checked via `SNAPSHOT_VERSION`.

Restoring a snapshot into a simulation wired with the **same** mechanisms reproduces the run bit-identically from the saved step onward — verified by tests that resume into a *different-seed* simulation and match an uninterrupted run. The bound is opt-in (`impl` blocks gated on `W: Serialize` / `DeserializeOwned`), so the `WorldState` trait is unchanged and non-serde worlds simply lack these methods. `SocialNetwork` serialises as a `{nodes, edges}` pair (petgraph `NodeIndex`es are rebuilt, not persisted), keeping snapshots stable across petgraph versions.

---

## Learnable policies (MARL, Phase 6)

`socsim-marl` makes the `Decision` phase learnable: a `PolicyMechanism` wraps a `Policy` (implemented by `DiscretePolicyNet`, a small `burn` MLP trained with REINFORCE) and slots into the same six-phase loop as any other mechanism — the engine needs no changes. `ObsEncoder`/`ActionApplier`/`RewardFn` bridge a concrete world to the flat feature/action space, a `TrajectoryBuffer` collects episodes, and `MarlTrainer` runs the outer learn loop. Weights are seeded from `SimRng` and all tensor math runs on CPU, so a frozen policy stays bit-reproducible. See the [library guide](library.md#learnable-policies-marl) for usage.

---

## Social network layer

`socsim-net` provides `SocialNetwork` — a thin, undirected-graph wrapper around `petgraph::UnGraph<AgentId, ()>` with an `AgentId → NodeIndex` map for O(1) lookups. Three random-graph generators are included, all accepting a `&mut SimRng`:

| Generator | Model |
|---|---|
| `SocialNetwork::erdos_renyi(ids, p, rng)` | Erdős–Rényi G(n,p) |
| `SocialNetwork::watts_strogatz(ids, k, beta, rng)` | Watts–Strogatz small-world |
| `SocialNetwork::barabasi_albert(ids, m, rng)` | Barabási–Albert preferential attachment |

The HR lifecycle baseline uses `watts_strogatz(k=4, beta=0.1)` to model a small-world inter-employee network. The `toxic_spread` and `turnover` mechanisms query neighbour lists at each step.

---

## Calibration philosophy

The HR lifecycle module separates two categories of parameters:

### Empirical correlations (ρ)

These are **fixed influence strengths** drawn directly from published meta-analyses. They represent the direction and relative magnitude of an effect as documented in the literature. Researchers should not modify them unless replacing the underlying citation.

| Constant | Value | Source |
|---|---|---|
| `RHO_SI` | 0.51 | Schmidt & Hunter (1998) — structured-interview validity |
| `ALPHA_PEER` | 0.17 | Mas & Moretti (2009) — peer-productivity multiplier |
| `P_TOXIC` | 0.04 | Housman & Minor (2015) — baseline toxic-worker prevalence |
| `P_SPREAD` | 0.46 | Housman & Minor (2015) — toxic-behaviour contagion probability |
| `PHI_TACIT` | 0.85 | Nonaka (1994) — tacit-to-total knowledge ratio |
| `RHO_PJ` | 0.20 | Kristof-Brown et al. (2005) — PJ-fit correlation |
| `RHO_PO` | 0.07 | Kristof-Brown et al. (2005) — PO-fit correlation |
| `RHO_PO_TURN` | −0.35 | Kristof-Brown et al. (2005) — PO-fit vs turnover intent |
| `LAMBDA_LEARN` | 0.15 | Bahk & Gort (1993) — learning-curve growth rate |

### Monthly-dynamics scale parameters (tunable)

These are **calibration controls** that govern the pace and magnitude of the simulation's monthly dynamics. They have no direct empirical counterpart but are tuned so the model produces plausible trajectories (e.g. ~15–22%/year voluntary turnover, a knowledge stock that grows gradually without diverging).

| Constant | Value | Governs |
|---|---|---|
| `BASE_MONTHLY_QUIT_HAZARD` | 0.008 | Baseline ~0.8%/month quit probability |
| `BASE_QUIT_LOGIT` | −4.82 | Logit intercept (`logit(0.008)`) |
| `QUIT_EMBED_SENS` | 1.0 | Sensitivity of quit logit to (1 − embeddedness) |
| `QUIT_SAT_SENS` | 0.8 | Sensitivity of quit logit to (1 − satisfaction) |
| `QUIT_CASCADE_BUMP` | 0.30 | Per-quit-neighbour additive logit bump (Krackhardt cascade) |
| `ALPHA_K` | 0.30 | OCB inflow coefficient into team knowledge stock |
| `BETA_LOSS` | 1.0 | Knowledge-loss exponent on tenure (in years) |
| `KAPPA_LOSS` | 0.40 | Knowledge-loss magnitude coefficient |
| `THETA_MEAN` | 1.0 | Mean true ability θ at hire |
| `THETA_SD` | 0.2 | Standard deviation of θ |

All calibration constants live in `crates/socsim-hr-lifecycle/src/calibration.rs` with doc-comments citing their sources.

---

## Scenario TOML schema

A scenario TOML has four sections:

```toml
[simulation]   # name, module_pack, t_max, seed, scheduler
[world]        # free-form params forwarded to the world factory
[[mechanism]]  # ordered array; one entry per mechanism to compose
[output]       # log_path template and metric keys
```

The `[[mechanism]]` array is **order-preserving**: composition order equals declaration order. Within each `Phase`, mechanisms fire in the order they appear in the scenario file.

The `output.log_path` template supports `{name}` and `{seed}` substitutions.

Two schedulers are available: `sequential` (sorted `AgentId` order, fully deterministic) and `random_activation` (shuffled each step using the simulation RNG).
