//! A minimal LLM-driven social model — the backing example for Tutorial 4.
//!
//! socsim's core is deterministic and LLM-free.  This example shows the
//! sanctioned way to put a language model *inside* a single phase of the
//! six-phase loop while keeping the run reproducible:
//!
//! - the LLM call is confined to one [`Decision`](Phase::Decision) mechanism;
//! - the mechanism talks to the model through the **`socsim-llm` harness**
//!   ([`LiveClient`] = [`CachingClient`]`<Box<dyn LlmClient>>`), so production
//!   and test wiring share one type;
//! - the runnable path uses a network-free [`ScriptedClient`] wrapped by
//!   [`wrap_client`], so it compiles and runs under **default features** (no
//!   `live`, no Ollama, no network) and prints deterministic output;
//! - generation is pinned with [`llm_config`] (temperature 0 + a fixed seed)
//!   and a [`PromptCache`], giving the two-layer determinism the contract wants
//!   (engine seed + LLM cache).
//!
//! The scenario: a tiny gossip model.  Five agents each hold a one-word belief
//! ("rumor" or "calm").  Each step, every still-"calm" agent asks the model
//! whether to start spreading the rumor, given how many of its neighbours
//! already are.  The scripted "model" adopts the rumor once a majority of
//! neighbours have — a stand-in for whatever judgement a real LLM would make
//! from the same prompt.
//!
//! Run it (default features, no network):
//!   cargo run -p socsim-llm --example tutorial_llm_agent

use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, StepContext, WorldState};
use socsim_engine::SimulationBuilder;
use socsim_llm::{
    llm_config, mock::ScriptedClient, wrap_client, LiveClient, LlmSettings, MetadataCollector,
    PromptCache,
};

// ── World: five agents on a line, each holding a one-word belief ─────────────

struct GossipWorld {
    clock: SimClock,
    /// Per-agent belief, indexed by `AgentId.0 as usize`: "rumor" or "calm".
    beliefs: Vec<String>,
}

impl GossipWorld {
    /// `n` agents in a line; only agent 0 starts knowing the rumor. `t_max` is a
    /// safety cap — the rumor reaches everyone well before it.
    fn new(n: usize, t_max: u64) -> Self {
        let beliefs = (0..n)
            .map(|i| if i == 0 { "rumor" } else { "calm" }.to_string())
            .collect();
        Self {
            clock: SimClock::new(t_max),
            beliefs,
        }
    }

    /// Line topology: agent `i`'s neighbours are `i-1` and `i+1`.
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
        let i = id.0 as usize;
        let mut nbrs = Vec::new();
        if i > 0 {
            nbrs.push(AgentId((i - 1) as u64));
        }
        if i + 1 < self.beliefs.len() {
            nbrs.push(AgentId((i + 1) as u64));
        }
        nbrs
    }

    /// How many agents currently believe the rumor — the absorbing-state probe.
    fn n_spreading(&self) -> usize {
        self.beliefs.iter().filter(|b| *b == "rumor").count()
    }
}

impl WorldState for GossipWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        (0..self.beliefs.len() as u64).map(AgentId).collect()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

// ── Mechanism: an LLM decides, for each calm agent, whether to start spreading ──
//
// The mechanism *owns* the client (a `LiveClient`) and the run-level metadata
// collector.  `apply` takes `&mut self`, so the `&mut`-on-cache-miss `complete`
// call slots straight in.  Everything LLM lives here, in `Decision`.

struct GossipDecision {
    client: LiveClient,
    settings: LlmSettings,
    collector: MetadataCollector,
}

