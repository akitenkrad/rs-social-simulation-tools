use std::{fs, sync::Arc, thread};

use socsim_games::llm::{
    BackendIdentity, CacheKeyV1, ReplayMode, ResponseRecord, ResponseStore, StoreError,
};
use socsim_llm::{CallMetadata, TokenUsage};
use tempfile::tempdir;

fn key(seed: u64) -> CacheKeyV1 {
    CacheKeyV1 {
        version: 1,
        backend: BackendIdentity {
            model: "scripted".to_owned(),
            endpoint: "memory://scripted".to_owned(),
        },
        system: Some("rules".to_owned()),
        prompt: format!("turn-{seed}"),
        temperature_bits: 0.0_f32.to_bits(),
        seed,
        omit_seed: false,
        max_tokens: Some(64),
        think: Some(true),
        allow_blank: false,
    }
}

fn record(seed: u64, text: &str) -> ResponseRecord {
    ResponseRecord {
        key: key(seed),
        text: text.to_owned(),
        thinking: Some(format!("thinking-{seed}")),
        usage: Some(TokenUsage {
            prompt_tokens: Some(seed + 10),
            completion_tokens: Some(seed + 20),
        }),
        backend_metadata: Some(CallMetadata {
            model: "scripted".to_owned(),
            endpoint: "memory://scripted".to_owned(),
            temperature: 0.0,
            seed,
            cache_hit: false,
            usage: Some(TokenUsage {
                prompt_tokens: Some(seed + 10),
                completion_tokens: Some(seed + 20),
            }),
        }),
    }
}

#[test]
fn memory_hit_restores_thinking_usage_and_metadata() {
    let mut store = ResponseStore::in_memory("test-store", ReplayMode::ReplayOnly);
    let expected = record(1, "answer");

    store.insert(expected.clone()).unwrap();

    assert_eq!(store.get(&expected.key), Some(&expected));
    assert_eq!(store.store_id(), "test-store");
    assert_eq!(store.mode(), ReplayMode::ReplayOnly);
}

#[test]
fn open_reuses_store_id_hashes_empty_base_and_never_changes_base_on_insert() {
    let dir = tempdir().unwrap();
    let mut first = ResponseStore::open(dir.path(), ReplayMode::RecordMisses, "run", 3).unwrap();
    let store_id = first.store_id().to_owned();
    let empty_sha256 = concat!(
        "sha256:e3b0c44298fc1c149afbf4c8996fb924",
        "27ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(first.base_digest(), empty_sha256);

    let base_before = fs::read(dir.path().join("base.jsonl")).unwrap_or_default();
    let expected = record(3, "answer");
    first.insert(expected.clone()).unwrap();
    assert_eq!(
        fs::read(dir.path().join("base.jsonl")).unwrap_or_default(),
        base_before
    );

    let reopened = ResponseStore::open(dir.path(), ReplayMode::ReplayOnly, "run", 3).unwrap();
    assert_eq!(reopened.store_id(), store_id);
    assert_eq!(reopened.get(&expected.key), Some(&expected));
}

#[test]
fn parallel_runs_keep_distinct_shards_and_merge_without_lost_updates() {
    const RUNS: u64 = 8;
    let dir = Arc::new(tempdir().unwrap());
    let handles: Vec<_> = (0..RUNS)
        .map(|seed| {
            let dir = Arc::clone(&dir);
            thread::spawn(move || {
                let mut store =
                    ResponseStore::open(dir.path(), ReplayMode::RecordMisses, "sweep", seed)
                        .unwrap();
                store
                    .insert(record(seed, &format!("answer-{seed}")))
                    .unwrap();
                store.store_id().to_owned()
            })
        })
        .collect();

    let store_ids: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(store_ids.windows(2).all(|ids| ids[0] == ids[1]));
    let shard_count = fs::read_dir(dir.path().join("shards"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .count();
    assert_eq!(shard_count, RUNS as usize);

    let report = ResponseStore::merge_shards(dir.path()).unwrap();
    assert_eq!(report.records_added, RUNS as usize);
    let merged = ResponseStore::open(dir.path(), ReplayMode::ReplayOnly, "verify", 999).unwrap();
    for seed in 0..RUNS {
        assert_eq!(
            merged.get(&key(seed)).map(|entry| entry.text.as_str()),
            Some(format!("answer-{seed}").as_str())
        );
    }
}

#[test]
fn insert_is_idempotent_but_rejects_any_changed_response_field() {
    let mut store = ResponseStore::in_memory("test-store", ReplayMode::RecordMisses);
    let original = record(2, "same");
    store.insert(original.clone()).unwrap();
    store.insert(original.clone()).unwrap();

    for changed in [
        ResponseRecord {
            text: "different".to_owned(),
            ..original.clone()
        },
        ResponseRecord {
            thinking: Some("different".to_owned()),
            ..original.clone()
        },
        ResponseRecord {
            usage: None,
            ..original.clone()
        },
        ResponseRecord {
            backend_metadata: None,
            ..original.clone()
        },
    ] {
        assert!(matches!(
            store.insert(changed),
            Err(StoreError::Conflict { .. })
        ));
    }
}

#[test]
fn merge_rejects_same_key_with_different_record() {
    let dir = tempdir().unwrap();
    let mut first = ResponseStore::open(dir.path(), ReplayMode::RecordMisses, "run", 1).unwrap();
    let mut second = ResponseStore::open(dir.path(), ReplayMode::RecordMisses, "run", 2).unwrap();
    first.insert(record(7, "first")).unwrap();
    second.insert(record(7, "second")).unwrap();

    assert!(matches!(
        ResponseStore::merge_shards(dir.path()),
        Err(StoreError::Conflict { .. })
    ));
}

#[test]
fn malformed_base_is_reported_as_json_error() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("base.jsonl"), b"not-json\n").unwrap();

    assert!(matches!(
        ResponseStore::open(dir.path(), ReplayMode::ReplayOnly, "run", 0),
        Err(StoreError::Json(_))
    ));
}

#[test]
fn simultaneous_merges_use_independent_temporary_files() {
    let dir = Arc::new(tempdir().unwrap());
    for seed in 0..4 {
        let mut store =
            ResponseStore::open(dir.path(), ReplayMode::RecordMisses, "merge", seed).unwrap();
        store.insert(record(seed, "answer")).unwrap();
    }

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let dir = Arc::clone(&dir);
            thread::spawn(move || ResponseStore::merge_shards(dir.path()))
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }

    let temporary_files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(temporary_files.is_empty());
}
