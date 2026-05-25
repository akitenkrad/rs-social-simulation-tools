//! High-level constructor for the canonical production LLM stack.
//!
//! Gated behind the `live` feature because it composes the live backends
//! ([`OllamaClient`](crate::OllamaClient) + [`OpenAiClient`](crate::OpenAiClient)).
//! It bundles the exact "Ollama-first → OpenAI-fallback → type-erased →
//! caching" composition that the socsim replications would otherwise hand-roll
//! (see e.g. chuang2024's `simulation/src/llm.rs`).

use std::path::Path;

use crate::cache::PromptCache;
use crate::client::{CachingClient, LlmClient, LlmError};
use crate::fallback::FallbackClient;
use crate::ollama::OllamaClient;
use crate::openai::OpenAiClient;

/// Build the canonical Ollama-first → OpenAI-fallback → caching client from
/// environment variables.
///
/// This is the production-ready stack the replications use, assembled in one
/// call:
///
/// ```text
/// CachingClient< Box<dyn LlmClient> >          // pseudo-determinising cache
///   └─ FallbackClient< OllamaClient, OpenAiClient >
///        ├─ primary:   OllamaClient   (OLLAMA_HOST / OLLAMA_MODEL)
///        └─ secondary: OpenAiClient   (OPENAI_API_KEY / OPENAI_MODEL)
/// ```
///
/// - **Ollama** is read via [`OllamaClient::from_env`] (`OLLAMA_HOST`,
///   default `http://localhost:11434`; `OLLAMA_MODEL`, default `llama3.1`).
/// - **OpenAI** is a *best-effort* fallback: [`OpenAiClient::from_env`] is
///   tried, and if its environment is absent (`OPENAI_API_KEY` unset) a
///   placeholder client with an empty key is constructed instead (using
///   `OPENAI_MODEL` or `gpt-4o-mini`).  The fallback is therefore always
///   present but only errors if it is actually reached *and* unconfigured —
///   i.e. only when Ollama itself failed.
/// - `cache_path = None` uses an in-memory cache via [`PromptCache::in_memory`];
///   otherwise the JSON-file cache at `cache_path` is opened via
///   [`PromptCache::open`] (created lazily; loaded if it already exists).
///
/// Construction is **lazy**: no network call is made here.  Backends are only
/// contacted when [`CachingClient::complete`] is invoked on a cache miss.
///
/// The backend is type-erased to `Box<dyn LlmClient>` so callers can keep one
/// concrete client type ([`CachingClient<Box<dyn LlmClient>>`]) whether they
/// use this production stack or inject a mock for tests.
pub fn build_live_client(
    cache_path: Option<&Path>,
) -> Result<CachingClient<Box<dyn LlmClient>>, LlmError> {
    let ollama = OllamaClient::from_env();
    // Allow running on Ollama alone: if OPENAI_API_KEY is unset, place an
    // empty-key placeholder. It is only reached when Ollama fails, and only
    // then will it surface a config error (matching the replications).
    let openai = OpenAiClient::from_env().unwrap_or_else(|_| {
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        OpenAiClient::new("", model)
    });

    let fallback = FallbackClient::new(ollama, openai);
    let backend: Box<dyn LlmClient> = Box::new(fallback);

    let cache = match cache_path {
        Some(path) => PromptCache::open(path)?,
        None => PromptCache::in_memory(),
    };
    Ok(CachingClient::new(backend, cache))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_with_in_memory_cache_without_network() {
        // Construction must be lazy — no backend is contacted here, so this
        // succeeds even with no Ollama/OpenAI reachable.
        let client = build_live_client(None).expect("in-memory construction should not fail");
        // Cache starts empty; no call has been made.
        assert!(client.cache().is_empty());
    }

    #[test]
    fn constructs_with_file_cache_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("llm_cache.json");
        // File does not exist yet: open() creates an empty in-memory view bound
        // to the path. Still no network at construction time.
        let client =
            build_live_client(Some(&path)).expect("file-cache construction should not fail");
        assert!(client.cache().is_empty());
    }
}
