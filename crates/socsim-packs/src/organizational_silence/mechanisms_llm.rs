//! Feature-gated LLM voice-decision mechanism for the organizational-silence
//! pack.
//!
//! Compiled only when the crate feature `organizational-silence-llm` is
//! enabled.  Provides [`VoiceDecisionLlmMechanism`], registered under the
//! canonical name `voice_decision` (the rule-based equivalent keeps
//! `voice_decision_rule`).  See §5.2 of the design doc.
//!
//! Determinism contract: the per-call config uses `temperature = 0` and the
//! scenario seed; a warm prompt cache turns the LLM into a reproducible
//! oracle (see `socsim_llm::harness`).

use serde::Deserialize;

use socsim_config::Params;
use socsim_core::{AgentId, Mechanism, Phase, Result, SocsimError, StepContext};
#[cfg(test)]
use socsim_llm::{wrap_client, PromptCache};
use socsim_llm::{
    build_live_client_from_settings, llm_config, LiveClient, LlmConfig, LlmSettings,
    MetadataCollector,
};

use crate::organizational_silence::world::{Expression, Motive, SilenceWorld};

/// JSON shape we expect the LLM to return.
///
/// `decision` is required; `motive` is required only when `decision ==
/// "SILENCE"`.  `rationale` is logged but otherwise unused.
#[derive(Debug, Deserialize)]
struct LlmVoiceDecision {
    decision: String,
    #[serde(default)]
    motive: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    rationale: Option<String>,
}

/// LLM-driven voice-vs-silence decision mechanism.
///
/// In the `Decision` phase, for each agent in `ctx.agent_order` we build a
/// structured persona+context prompt, call [`LiveClient::complete`] under the
/// deterministic config, and parse the response JSON into a (decision,
/// motive) pair.  On parse failure we fall back to a permissive default
/// (Silence + Defensive) and continue rather than aborting the run.
pub struct VoiceDecisionLlmMechanism {
    client: LiveClient,
    collector: MetadataCollector,
    cfg: LlmConfig,
}

impl VoiceDecisionLlmMechanism {
    /// Construct from `[mechanism.params]`.
    ///
    /// Recognised keys:
    /// - `cache_path` (string, default `"runs/silence_cache.json"`).
    /// - `seed` (u64, default 42).
    /// - `temperature` (f64, default 0.0).
    ///
    /// On environments without a reachable live backend this builds the live
    /// client lazily — if `build_live_client_from_settings` fails (e.g. no
    /// Ollama/OpenAI configured), the constructor returns a wrapped error.
    /// Tests should use [`Self::from_client`] to inject a `ScriptedClient`.
    pub fn from_params(p: &Params) -> Result<Self> {
        let settings = LlmSettings {
            temperature: p.get_f64("temperature", 0.0) as f32,
            seed: p.get_u64("seed", 42),
            cache_path: Some(
                p.get_str("cache_path", "runs/silence_cache.json").to_owned(),
            ),
        };
        let client = build_live_client_from_settings(&settings).map_err(|e| {
            SocsimError::Mechanism(format!("failed to build live LLM client: {e}"))
        })?;
        Ok(Self {
            client,
            collector: MetadataCollector::new(),
            cfg: llm_config(&settings),
        })
    }

    /// Construct from an already-wired [`LiveClient`].  Intended for tests:
    /// build a [`socsim_llm::mock::ScriptedClient`], wrap it with
    /// [`wrap_client`], and pass it in.
    pub fn from_client(client: LiveClient, cfg: LlmConfig) -> Self {
        Self {
            client,
            collector: MetadataCollector::new(),
            cfg,
        }
    }

    /// Shared in-memory test constructor: wraps a scripted closure and an
    /// in-memory cache around a deterministic [`LlmConfig`].
    #[cfg(test)]
    pub fn from_scripted<F: Fn(&str) -> String + Send + Sync + 'static>(
        model: &str,
        answer: F,
    ) -> Self {
        let scripted = socsim_llm::mock::ScriptedClient::new(model.to_string(), answer);
        let client = wrap_client(scripted, PromptCache::in_memory());
        Self::from_client(client, LlmConfig::deterministic())
    }

    /// Cache-hit-rate summary across all calls this mechanism has made.
    pub fn metadata(&self) -> &MetadataCollector {
        &self.collector
    }

    /// Build the user-visible prompt for one agent.  Pure function so tests
    /// can inspect the rendered text.
    fn build_prompt(world: &SilenceWorld, id: AgentId) -> String {
        let emp = match world.employees.get(&id) {
            Some(e) => e,
            None => return String::new(),
        };
        let u_k = world
            .teams
            .get(emp.team)
            .map(|t| t.supervisor_openness)
            .unwrap_or(0.0);
        let retaliated = world.retaliation_this_step.contains(&id);
        format!(
            "You are an employee in an organisation.\n\
             Persona: level={lvl}, tenure_months={ten}, fear={fear:.2}, \
             psych_safety={psafe:.2}, ivt_strength={ivt:.2}.\n\
             Context: neighbour_silence_ratio={rho:.2}, supervisor_openness={uk:.2}, \
             retaliated_this_step={ret}, issue_salience={sigma:.2}.\n\
             Decide whether to VOICE a concern or stay SILENT.  If SILENT, \
             pick a motive from {{acquiescent, defensive, prosocial}}.\n\
             Reply with exactly one JSON object on a single line: \
             {{\"decision\": \"VOICE\"|\"SILENCE\", \
             \"motive\": \"acquiescent\"|\"defensive\"|\"prosocial\"|null, \
             \"rationale\": \"...\"}}.",
            lvl = emp.level,
            ten = emp.tenure,
            fear = emp.fear,
            psafe = emp.psych_safety,
            ivt = emp.ivt_strength,
            rho = emp.neighbor_silence_ratio,
            uk = u_k,
            ret = if retaliated { "yes" } else { "no" },
            sigma = world.issue_salience,
        )
    }

    /// Parse an LLM JSON response into (expression, motive).  Tolerant of
    /// uppercase / lowercase variants; falls back to (`Silence`,
    /// `Some(Defensive)`) on any parse failure so a single misbehaving call
    /// never aborts the run.
    fn parse_response(text: &str) -> (Expression, Option<Motive>) {
        match serde_json::from_str::<LlmVoiceDecision>(text) {
            Ok(d) => {
                let decision = d.decision.to_ascii_uppercase();
                if decision == "VOICE" {
                    (Expression::Voice, None)
                } else {
                    let motive = d
                        .motive
                        .as_deref()
                        .map(|s| s.to_ascii_lowercase())
                        .and_then(|s| match s.as_str() {
                            "acquiescent" => Some(Motive::Acquiescent),
                            "defensive" => Some(Motive::Defensive),
                            "prosocial" => Some(Motive::Prosocial),
                            _ => None,
                        })
                        .or(Some(Motive::Defensive));
                    (Expression::Silence, motive)
                }
            }
            Err(_) => (Expression::Silence, Some(Motive::Defensive)),
        }
    }
}