impl Mechanism<GossipWorld> for GossipDecision {
    fn name(&self) -> &str {
        "gossip_decision"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, GossipWorld>) -> Result<()> {
        let cfg = llm_config(&self.settings);

        // Snapshot start-of-step beliefs so every agent decides from the same
        // state (a synchronous update, independent of activation order).
        let prev = ctx.world.beliefs.clone();

        let mut decisions: Vec<(AgentId, String)> = Vec::new();
        for &id in &ctx.agent_order.to_vec() {
            let i = id.0 as usize;
            if prev[i] == "rumor" {
                continue; // already spreading — nothing to decide
            }
            // Build a prompt summarising the agent's neighbourhood.
            let nbrs = ctx.world.neighbors_of(id);
            let spreading = nbrs.iter().filter(|n| prev[n.0 as usize] == "rumor").count();
            let prompt = format!(
                "You are agent {i}. Of your {} neighbours, {spreading} are spreading a rumor. \
                 Reply with exactly one word: \"rumor\" to start spreading it, or \"calm\".",
                nbrs.len()
            );

            // The one LLM call. The scripted backend answers deterministically;
            // a warm cache replays it. `complete` is &mut (a miss writes the cache).
            // `LlmError` is the LLM layer's own error type, so map it onto the
            // engine's `SocsimError` at this boundary.
            let resp = self
                .client
                .complete(&prompt, &cfg)
                .map_err(|e| socsim_core::SocsimError::Mechanism(e.to_string()))?;
            self.collector.record(resp.metadata);
            decisions.push((id, resp.text.trim().to_string()));
        }

        // Batch write-back: apply every "rumor" decision after all agents decided.
        for (id, choice) in decisions {
            if choice == "rumor" {
                ctx.world.beliefs[id.0 as usize] = "rumor".to_string();
            }
        }

        // Absorbing state: everyone is spreading ⇒ ask the engine to stop.
        if ctx.world.n_spreading() == ctx.world.beliefs.len() {
            ctx.request_stop();
        }
        Ok(())
    }
}

fn main() {
    // LLM-layer settings: temperature 0 + a fixed seed → reproducible generation.
    let settings = LlmSettings {
        temperature: 0.0,
        seed: 42,
        cache_path: None, // in-memory cache for this demo
    };

    // PRODUCTION path (real Ollama → OpenAI fallback) — gated behind `live` so the
    // default build stays network-free. Behind the cfg so default builds are clean:
    #[cfg(feature = "live")]
    let _live: socsim_llm::LiveClient =
        socsim_llm::build_live_client_from_settings(&settings).expect("live client");

    // RUNNABLE path (default features): a network-free "model" wrapped in a cache.
    // It adopts the rumor once *any* neighbour is already spreading — a
    // deterministic stand-in for the judgement a real LLM would make from the
    // same prompt. The rumor therefore cascades one hop per step along the line.
    let backend = ScriptedClient::new("gossip-mock", |prompt: &str| {
        // Parse "... neighbours, M are spreading ..." back out of the prompt.
        let m: usize = extract_after(prompt, "neighbours, ", " are spreading");
        if m >= 1 {
            "rumor".to_string()
        } else {
            "calm".to_string()
        }
    });
    let client: LiveClient = wrap_client(backend, PromptCache::in_memory());

    let world = GossipWorld::new(5, 20); // 5 agents; cap at 20 steps
    let mut sim = SimulationBuilder::new(world)
        .seed(7) // engine seed — the FIRST determinism layer
        .add_mechanism(Box::new(GossipDecision {
            client,
            settings,
            collector: MetadataCollector::new(),
        }))
        .build();

    println!("=== socsim tutorial_llm_agent (LLM-driven gossip on a line) ===");
    println!("5 agents; only agent 0 starts with the rumor.\n");
    println!("  t   beliefs                          spreading");
    println!("  ------------------------------------------------");
    sim.run_observed(|report| {
        let beliefs: Vec<&str> = report.world.beliefs.iter().map(|s| s.as_str()).collect();
        println!(
            "  {:>2}   {:<32}  {}",
            report.t,
            format!("{beliefs:?}"),
            report.world.n_spreading(),
        );
    })
    .expect("simulation completed");

    println!(
        "\nThe rumor reached every agent at t = {}.",
        sim.world().clock().t()
    );
}

/// Tiny helper: pull the integer that sits between `start` and `end` in `s`.
fn extract_after(s: &str, start: &str, end: &str) -> usize {
    let from = s.find(start).map(|i| i + start.len()).unwrap_or(0);
    let rest = &s[from..];
    let to = rest.find(end).unwrap_or(rest.len());
    rest[..to].trim().parse().unwrap_or(0)
}
