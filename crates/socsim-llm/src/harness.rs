//! Reusable LLM-harness helpers shared by the socsim *replications*.
//!
//! Every LLM-driven replication (chuang2024, li2024, zhao2024, …) used to carry
//! a near-identical `simulation/src/llm.rs` that
//!
//! 1. defined an identical `LlmSettings { temperature, seed, cache_path }`,
//! 2. aliased `CachingClient<Box<dyn LlmClient>>` under a repo-specific name,
//! 3. wrapped any backend in a [`CachingClient`] (`wrap_client`),
//! 4. built an [`LlmConfig`] from the settings (`llm_config`), and
//! 5. built the live «Ollama → OpenAI + cache» client from `settings.cache_path`.
//!
//! This module hoists that boilerplate into `socsim-llm` so the replications can
//! `use socsim_llm::{LlmSettings, LiveClient, wrap_client, llm_config,
//! build_live_client_from_settings};` instead of re-declaring it.
//!
//! Everything here except [`build_live_client_from_settings`] is **feature-free**
//! (no `live`/`ollama`/`openai` needed): the helpers build on the always-available
//! [`CachingClient`] / [`PromptCache`] / [`LlmConfig`] / [`LlmError`] / [`LlmClient`]
//! types, so tests can use them with a [`mock::ScriptedClient`](crate::mock).

use crate::{CachingClient, LlmClient, LlmConfig, PromptCache};

/// LLM-layer settings for a replication run.
///
/// This is the shared definition that each replication previously declared
/// (byte-for-byte identically) in its own `config.rs`.  Field names and default
/// values are unchanged, so any TOML / config that targeted the per-repo struct
/// deserialises into this one identically.
///
/// The provider order is fixed «Ollama first → OpenAI fallback»; model / host /
/// API key come from the environment (`OLLAMA_HOST` / `OLLAMA_MODEL` /
/// `OPENAI_API_KEY` / `OPENAI_MODEL`).  `temperature` / `seed` pseudo-determinise
/// generation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmSettings {
    /// Generation temperature (default `0.0` for reproducibility).
    #[serde(default)]
    pub temperature: f32,
    /// Generation seed (passed to the backend; Ollama honours it, OpenAI is
    /// best-effort).
    #[serde(default)]
    pub seed: u64,
    /// Where to persist the prompt → response cache (`None` = in-memory).
    #[serde(default)]
    pub cache_path: Option<String>,
}

impl Default for LlmSettings {
    fn default() -> Self {
        LlmSettings {
            temperature: 0.0,
            seed: 0,
            cache_path: None,
        }
    }
}

/// The shared caching-client type used by the replications.
///
/// The backend is type-erased to `Box<dyn LlmClient>` so production
/// (`FallbackClient<OllamaClient, OpenAiClient>`) and test
/// ([`mock::ScriptedClient`](crate::mock)) backends share one alias.  The
/// `impl LlmClient for Box<T>` forwarding lets the boxed backend satisfy
/// `CachingClient`'s `C: LlmClient` bound without a newtype.
pub type LiveClient = CachingClient<Box<dyn LlmClient>>;

/// Wrap any [`LlmClient`] (e.g. a `mock::ScriptedClient`) in a cache, producing a
/// [`LiveClient`].  Used in tests and for offline runs.
pub fn wrap_client<C: LlmClient + 'static>(backend: C, cache: PromptCache) -> LiveClient {
    let boxed: Box<dyn LlmClient> = Box::new(backend);
    CachingClient::new(boxed, cache)
}

/// Build an [`LlmConfig`] from [`LlmSettings`]: the deterministic base config
/// with the settings' `temperature` and `seed` applied.
pub fn llm_config(settings: &LlmSettings) -> LlmConfig {
    LlmConfig::deterministic()
        .with_temperature(settings.temperature)
        .with_seed(settings.seed)
}

/// Build the production «Ollama first → OpenAI fallback + cache» client from
/// [`LlmSettings`], delegating to [`build_live_client`](crate::build_live_client)
/// with `settings.cache_path`.
///
/// Feature-gated behind `live` to match [`build_live_client`](crate::build_live_client).
#[cfg(feature = "live")]
pub fn build_live_client_from_settings(
    settings: &LlmSettings,
) -> Result<LiveClient, crate::LlmError> {
    crate::build_live_client(settings.cache_path.as_deref().map(std::path::Path::new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::ScriptedClient;

    #[test]
    fn llm_settings_default_matches_repos() {
        let s = LlmSettings::default();
        assert_eq!(s.temperature, 0.0);
        assert_eq!(s.seed, 0);
        assert_eq!(s.cache_path, None);
    }

    #[test]
    fn llm_settings_serde_round_trip() {
        let s = LlmSettings {
            temperature: 0.7,
            seed: 42,
            cache_path: Some("cache.json".to_string()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: LlmSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.temperature, s.temperature);
        assert_eq!(back.seed, s.seed);
        assert_eq!(back.cache_path, s.cache_path);
    }

    #[test]
    fn llm_settings_from_toml_repo_keys() {
        // The exact keys the replications use in their TOML/config.
        let toml_src = r#"
            temperature = 0.7
            seed = 42
            cache_path = "results/cache.json"
        "#;
        let s: LlmSettings = toml::from_str(toml_src).unwrap();
        assert_eq!(s.temperature, 0.7);
        assert_eq!(s.seed, 42);
        assert_eq!(s.cache_path.as_deref(), Some("results/cache.json"));
    }

    #[test]
    fn llm_settings_from_toml_uses_defaults_when_missing() {
        // `#[serde(default)]` means an empty / partial table fills in the
        // same defaults the repos' `impl Default` produces.
        let s: LlmSettings = toml::from_str("").unwrap();
        assert_eq!(s.temperature, 0.0);
        assert_eq!(s.seed, 0);
        assert_eq!(s.cache_path, None);
    }

    #[test]
    fn wrap_client_returns_working_live_client() {
        let backend = ScriptedClient::new("test-model", |_prompt| "scripted-answer".to_string());
        let mut client = wrap_client(backend, PromptCache::in_memory());

        let resp = client
            .complete("anything?", &LlmConfig::deterministic())
            .unwrap();
        assert_eq!(resp.text, "scripted-answer");
        assert!(!resp.metadata.cache_hit); // first call: miss

        // Second identical call is served from the cache.
        let resp2 = client
            .complete("anything?", &LlmConfig::deterministic())
            .unwrap();
        assert_eq!(resp2.text, "scripted-answer");
        assert!(resp2.metadata.cache_hit);
    }

    #[test]
    fn llm_config_applies_temperature_and_seed() {
        let settings = LlmSettings {
            temperature: 0.42,
            seed: 1234,
            cache_path: None,
        };
        let cfg = llm_config(&settings);
        assert_eq!(cfg.temperature, 0.42);
        assert_eq!(cfg.seed, 1234);
    }
}
