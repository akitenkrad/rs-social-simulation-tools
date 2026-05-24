//! End-to-end (network-free) test of the determinism contract: a caching
//! client over a scripted backend, with metadata collected across calls.

use socsim_llm::mock::ScriptedClient;
use socsim_llm::{CachingClient, LlmConfig, MetadataCollector, PromptCache};

#[test]
fn cache_hit_miss_and_metadata_rate() {
    let backend = ScriptedClient::new("test-model", |p| format!("reply-to:{p}"));
    let mut client = CachingClient::new(backend, PromptCache::in_memory());
    let cfg = LlmConfig::deterministic();
    let mut collector = MetadataCollector::new();

    // First call to each of two prompts: misses.
    let a1 = client.complete("alpha", &cfg).unwrap();
    collector.record(a1.metadata.clone());
    let b1 = client.complete("beta", &cfg).unwrap();
    collector.record(b1.metadata.clone());

    // Repeat: hits, identical text.
    let a2 = client.complete("alpha", &cfg).unwrap();
    collector.record(a2.metadata.clone());
    let b2 = client.complete("beta", &cfg).unwrap();
    collector.record(b2.metadata.clone());

    assert_eq!(a1.text, "reply-to:alpha");
    assert_eq!(a1.text, a2.text);
    assert!(!a1.metadata.cache_hit);
    assert!(a2.metadata.cache_hit);
    assert_eq!(a2.metadata.endpoint, "cache");

    assert_eq!(collector.total(), 4);
    assert_eq!(collector.cache_hits(), 2);
    assert!((collector.cache_hit_rate() - 0.5).abs() < 1e-12);
}

#[test]
fn warm_file_cache_replays_without_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("llm-cache.json");
    let cfg = LlmConfig::deterministic();

    // Run 1: cold cache, backend answers, persist.
    {
        let backend = ScriptedClient::constant("m", "run1-answer");
        let mut client = CachingClient::new(backend, PromptCache::open(&path).unwrap());
        let r = client.complete("q", &cfg).unwrap();
        assert!(!r.metadata.cache_hit);
        client.cache().save().unwrap();
    }

    // Run 2: warm cache + a *different* backend reply — the cached value wins,
    // proving the backend was not consulted (pseudo-determinism across runs).
    {
        let backend = ScriptedClient::constant("m", "run2-different");
        let mut client = CachingClient::new(backend, PromptCache::open(&path).unwrap());
        let r = client.complete("q", &cfg).unwrap();
        assert!(r.metadata.cache_hit);
        assert_eq!(r.text, "run1-answer");
    }
}
