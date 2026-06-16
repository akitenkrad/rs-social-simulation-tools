//! Optional LLM helper layer for the `socsim` platform.
//!
//! # The determinism contract
//!
//! socsim's core (`socsim-core`, `socsim-engine`, …) is **deterministic and
//! LLM-free**: given a seed, a run reproduces bit-for-bit.  Many social
//! simulations, however, want an LLM inside a single phase (usually
//! `Decision` / `Interaction`).  An LLM is inherently non-deterministic, so
//! this crate confines that non-determinism to one place and *pseudo-determinises*
//! it:
//!
//! - **`temperature = 0`** and a fixed **`seed`** are plumbed through
//!   [`LlmConfig`] (Ollama honours `options.seed`; OpenAI is best-effort).
//! - A **prompt → response cache** keyed on `hash(prompt + model)` means a
//!   re-run with a warm cache replays identical responses — turning a noisy
//!   model into a reproducible oracle.
//! - **[`CallMetadata`]** records model / endpoint / temperature / seed /
//!   cache-hit for every call so a run can log exactly what it talked to.
//!
//! This crate is **optional and feature-gated**: it is *not* a dependency of
//! `socsim-core` / `socsim-engine`, and the default build pulls in no
//! networking at all.  The live provider backends are behind cargo features:
//!
//! | Feature | Pulls in | Enables |
//! |---|---|---|
//! | (default) | nothing | [`LlmClient`] trait, [`PromptCache`], [`CachingClient`], metadata, in-memory test clients |
//! | `ollama` | `ureq` | [`OllamaClient`] (`/api/chat`) |
//! | `openai` | `ureq` | [`OpenAiClient`] (`/v1/chat/completions`) |
//! | `live`   | both | [`FallbackClient`] over the two live backends |
//!
//! # Example (network-free)
//!
//! ```rust
//! use socsim_llm::{LlmClient, LlmConfig, CachingClient, PromptCache, mock::ScriptedClient};
//!
//! // A scripted "model" that always answers "42" — no network.
//! let backend = ScriptedClient::new("test-model", |_prompt| "42".to_string());
//! let mut client = CachingClient::new(backend, PromptCache::in_memory());
//!
//! let r1 = client.complete("the answer?", &LlmConfig::deterministic()).unwrap();
//! assert!(!r1.metadata.cache_hit);          // first call: miss
//! let r2 = client.complete("the answer?", &LlmConfig::deterministic()).unwrap();
//! assert!(r2.metadata.cache_hit);           // second call: served from cache
//! assert_eq!(r1.text, r2.text);
//! ```

mod cache;
mod client;
mod fallback;
mod harness;
pub mod mock;
pub mod parse;

#[cfg(feature = "live")]
mod live;
#[cfg(feature = "ollama")]
mod ollama;
#[cfg(feature = "openai")]
mod openai;

pub use cache::{cache_key, PromptCache};
pub use client::{
    CachingClient, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse, MetadataCollector,
    RunMetadata, SharedCachingClient, TokenLogprob,
};
pub use fallback::FallbackClient;
pub use harness::{
    llm_config, wrap_client, wrap_client_shared, LiveClient, LlmSettings, SharedLiveClient,
};
pub use parse::extract_first_choice;

#[cfg(feature = "live")]
pub use harness::{build_live_client_from_settings, build_shared_live_client_from_settings};
#[cfg(feature = "live")]
pub use live::{build_live_client, build_shared_live_client};
#[cfg(feature = "ollama")]
pub use ollama::OllamaClient;
#[cfg(feature = "openai")]
pub use openai::OpenAiClient;
