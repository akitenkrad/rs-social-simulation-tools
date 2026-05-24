//! The provider-agnostic [`LlmClient`] trait, its configuration, response and
//! metadata types, and the [`CachingClient`] decorator.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cache::{cache_key, PromptCache};

// ── errors ──────────────────────────────────────────────────────────────────

/// Errors that an [`LlmClient`] can return.
#[derive(Debug, Error)]
pub enum LlmError {
    /// The backend could not be reached or the transport failed.
    #[error("transport error talking to {endpoint}: {message}")]
    Transport {
        /// The endpoint that was being contacted.
        endpoint: String,
        /// A human-readable description.
        message: String,
    },
    /// The backend returned a non-success status or an unparseable body.
    #[error("backend error from {endpoint} (status {status}): {message}")]
    Backend {
        /// The endpoint that produced the error.
        endpoint: String,
        /// HTTP status code (0 if not applicable).
        status: u16,
        /// A human-readable description.
        message: String,
    },
    /// A required configuration value (e.g. an API key) was missing.
    #[error("configuration error: {0}")]
    Config(String),
    /// Every backend in a [`FallbackClient`](crate::FallbackClient) failed.
    #[error("all backends failed: {0}")]
    AllBackendsFailed(String),
}

// ── config ──────────────────────────────────────────────────────────────────

/// Generation configuration shared across backends.
///
/// Defaults to the **deterministic** setting (`temperature = 0`, `seed = 0`)
/// that the socsim determinism contract assumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Sampling temperature.  `0.0` requests greedy decoding.
    pub temperature: f32,
    /// Sampling seed.  Ollama honours this via `options.seed`; OpenAI applies
    /// it best-effort.
    pub seed: u64,
    /// Optional cap on generated tokens (`None` = backend default).
    pub max_tokens: Option<u32>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self::deterministic()
    }
}

impl LlmConfig {
    /// The deterministic configuration: `temperature = 0`, `seed = 0`.
    pub fn deterministic() -> Self {
        Self {
            temperature: 0.0,
            seed: 0,
            max_tokens: None,
        }
    }

    /// Set the seed (builder style).
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the temperature (builder style).
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Set the max-tokens cap (builder style).
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

// ── metadata ──────────────────────────────────────────────────────────────────

/// What a single LLM call talked to, for logging into the run's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallMetadata {
    /// Model name/version, e.g. `"llama3.1"` or `"gpt-4o-mini"`.
    pub model: String,
    /// The endpoint that served the call (or `"cache"` for a pure cache hit).
    pub endpoint: String,
    /// Temperature used.
    pub temperature: f32,
    /// Seed used.
    pub seed: u64,
    /// Whether the response was served from the cache (no backend call).
    pub cache_hit: bool,
}

/// A response together with the [`CallMetadata`] describing how it was
/// produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    /// The generated text.
    pub text: String,
    /// Provenance of this response.
    pub metadata: CallMetadata,
}

/// Accumulates [`CallMetadata`] across many calls so a run can report e.g. the
/// overall cache-hit rate.  Cheap to clone-free `push` and serialise.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataCollector {
    /// Every recorded call, in order.
    pub calls: Vec<CallMetadata>,
}

impl MetadataCollector {
    /// Create an empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one call.
    pub fn record(&mut self, meta: CallMetadata) {
        self.calls.push(meta);
    }

    /// Total number of calls recorded.
    pub fn total(&self) -> usize {
        self.calls.len()
    }

    /// Number of calls served from cache.
    pub fn cache_hits(&self) -> usize {
        self.calls.iter().filter(|c| c.cache_hit).count()
    }

    /// Cache-hit rate in `[0, 1]` (0 if no calls recorded).
    pub fn cache_hit_rate(&self) -> f64 {
        if self.calls.is_empty() {
            0.0
        } else {
            self.cache_hits() as f64 / self.calls.len() as f64
        }
    }
}

// ── the trait ──────────────────────────────────────────────────────────────

/// A provider-agnostic, **synchronous** chat-completion client.
///
/// Synchronous on purpose: the socsim engine runs a synchronous six-phase
/// loop, so a mechanism calls `complete` inline.
pub trait LlmClient {
    /// The model name/version this client targets.
    fn model(&self) -> &str;

    /// The endpoint this client talks to (for metadata).
    fn endpoint(&self) -> &str;

    /// Send `prompt` and return the completion plus its [`CallMetadata`].
    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError>;
}

// ── forwarding impls ─────────────────────────────────────────────────────────

