//! The provider-agnostic [`LlmClient`] trait, its configuration, response and
//! metadata types, and the [`CachingClient`] decorator.

use std::cell::RefCell;

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
    /// The backend returned a successful but **empty** (or whitespace-only)
    /// response text.  Reasoning/harmony models (e.g. gpt-oss) can spend the
    /// whole `max_tokens` budget on a hidden thinking trace and emit no visible
    /// answer; surfacing this as an error lets callers retry or raise the budget
    /// instead of silently propagating a blank completion.
    #[error("empty response from {endpoint} (model {model})")]
    EmptyResponse {
        /// The endpoint that produced the empty response.
        endpoint: String,
        /// The model that produced the empty response.
        model: String,
    },
    /// Every backend in a [`FallbackClient`](crate::FallbackClient) failed.
    #[error("all backends failed: {0}")]
    AllBackendsFailed(String),
    /// The requested operation is not implemented by this backend.
    ///
    /// Returned by the default [`LlmClient::complete_with_logprobs`] impl: a
    /// client that does not expose token log-probabilities reports it here
    /// rather than silently degrading to a logprob-free completion.
    #[error("operation not supported by {endpoint}: {operation}")]
    Unsupported {
        /// The endpoint that does not support the operation.
        endpoint: String,
        /// A short name for the unsupported operation (e.g. `"logprobs"`).
        operation: String,
    },
}

/// Reject a blank completion: if `text` is empty or whitespace-only, return
/// [`LlmError::EmptyResponse`] carrying the `endpoint` / `model` context;
/// otherwise pass `text` through unchanged.
///
/// Live backends call this after extracting the response text so a model that
/// spent its whole token budget on a hidden reasoning trace surfaces as an
/// error rather than a silent empty string.  Factored out (and unit-tested
/// below) so the check is exercised without a live server.
///
/// Only compiled when a live backend that uses it is enabled, so a
/// network-free default build does not carry (or warn about) an unused helper.
#[cfg(any(feature = "ollama", feature = "openai"))]
pub(crate) fn reject_blank_response(
    text: String,
    endpoint: &str,
    model: &str,
) -> Result<String, LlmError> {
    if text.trim().is_empty() {
        return Err(LlmError::EmptyResponse {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
        });
    }
    Ok(text)
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
    /// Optional system prompt.  When `Some`, live chat backends prepend a
    /// `{role: system, content}` message before the user message; `None`
    /// (the default) sends only the user message, preserving current behaviour.
    #[serde(default)]
    pub system: Option<String>,
    /// When `true`, live backends do **not** send a `seed`, enabling
    /// temperature-driven sampling without a pinned seed.  Defaults to `false`
    /// (the current behaviour: the seed is always emitted).
    #[serde(default)]
    pub omit_seed: bool,
    /// When `true`, an empty/whitespace-only completion is returned as an
    /// `Ok` response with empty `text` instead of erroring.  Defaults to
    /// `false` (the current behaviour: a blank response is rejected).
    #[serde(default)]
    pub allow_blank: bool,
    /// Number of top alternatives to request when calling
    /// [`complete_with_logprobs`](LlmClient::complete_with_logprobs).  `None`
    /// (the default) uses the backend's sane default (20).  Ignored by the
    /// plain [`complete`](LlmClient::complete) path.
    #[serde(default)]
    pub top_logprobs: Option<u32>,
}

