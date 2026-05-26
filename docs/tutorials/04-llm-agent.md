**English** | [日本語](04-llm-agent.ja.md)

# T4 — An LLM-driven agent

**What you'll build:** a tiny gossip model whose agents decide via a language model — kept fully deterministic and runnable with **no network** by using the `socsim-llm` harness and a scripted mock.
**Estimated time:** 45 minutes.

## Prerequisites

- [T1 — Your first model](01-first-model.md) (`WorldState`, `Mechanism`, `run_observed`, seeds).
- **Optional**, only for the live path: a running [Ollama](https://ollama.com) (or an OpenAI key). The runnable tutorial needs neither.

Backing example, CI-compiled under **default features**: [`crates/socsim-llm/examples/tutorial_llm_agent.rs`](../../crates/socsim-llm/examples/tutorial_llm_agent.rs). Open it alongside this page.

## The determinism contract

socsim's core is deterministic: same seed → same trajectory, bit-for-bit. An LLM is inherently *non*-deterministic, so socsim confines it to one phase and pseudo-determinises it with **two layers**:

1. the **engine seed** (the usual `SimRng`), and
2. an **LLM layer**: `temperature = 0` + a fixed generation seed + a **prompt → response cache** that replays identical responses on a warm cache.

`socsim-llm` packages this so you never hand-roll client wiring. See the [Library API LLM section](../library.md#llm-agents-and-result-output-in-library-mode).

## Steps

### 1. The model

Five agents on a line each hold a one-word belief (`"rumor"` or `"calm"`); only agent 0 starts with the rumor. Each step, every still-calm agent asks the model whether to start spreading, given how many neighbours already are.

```rust
struct GossipWorld {
    clock: SimClock,
    /// Per-agent belief, indexed by `AgentId.0 as usize`: "rumor" or "calm".
    beliefs: Vec<String>,
}
```

### 2. Confine the LLM call to a `Decision` mechanism

`LlmClient::complete` is synchronous, so it drops straight into `apply`. The mechanism *owns* the client and a `MetadataCollector` (which records what every call talked to). All the LLM lives here, in `Phase::Decision`:

```rust
struct GossipDecision {
    client: LiveClient,
    settings: LlmSettings,
    collector: MetadataCollector,
}

impl Mechanism<GossipWorld> for GossipDecision {
    fn name(&self) -> &str { "gossip_decision" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Decision] }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, GossipWorld>) -> Result<()> {
        let cfg = llm_config(&self.settings);
        let prev = ctx.world.beliefs.clone();   // snapshot: synchronous update
        // ... build a prompt per calm agent, call the model, collect decisions ...
    }
}
```

`llm_config(&settings)` builds an `LlmConfig` that is `LlmConfig::deterministic()` (temperature 0, seed 0) with the settings' `temperature` and `seed` applied — the first half of the LLM determinism layer.

### 3. The one LLM call

For each still-calm agent, build a prompt, call the model once, and record the metadata. Note the error map: the LLM layer has its own `LlmError`, so you convert it onto the engine's `SocsimError` at this boundary:

```rust
let resp = self
    .client
    .complete(&prompt, &cfg)
    .map_err(|e| socsim_core::SocsimError::Mechanism(e.to_string()))?;
self.collector.record(resp.metadata);
decisions.push((id, resp.text.trim().to_string()));
```

`self.client` is a `LiveClient` = `CachingClient<Box<dyn LlmClient>>`. `complete` takes `&mut self` because a cache **miss** writes the new response into the cache; a **hit** replays it without touching the backend — that is the second half of the determinism layer.

### 4. Wire the runnable (network-free) client with `wrap_client` + `ScriptedClient`

This is the crux. The same `LiveClient` type is produced two ways — production and test — so your mechanism code never changes. The runnable path uses a `ScriptedClient` (a deterministic in-memory "model" defined by a closure) wrapped in a cache by `wrap_client`:

```rust
let backend = ScriptedClient::new("gossip-mock", |prompt: &str| {
    // The scripted "model": adopt the rumor once *any* neighbour is spreading.
    let m: usize = extract_after(prompt, "neighbours, ", " are spreading");
    if m >= 1 { "rumor".to_string() } else { "calm".to_string() }
});
let client: LiveClient = wrap_client(backend, PromptCache::in_memory());
```

The **live** path is shown but gated behind the `live` feature so the default build pulls in no networking at all:

```rust
#[cfg(feature = "live")]
let _live: socsim_llm::LiveClient =
    socsim_llm::build_live_client_from_settings(&settings).expect("live client");
```

Flip to a real model later by enabling `--features live` and using `_live` as the client — the rest of the model is identical. The settings carry the two LLM-layer knobs plus the cache location:

```rust
let settings = LlmSettings {
    temperature: 0.0,
    seed: 42,
    cache_path: None, // in-memory cache for this demo
};
```

### 5. Build and run as usual

The LLM mechanism is just a mechanism, so the assembly is ordinary library mode (note the **engine** seed — the first determinism layer — distinct from the LLM seed in `settings`):

```rust
let mut sim = SimulationBuilder::new(world)
    .seed(7) // engine seed — the FIRST determinism layer
    .add_mechanism(Box::new(GossipDecision { client, settings, collector: MetadataCollector::new() }))
    .build();

sim.run_observed(|report| { /* print beliefs + spreading count */ })
   .expect("simulation completed");
```

## Run it

Default features — no Ollama, no network, deterministic:

```sh
cargo run -p socsim-llm --example tutorial_llm_agent
```

```
=== socsim tutorial_llm_agent (LLM-driven gossip on a line) ===
5 agents; only agent 0 starts with the rumor.

  t   beliefs                          spreading
  ------------------------------------------------
   1   ["rumor", "rumor", "calm", "calm", "calm"]  2
   2   ["rumor", "rumor", "rumor", "calm", "calm"]  3
   3   ["rumor", "rumor", "rumor", "rumor", "calm"]  4
   4   ["rumor", "rumor", "rumor", "rumor", "rumor"]  5

The rumor reached every agent at t = 4.
```

The rumor cascades one hop per step and the model stops itself at consensus. Run it again — identical output, because the engine seed + the scripted model + the warm cache make it deterministic. That same determinism is what lets you **unit-test an LLM model**: swap the closure for your test oracle and assert on the trajectory.

To confirm the live path still compiles (it won't call out unless you run it):

```sh
cargo build -p socsim-llm --all-features --example tutorial_llm_agent
```

## What you learned

- Confine the LLM to **one `Decision` mechanism**; `complete` is synchronous and slots into `apply`.
- The `socsim-llm` **harness** gives one `LiveClient` type for both production (`build_live_client_from_settings`, behind `live`) and tests (`wrap_client` + `mock::ScriptedClient`) — your model code is identical either way.
- **Two-layer determinism**: the engine seed *plus* an LLM layer (`temperature = 0`, fixed seed, and a `PromptCache` that replays responses).
- `MetadataCollector` records what every call talked to (model / endpoint / cache-hit) for the run's provenance.
- Map `LlmError` onto `SocsimError` at the mechanism boundary.

See the [Library API LLM section](../library.md#llm-agents-and-result-output-in-library-mode) for `build_live_client`, `RunMetadata`, and persisting an `llm_meta.json` sidecar.

## Next

[T5 — A scenario pack](05-scenario-pack.md): package mechanisms into a `ModulePack` and drive them from scenario TOML and the CLI.