/// Forward [`LlmClient`] through a [`Box`], so a **type-erased** client
/// (`Box<dyn LlmClient>`) is itself an [`LlmClient`].
///
/// This lets a downstream user unify several concrete client types — e.g. the
/// production `FallbackClient<…>` versus a [`mock::ScriptedClient`](crate::mock::ScriptedClient)
/// — behind one `Box<dyn LlmClient>` and still satisfy bounds like
/// `CachingClient<C: LlmClient>`, without having to define a local newtype
/// purely to work around the orphan rule.
impl<T: LlmClient + ?Sized> LlmClient for Box<T> {
    fn model(&self) -> &str {
        (**self).model()
    }

    fn endpoint(&self) -> &str {
        (**self).endpoint()
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        (**self).complete(prompt, config)
    }
}

/// Forward [`LlmClient`] through a shared reference, so `&client` is itself an
/// [`LlmClient`].  Handy for passing a borrowed client where an owned one is
/// expected without giving up ownership.
impl<T: LlmClient + ?Sized> LlmClient for &T {
    fn model(&self) -> &str {
        (**self).model()
    }

    fn endpoint(&self) -> &str {
        (**self).endpoint()
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        (**self).complete(prompt, config)
    }
}

// ── caching decorator ────────────────────────────────────────────────────────

/// Wraps any [`LlmClient`] with a prompt-keyed [`PromptCache`], pseudo-determinising
/// a non-deterministic backend: a warm cache replays identical responses.
///
/// The cache key is `hash(prompt + model)` (see [`cache_key`]).  On a hit the
/// backend is **not** contacted and the returned [`CallMetadata`] has
/// `cache_hit = true` and `endpoint = "cache"`.
pub struct CachingClient<C: LlmClient> {
    inner: C,
    cache: PromptCache,
}

impl<C: LlmClient> CachingClient<C> {
    /// Wrap `inner` with `cache`.
    pub fn new(inner: C, cache: PromptCache) -> Self {
        Self { inner, cache }
    }

    /// Borrow the underlying cache (e.g. to persist it).
    pub fn cache(&self) -> &PromptCache {
        &self.cache
    }

    /// Mutably borrow the underlying cache.
    pub fn cache_mut(&mut self) -> &mut PromptCache {
        &mut self.cache
    }

    /// Borrow the wrapped backend.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

impl<C: LlmClient> CachingClient<C> {
    /// Complete `prompt`, serving from cache when possible and recording the
    /// fresh response on a miss.  Takes `&mut self` because a miss updates the
    /// cache.
    pub fn complete(&mut self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        let key = cache_key(prompt, self.inner.model());
        if let Some(text) = self.cache.get(&key) {
            return Ok(LlmResponse {
                text,
                metadata: CallMetadata {
                    model: self.inner.model().to_string(),
                    endpoint: "cache".to_string(),
                    temperature: config.temperature,
                    seed: config.seed,
                    cache_hit: true,
                },
            });
        }
        let resp = self.inner.complete(prompt, config)?;
        self.cache.insert(key, resp.text.clone());
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::ScriptedClient;
    use crate::PromptCache;

    #[test]
    fn boxed_client_forwards() {
        let c: Box<dyn LlmClient> =
            Box::new(ScriptedClient::new("boxed-model", |p| format!("echo:{p}")));
        assert_eq!(c.model(), "boxed-model");
        assert_eq!(c.endpoint(), "mock://scripted");
        let r = c.complete("hi", &LlmConfig::deterministic()).unwrap();
        assert_eq!(r.text, "echo:hi");
        assert_eq!(r.metadata.model, "boxed-model");
        assert!(!r.metadata.cache_hit);
    }

    #[test]
    fn caching_client_accepts_boxed_client() {
        // Proves the `C: LlmClient` bound is satisfied by `Box<dyn LlmClient>`.
        let c: Box<dyn LlmClient> = Box::new(ScriptedClient::constant("boxed-model", "42"));
        let mut cached = CachingClient::new(c, PromptCache::in_memory());

        let r1 = cached.complete("q", &LlmConfig::deterministic()).unwrap();
        assert!(!r1.metadata.cache_hit); // cold miss hits the boxed backend
        assert_eq!(r1.text, "42");

        let r2 = cached.complete("q", &LlmConfig::deterministic()).unwrap();
        assert!(r2.metadata.cache_hit); // warm hit served from cache
        assert_eq!(r2.text, "42");
    }

    #[test]
    fn ref_client_forwards() {
        // Exercise the `impl LlmClient for &T` via an explicit reference binding
        // so the methods resolve through the forwarding impl (not auto-deref).
        let scripted = ScriptedClient::new("ref-model", |p| format!("r:{p}"));
        let by_ref: &dyn LlmClient = &scripted;
        assert_eq!(LlmClient::model(&by_ref), "ref-model");
        let r = LlmClient::complete(&by_ref, "hi", &LlmConfig::deterministic()).unwrap();
        assert_eq!(r.text, "r:hi");
    }
}