/// Default `top_logprobs` requested when logprobs are asked for but no explicit
/// count is set (mirrors argyle2023's `DEFAULT_TOP_LOGPROBS`).
///
/// Only used by the live backends, so gated to avoid a dead-code warning in a
/// network-free default build.
#[cfg(any(feature = "ollama", feature = "openai"))]
pub(crate) const DEFAULT_TOP_LOGPROBS: u32 = 20;

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
            system: None,
            omit_seed: false,
            allow_blank: false,
            top_logprobs: None,
        }
    }

    /// A **sampling** configuration: the deterministic base with `temperature`
    /// applied and the seed omitted (`omit_seed = true`).
    ///
    /// This is the RSS-style setting used to observe a model's sampling
    /// distribution: a non-zero temperature without a pinned seed.  All other
    /// fields keep their deterministic defaults; chain the builders below to
    /// add e.g. a system prompt or blank tolerance.
    pub fn sampling(temperature: f32) -> Self {
        Self::deterministic()
            .with_temperature(temperature)
            .omit_seed()
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

    /// Set the system prompt (builder style).
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Stop sending a `seed` to the backend (builder style), enabling
    /// temperature sampling without a pinned seed.
    pub fn omit_seed(mut self) -> Self {
        self.omit_seed = true;
        self
    }

    /// Tolerate an empty completion (builder style): a blank response is
    /// returned as `Ok` with empty `text` rather than erroring.
    pub fn allow_blank(mut self) -> Self {
        self.allow_blank = true;
        self
    }

    /// Set how many top alternatives to request from
    /// [`complete_with_logprobs`](LlmClient::complete_with_logprobs)
    /// (builder style).
    pub fn with_top_logprobs(mut self, n: u32) -> Self {
        self.top_logprobs = Some(n);
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

/// A single candidate token with its natural-log probability.
///
/// Mirrors the shape Ollama / OpenAI return for a generation position: the
/// surface `token` string (which may include a leading space, e.g. `" Donald"`),
/// its raw `bytes` (kept so whitespace / multi-byte tokens are judged robustly),
/// and the natural-log `logprob`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenLogprob {
    /// The token's surface string (may contain a leading space).
    pub token: String,
    /// The token's raw bytes.
    pub bytes: Vec<u8>,
    /// The natural-log probability of this token.
    pub logprob: f64,
}

/// A response together with the [`CallMetadata`] describing how it was
/// produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    /// The generated text.
    pub text: String,
    /// Provenance of this response.
    pub metadata: CallMetadata,
    /// Top-K token log-probabilities for the first generated position, when
    /// requested via [`complete_with_logprobs`](LlmClient::complete_with_logprobs).
    /// `None` for the plain [`complete`](LlmClient::complete) path (the default),
    /// preserving the current response shape for existing callers.
    #[serde(default)]
    pub logprobs: Option<Vec<TokenLogprob>>,
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

    /// Summarise the collected calls into a serialisable [`RunMetadata`].
    ///
    /// The identity fields (`llm_model` / `llm_endpoint` / `llm_temperature` /
    /// `llm_seed`) are taken from the **most recent non-cache** call when one
    /// exists — that is the backend that actually answered, not the synthetic
    /// `"cache"` endpoint a cache hit records — falling back to the most recent
    /// call of any kind, then to defaults.  The counts and hit-rate come from
    /// the collector itself.  An empty collector yields empty strings, zero
    /// counts and a `0.0` hit-rate without panicking.
    pub fn summary(&self) -> RunMetadata {
        // Prefer the last non-cache call (the real backend), else the last call
        // of any kind, so the recorded model/endpoint reflect a live provider
        // rather than the `"cache"` placeholder when at least one miss happened.
        let identity = self
            .calls
            .iter()
            .rev()
            .find(|c| !c.cache_hit)
            .or_else(|| self.calls.last());

        match identity {
            Some(meta) => RunMetadata {
                llm_model: meta.model.clone(),
                llm_endpoint: meta.endpoint.clone(),
                llm_temperature: meta.temperature,
                llm_seed: meta.seed,
                total_calls: self.total(),
                cache_hits: self.cache_hits(),
                cache_hit_rate: self.cache_hit_rate(),
            },
            None => RunMetadata::default(),
        }
    }
}

