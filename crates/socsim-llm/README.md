# socsim-llm

Optional LLM helper layer for the [socsim](../../README.md) social-simulation platform.

## The determinism contract

socsim's core (`socsim-core`, `socsim-engine`, …) is **deterministic and LLM-free**: given a seed, a run reproduces bit-for-bit. Many social simulations want an LLM inside a single phase (usually `Decision` / `Interaction`), which is inherently non-deterministic. This crate confines that non-determinism to one place and *pseudo-determinises* it:

- **`temperature = 0`** and a fixed **`seed`** are plumbed through `LlmConfig` (Ollama honours `options.seed`; OpenAI is best-effort).
- Optional **`think`** control is sent only to Ollama as a top-level request field: `None` leaves the field absent and preserves the model's default, while `Some(false)` / `Some(true)` explicitly disables / enables thinking. OpenAI ignores it.
- A **prompt → response cache** keyed on `hash(prompt + model)` makes a re-run with a warm cache replay identical responses — turning a noisy model into a reproducible oracle.
- **`CallMetadata`** records model / endpoint / temperature / seed / cache-hit and optional backend-reported token usage for every call, and `MetadataCollector` aggregates the cache-hit rate for the run's output.
- **`LlmResponse`** carries the generated text plus an optional non-blank thinking trace surfaced by Ollama responses.

The crate is **optional and feature-gated**: it is *not* a dependency of `socsim-core` / `socsim-engine`, and the default build pulls in no networking at all.

## Features

| Feature | Pulls in | Enables |
|---|---|---|
| (default) | nothing | `LlmClient` trait, `PromptCache`, `CachingClient`, metadata, in-memory mock clients |
| `ollama` | `ureq` | `OllamaClient` (`/api/chat`, `OLLAMA_HOST` / `OLLAMA_MODEL`) |
| `openai` | `ureq` | `OpenAiClient` (`/v1/chat/completions`, `OPENAI_API_KEY` / `OPENAI_MODEL`) |
| `live`   | both | `FallbackClient` over the two live backends |

The provider backends use a **synchronous** HTTP client (`ureq`) because the socsim engine runs a synchronous six-phase loop.

## Usage

```rust
use socsim_llm::{LlmClient, LlmConfig, CachingClient, PromptCache, mock::ScriptedClient};

// Offline / tests: a scripted "model" — no network.
let backend = ScriptedClient::constant("test-model", "42");
let mut client = CachingClient::new(backend, PromptCache::in_memory());

let r1 = client.complete("the answer?", &LlmConfig::deterministic()).unwrap();
assert!(!r1.metadata.cache_hit);  // miss
let r2 = client.complete("the answer?", &LlmConfig::deterministic()).unwrap();
assert!(r2.metadata.cache_hit);   // hit — identical response
```

The canonical live configuration is "Ollama first, OpenAI fallback":

```rust,ignore
use socsim_llm::{FallbackClient, OllamaClient, OpenAiClient, CachingClient, PromptCache};

let fallback = FallbackClient::new(OllamaClient::from_env(), OpenAiClient::from_env()?);
let mut client = CachingClient::new(fallback, PromptCache::open("llm-cache.json")?);
```

## Tests

All unit/integration tests are **network-free** (cache hit/miss, fallback selection, metadata). Live-network tests are marked `#[ignore]` and require a running Ollama or a real OpenAI key.

```bash
cargo test -p socsim-llm --all-features
```