impl Mechanism<SilenceWorld> for VoiceDecisionLlmMechanism {
    fn name(&self) -> &str {
        "voice_decision"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        let order: Vec<AgentId> = ctx.agent_order.to_vec();
        for id in order {
            if !ctx.world.employees.contains_key(&id) {
                continue;
            }
            let prompt = Self::build_prompt(ctx.world, id);
            let resp = self.client.complete(&prompt, &self.cfg).map_err(|e| {
                SocsimError::Mechanism(format!("LLM call failed: {e}"))
            })?;
            self.collector.record(resp.metadata.clone());
            let (expression, motive) = Self::parse_response(&resp.text);
            if let Some(emp) = ctx.world.employees.get_mut(&id) {
                emp.expression = expression;
                emp.silence_motive = if expression == Expression::Voice {
                    None
                } else {
                    motive
                };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use socsim_core::{Recorder, SimRng};
    use socsim_engine::{SequentialScheduler, SimulationBuilder};
    use socsim_log::InMemoryRecorder;
    use std::sync::{Arc, Mutex};

    use super::*;

    struct SharedRecorder(Arc<Mutex<InMemoryRecorder>>);
    impl SharedRecorder {
        fn new() -> (Self, Arc<Mutex<InMemoryRecorder>>) {
            let inner = Arc::new(Mutex::new(InMemoryRecorder::new()));
            (Self(Arc::clone(&inner)), inner)
        }
    }
    impl Recorder for SharedRecorder {
        fn record_metric(&mut self, t: u64, key: &str, value: f64) {
            self.0.lock().unwrap().record_metric(t, key, value);
        }
        fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().record_event(t, kind, payload);
        }
    }

    #[test]
    fn parse_response_handles_well_formed_voice() {
        let (e, m) = VoiceDecisionLlmMechanism::parse_response(
            r#"{"decision": "VOICE", "motive": null, "rationale": "ok"}"#,
        );
        assert_eq!(e, Expression::Voice);
        assert!(m.is_none());
    }

    #[test]
    fn parse_response_handles_silence_defensive() {
        let (e, m) = VoiceDecisionLlmMechanism::parse_response(
            r#"{"decision": "SILENCE", "motive": "defensive", "rationale": "scared"}"#,
        );
        assert_eq!(e, Expression::Silence);
        assert_eq!(m, Some(Motive::Defensive));
    }

    #[test]
    fn parse_response_falls_back_on_garbage() {
        let (e, m) = VoiceDecisionLlmMechanism::parse_response("not json");
        assert_eq!(e, Expression::Silence);
        assert_eq!(m, Some(Motive::Defensive));
    }

    #[test]
    fn llm_voice_decision_handles_scripted_responses() {
        // Scripted client that always answers "VOICE" / null motive.
        let mut mech = VoiceDecisionLlmMechanism::from_scripted("test-model", |_| {
            r#"{"decision": "VOICE", "motive": null, "rationale": "ok"}"#.to_string()
        });

        // Build a tiny world and run the mechanism by stepping through one
        // Decision phase via the engine.
        let mut rng = SimRng::from_seed(7);
        let world = SilenceWorld::new(1, 4, 1, 2, 0.0, 1.0, &mut rng);

        // Pre-step state: every agent starts Neutral.
        assert!(world
            .employees
            .values()
            .all(|e| e.expression == Expression::Neutral));

        let (rec, _handle) = SharedRecorder::new();
        let mut builder = SimulationBuilder::new(world)
            .scheduler(Box::new(SequentialScheduler))
            .seed(0)
            .recorder(Box::new(rec));
        // We need a stable ordering and a phase to run.  Build the mech and
        // step the simulation.
        // Reuse the box impl by transferring the mech into the builder.
        builder = builder.add_mechanism(Box::new(std::mem::replace(
            &mut mech,
            VoiceDecisionLlmMechanism::from_scripted("placeholder", |_| String::new()),
        )));
        let mut sim = builder.build();
        sim.step().unwrap();
        // After the scripted Decision phase every employee should be VOICE.
        assert!(sim
            .world()
            .employees
            .values()
            .all(|e| e.expression == Expression::Voice));
    }
}