/// A serialisable, run-level summary of an entire run's LLM activity.
///
/// Produced by [`MetadataCollector::summary`].  This is the uniform shape the
/// downstream replications persist (e.g. as `llm_meta.json`): the model /
/// endpoint / generation settings the run talked to, plus the call and
/// cache-hit counts.  Replication-specific prose (e.g. a determinism note)
/// is intentionally **not** part of this struct.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunMetadata {
    /// Model name/version the run talked to (from the latest backend call).
    pub llm_model: String,
    /// Endpoint that served the run's backend calls.
    pub llm_endpoint: String,
    /// Temperature used (matches [`CallMetadata::temperature`]).
    pub llm_temperature: f32,
    /// Seed used (matches [`CallMetadata::seed`]).
    pub llm_seed: u64,
    /// Total number of [`complete`](LlmClient::complete) calls recorded.
    pub total_calls: usize,
    /// Number of those calls served from cache.
    pub cache_hits: usize,
    /// Cache-hit rate in `[0, 1]` (`0.0` if no calls were recorded).
    pub cache_hit_rate: f64,
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

    /// Send `prompt` and return the completion **with** the first generated
    /// position's top-K token log-probabilities in
    /// [`LlmResponse::logprobs`].
    ///
    /// The default implementation returns [`LlmError::Unsupported`]: a backend
    /// that cannot expose log-probabilities (e.g. the in-memory test clients,
    /// or a cloud model that does not return them) reports it rather than
    /// silently producing a logprob-free response.  Backends that support it
    /// (e.g. [`OllamaClient`](crate::OllamaClient),
    /// [`OpenAiClient`](crate::OpenAiClient)) override this.
    ///
    /// The number of alternatives requested is `config.top_logprobs`, falling
    /// back to a sane default (20) when unset.
    fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        let _ = (prompt, config);
        Err(LlmError::Unsupported {
            endpoint: self.endpoint().to_string(),
            operation: "logprobs".to_string(),
        })
    }
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

    fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        (**self).complete_with_logprobs(prompt, config)
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

    fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        (**self).complete_with_logprobs(prompt, config)
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
                logprobs: None,
            });
        }
        let resp = self.inner.complete(prompt, config)?;
        self.cache.insert(key, resp.text.clone());
        Ok(resp)
    }

    /// Request a completion **with** token log-probabilities, delegating to the
    /// wrapped backend.
    ///
    /// Unlike [`complete`](Self::complete) this does **not** consult or populate
    /// the cache: the [`PromptCache`] only stores response *text*, so it cannot
    /// faithfully replay a logprob distribution.  Logprob callers
    /// (e.g. argyle2023) drive their own per-prompt caching when needed.
    pub fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        self.inner.complete_with_logprobs(prompt, config)
    }
}

// ── interior-mutable caching decorator ───────────────────────────────────────

/// An interior-mutable sibling of [`CachingClient`] that **implements
/// [`LlmClient`]**, so a cache-backed client can be passed through a
/// `&dyn LlmClient` / `Box<dyn LlmClient>` injection point.
///
/// [`CachingClient::complete`] takes `&mut self` because a miss updates the
/// cache, which means it cannot satisfy `LlmClient::complete(&self, …)`.  This
/// wrapper moves the cache behind a [`RefCell`] so the `&self` trait method can
/// still record fresh responses on a miss.  The caching semantics are otherwise
/// **identical** to [`CachingClient`]: the same `hash(prompt + model)` key (see
/// [`cache_key`]), the same hit/miss behaviour, and a hit returns
/// [`CallMetadata`] with `cache_hit = true` and `endpoint = "cache"`.
///
/// # Single-threaded by design
///
/// [`LlmClient`] is a **synchronous, single-threaded** trait (the socsim engine
/// drives a synchronous six-phase loop and calls `complete` inline), and the
/// trait carries no `Send`/`Sync` bound.  A [`RefCell`] therefore suffices and
/// avoids a [`Mutex`](std::sync::Mutex)'s locking cost and poisoning; the
/// borrows taken in [`complete`](Self::complete) are short and never overlap, so
/// no runtime borrow conflict can occur.
///
/// Use [`CachingClient`] when an owned `&mut self` decorator is enough; reach for
/// `SharedCachingClient` only when the cache must live behind a shared
/// `&dyn LlmClient`.
pub struct SharedCachingClient<C: LlmClient> {
    inner: C,
    cache: RefCell<PromptCache>,
}

impl<C: LlmClient> SharedCachingClient<C> {
    /// Wrap `inner` with `cache`.
    pub fn new(inner: C, cache: PromptCache) -> Self {
        Self {
            inner,
            cache: RefCell::new(cache),
        }
    }

    /// Borrow the wrapped backend.
    pub fn inner(&self) -> &C {
        &self.inner
    }

    /// Borrow the interior-mutable cache cell (e.g. to persist it via
    /// [`PromptCache::save`]).
    pub fn cache(&self) -> &RefCell<PromptCache> {
        &self.cache
    }

    /// Consume the wrapper, returning the inner client and the cache.
    ///
    /// Useful to recover the [`PromptCache`] after a run (e.g. to save it) once
    /// the shared client is no longer needed.
    pub fn into_parts(self) -> (C, PromptCache) {
        (self.inner, self.cache.into_inner())
    }
}

impl<C: LlmClient> LlmClient for SharedCachingClient<C> {
    fn model(&self) -> &str {
        self.inner.model()
    }

    fn endpoint(&self) -> &str {
        self.inner.endpoint()
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        let key = cache_key(prompt, self.inner.model());
        // Scope the immutable borrow so it is released before the backend call.
        if let Some(text) = self.cache.borrow().get(&key) {
            return Ok(LlmResponse {
                text,
                metadata: CallMetadata {
                    model: self.inner.model().to_string(),
                    endpoint: "cache".to_string(),
                    temperature: config.temperature,
                    seed: config.seed,
                    cache_hit: true,
                },
                logprobs: None,
            });
        }
        let resp = self.inner.complete(prompt, config)?;
        self.cache.borrow_mut().insert(key, resp.text.clone());
        Ok(resp)
    }

    /// Request a completion **with** token log-probabilities, delegating to the
    /// wrapped backend **without** caching.
    ///
    /// Mirrors [`CachingClient::complete_with_logprobs`]: the [`PromptCache`]
    /// only stores response *text*, so it cannot faithfully replay a logprob
    /// distribution; logprob callers drive their own per-prompt caching when
    /// needed.
    fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        self.inner.complete_with_logprobs(prompt, config)
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
    fn summary_of_populated_collector() {
        let mut collector = MetadataCollector::new();
        // Two backend misses then one cache hit: 1/3 hit rate, identity from a
        // real backend call (not the "cache" placeholder).
        collector.record(CallMetadata {
            model: "llama3.1".into(),
            endpoint: "http://localhost:11434/api/chat".into(),
            temperature: 0.0,
            seed: 42,
            cache_hit: false,
        });
        collector.record(CallMetadata {
            model: "llama3.1".into(),
            endpoint: "http://localhost:11434/api/chat".into(),
            temperature: 0.0,
            seed: 42,
            cache_hit: false,
        });
        collector.record(CallMetadata {
            model: "llama3.1".into(),
            endpoint: "cache".into(),
            temperature: 0.0,
            seed: 42,
            cache_hit: true,
        });

        let s = collector.summary();
        assert_eq!(s.total_calls, 3);
        assert_eq!(s.cache_hits, 1);
        assert!((s.cache_hit_rate - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(s.llm_model, "llama3.1");
        // Identity prefers the real backend over the "cache" placeholder.
        assert_eq!(s.llm_endpoint, "http://localhost:11434/api/chat");
        assert_eq!(s.llm_temperature, 0.0);
        assert_eq!(s.llm_seed, 42);
    }

    #[test]
    fn summary_falls_back_to_cache_identity_when_all_hits() {
        // If every recorded call is a cache hit, identity comes from the last
        // call (there is no non-cache call to prefer).
        let mut collector = MetadataCollector::new();
        collector.record(CallMetadata {
            model: "gpt-4o-mini".into(),
            endpoint: "cache".into(),
            temperature: 0.0,
            seed: 7,
            cache_hit: true,
        });
        let s = collector.summary();
        assert_eq!(s.total_calls, 1);
        assert_eq!(s.cache_hits, 1);
        assert_eq!(s.cache_hit_rate, 1.0);
        assert_eq!(s.llm_model, "gpt-4o-mini");
        assert_eq!(s.llm_endpoint, "cache");
        assert_eq!(s.llm_seed, 7);
    }

    #[test]
    fn summary_of_empty_collector_is_defaults() {
        let s = MetadataCollector::new().summary();
        assert_eq!(s.llm_model, "");
        assert_eq!(s.llm_endpoint, "");
        assert_eq!(s.llm_temperature, 0.0);
        assert_eq!(s.llm_seed, 0);
        assert_eq!(s.total_calls, 0);
        assert_eq!(s.cache_hits, 0);
        assert_eq!(s.cache_hit_rate, 0.0);
    }

    #[cfg(any(feature = "ollama", feature = "openai"))]
    #[test]
    fn reject_blank_response_flags_empty_and_whitespace() {
        // Empty string → error carrying endpoint/model context.
        let err = reject_blank_response(String::new(), "http://h/api/chat", "gpt-oss").unwrap_err();
        match err {
            LlmError::EmptyResponse { endpoint, model } => {
                assert_eq!(endpoint, "http://h/api/chat");
                assert_eq!(model, "gpt-oss");
            }
            other => panic!("expected EmptyResponse, got {other:?}"),
        }
        // Whitespace-only (incl. newlines/tabs) → error.
        assert!(matches!(
            reject_blank_response("  \n\t ".to_string(), "ep", "m"),
            Err(LlmError::EmptyResponse { .. })
        ));
        // Non-blank text passes through unchanged (not trimmed).
        assert_eq!(
            reject_blank_response("  hello  ".to_string(), "ep", "m").unwrap(),
            "  hello  "
        );
    }

    #[test]
    fn default_complete_with_logprobs_is_unsupported() {
        // A backend that does not override the method reports Unsupported,
        // carrying its endpoint — it does not silently degrade.
        let c = ScriptedClient::constant("m", "x");
        let err = c
            .complete_with_logprobs("hi", &LlmConfig::deterministic())
            .unwrap_err();
        match err {
            LlmError::Unsupported {
                endpoint,
                operation,
            } => {
                assert_eq!(endpoint, "mock://scripted");
                assert_eq!(operation, "logprobs");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn deterministic_defaults_are_unchanged() {
        // Backward-compat: the new fields default to current behaviour and the
        // existing fields keep their values.
        let c = LlmConfig::deterministic();
        assert_eq!(c.temperature, 0.0);
        assert_eq!(c.seed, 0);
        assert_eq!(c.max_tokens, None);
        assert_eq!(c.system, None);
        assert!(!c.omit_seed);
        assert!(!c.allow_blank);
        assert_eq!(c.top_logprobs, None);
    }

    #[test]
    fn sampling_omits_seed_and_sets_temperature() {
        let c = LlmConfig::sampling(1.0);
        assert_eq!(c.temperature, 1.0);
        assert!(c.omit_seed);
        // seed value untouched; backends just don't send it.
        assert_eq!(c.seed, 0);
    }

    #[test]
    fn config_builders_compose() {
        let c = LlmConfig::deterministic()
            .with_system("sys")
            .allow_blank()
            .with_top_logprobs(5);
        assert_eq!(c.system.as_deref(), Some("sys"));
        assert!(c.allow_blank);
        assert_eq!(c.top_logprobs, Some(5));
    }

    #[test]
    fn old_llm_response_json_deserializes_without_logprobs() {
        // A serialized response from before the `logprobs` field must still
        // deserialize (serde default → None).
        let json = r#"{"text":"hi","metadata":{"model":"m","endpoint":"e",
            "temperature":0.0,"seed":0,"cache_hit":false}}"#;
        let r: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.text, "hi");
        assert_eq!(r.logprobs, None);
    }

    #[test]
    fn old_llm_config_json_deserializes_with_field_defaults() {
        // A serialized config from before the new fields must still deserialize.
        let json = r#"{"temperature":0.0,"seed":0,"max_tokens":null}"#;
        let c: LlmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c, LlmConfig::deterministic());
    }

    #[test]
    fn shared_caching_client_hit_miss_through_self() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // Count backend calls so a cache hit is observable as "inner not called".
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in = Arc::clone(&calls);
        let backend = ScriptedClient::new("shared-model", move |p| {
            calls_in.fetch_add(1, Ordering::SeqCst);
            format!("echo:{p}")
        });
        let client = SharedCachingClient::new(backend, PromptCache::in_memory());
        let cfg = LlmConfig::deterministic();

        // Cold miss: inner is called once.
        let r1 = client.complete("q", &cfg).unwrap();
        assert_eq!(r1.text, "echo:q");
        assert!(!r1.metadata.cache_hit);
        assert_eq!(r1.metadata.endpoint, "mock://scripted");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Warm hit: same prompt is served from cache, inner NOT called again.
        let r2 = client.complete("q", &cfg).unwrap();
        assert_eq!(r2.text, "echo:q");
        assert!(r2.metadata.cache_hit);
        assert_eq!(r2.metadata.endpoint, "cache");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Different prompt misses and calls the backend again.
        let r3 = client.complete("other", &cfg).unwrap();
        assert_eq!(r3.text, "echo:other");
        assert!(!r3.metadata.cache_hit);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shared_caching_client_works_as_dyn_llm_client() {
        // Store behind &dyn / Box<dyn> LlmClient and call through the trait.
        let inner = ScriptedClient::constant("dyn-model", "42");
        let client = SharedCachingClient::new(inner, PromptCache::in_memory());
        let boxed: Box<dyn LlmClient> = Box::new(client);
        let by_dyn: &dyn LlmClient = &*boxed;

        assert_eq!(by_dyn.model(), "dyn-model");
        let r1 = by_dyn.complete("q", &LlmConfig::deterministic()).unwrap();
        assert!(!r1.metadata.cache_hit);
        assert_eq!(r1.text, "42");
        let r2 = by_dyn.complete("q", &LlmConfig::deterministic()).unwrap();
        assert!(r2.metadata.cache_hit); // cache survives behind &dyn
        assert_eq!(r2.text, "42");
    }

    #[test]
    fn shared_caching_client_delegates_logprobs_without_caching() {
        // A backend that produces logprobs; the wrapper returns them verbatim
        // and does not consult/populate the text cache.
        struct LogprobBackend;
        impl LlmClient for LogprobBackend {
            fn model(&self) -> &str {
                "lp-model"
            }
            fn endpoint(&self) -> &str {
                "mock://logprob"
            }
            fn complete(&self, _p: &str, cfg: &LlmConfig) -> Result<LlmResponse, LlmError> {
                Ok(LlmResponse {
                    text: "plain".into(),
                    metadata: CallMetadata {
                        model: "lp-model".into(),
                        endpoint: "mock://logprob".into(),
                        temperature: cfg.temperature,
                        seed: cfg.seed,
                        cache_hit: false,
                    },
                    logprobs: None,
                })
            }
            fn complete_with_logprobs(
                &self,
                _p: &str,
                cfg: &LlmConfig,
            ) -> Result<LlmResponse, LlmError> {
                Ok(LlmResponse {
                    text: "Donald".into(),
                    metadata: CallMetadata {
                        model: "lp-model".into(),
                        endpoint: "mock://logprob".into(),
                        temperature: cfg.temperature,
                        seed: cfg.seed,
                        cache_hit: false,
                    },
                    logprobs: Some(vec![TokenLogprob {
                        token: " Donald".into(),
                        bytes: b" Donald".to_vec(),
                        logprob: -0.1,
                    }]),
                })
            }
        }

        let client = SharedCachingClient::new(LogprobBackend, PromptCache::in_memory());
        let cfg = LlmConfig::deterministic();
        let r = client.complete_with_logprobs("q", &cfg).unwrap();
        assert_eq!(r.text, "Donald");
        let lp = r.logprobs.expect("logprobs delegated through");
        assert_eq!(lp.len(), 1);
        assert_eq!(lp[0].token, " Donald");
        // The logprob path must not have populated the text cache: a plain
        // `complete` for the same prompt is still a miss.
        let plain = client.complete("q", &cfg).unwrap();
        assert!(!plain.metadata.cache_hit);
        assert_eq!(plain.text, "plain");
    }

    #[test]
    fn shared_caching_client_uses_same_key_scheme_as_caching_client() {
        // Parity: a hit is keyed on cache_key(prompt, model) exactly like
        // CachingClient — pre-seeding the cache under that key yields a hit.
        let mut cache = PromptCache::in_memory();
        cache.insert(cache_key("q", "parity-model"), "seeded".into());
        let client =
            SharedCachingClient::new(ScriptedClient::constant("parity-model", "fresh"), cache);
        let r = client.complete("q", &LlmConfig::deterministic()).unwrap();
        // Served from the seeded entry, not the backend's "fresh".
        assert!(r.metadata.cache_hit);
        assert_eq!(r.text, "seeded");
    }

    #[test]
    fn shared_caching_client_into_parts_recovers_cache() {
        let client =
            SharedCachingClient::new(ScriptedClient::constant("m", "v"), PromptCache::in_memory());
        client.complete("q", &LlmConfig::deterministic()).unwrap();
        let (inner, cache) = client.into_parts();
        assert_eq!(inner.model(), "m");
        assert_eq!(cache.get_for("q", "m"), Some("v".into()));
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
