use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use socsim_llm::{CallMetadata, TokenUsage};

const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;
const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static MERGE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Backend identity included in cache keys and replay manifests.
pub struct BackendIdentity {
    /// Provider-specific model name.
    pub model: String,
    /// Backend endpoint or stable endpoint label.
    pub endpoint: String,
}

/// Versioned, complete identity of one LLM request.
///
/// Declaration order is part of the cache format because serde emits struct
/// fields in this order when [`Self::canonical_json`] is called.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CacheKeyV1 {
    /// Cache-key schema version; current writers use `1`.
    pub version: u32,
    /// Model and endpoint that would serve a cache miss.
    pub backend: BackendIdentity,
    /// Optional system instruction supplied separately from the user prompt.
    pub system: Option<String>,
    /// Exact user prompt bytes interpreted as UTF-8.
    pub prompt: String,
    /// Bit-preserving representation of the request temperature.
    pub temperature_bits: u32,
    /// Requested backend seed, even when `omit_seed` prevents sending it.
    pub seed: u64,
    /// Whether the backend request omits its seed parameter.
    pub omit_seed: bool,
    /// Optional completion-token limit.
    pub max_tokens: Option<u32>,
    /// Optional backend reasoning-mode request.
    pub think: Option<bool>,
    /// Whether an empty or whitespace-only completion is accepted.
    pub allow_blank: bool,
}

impl CacheKeyV1 {
    /// Serializes the key in stable declaration order for comparison and hashing.
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).expect("CacheKeyV1 contains only JSON-serializable fields")
    }

    /// Computes the wrapping 64-bit FNV-1a index of the canonical JSON.
    ///
    /// Stores compare canonical JSON after lookup, so a digest collision is
    /// reported instead of returning the wrong record.
    pub fn digest(&self) -> u64 {
        self.canonical_json()
            .bytes()
            .fold(FNV1A_OFFSET_BASIS, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(FNV1A_PRIME)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Persisted response and metadata for one complete request identity.
pub struct ResponseRecord {
    /// Request identity used for lookup and collision verification.
    pub key: CacheKeyV1,
    /// Completion text returned to the action parser.
    pub text: String,
    /// Optional reasoning text returned separately by the backend.
    pub thinking: Option<String>,
    /// Optional token counts reported by the backend.
    pub usage: Option<TokenUsage>,
    /// Raw normalized call metadata, excluding store-level hit information.
    pub backend_metadata: Option<CallMetadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Policy for cache misses in a [`ResponseStore`].
pub enum ReplayMode {
    /// Treat every miss as an error and never contact the backend.
    ReplayOnly,
    /// Call the backend on a miss and append the response to this run's shard.
    RecordMisses,
    /// Call the backend on a miss without persisting the response.
    Live,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Counts produced by one deterministic shard merge.
pub struct MergeReport {
    /// Previously unseen records added to `base.jsonl`.
    pub records_added: usize,
    /// Idempotent records already present in the base or an earlier shard.
    pub duplicate_records: usize,
    /// Number of shard files considered, including empty shards.
    pub shards_merged: usize,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
/// Failure to read, validate, append, or merge response-store data.
pub enum StoreError {
    /// Filesystem operation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A store file is not valid UTF-8 JSON Lines for [`ResponseRecord`].
    #[error("invalid response-store JSON: {0}")]
    Json(String),
    /// One canonical request key maps to different response data.
    #[error("conflicting responses for cache key {key}")]
    Conflict {
        /// Canonical JSON of the conflicting request key.
        key: String,
    },
    /// Two distinct canonical keys produced the same 64-bit digest.
    #[error("FNV-1a cache-key collision for digest {digest:016x}")]
    KeyCollision {
        /// Colliding FNV-1a digest.
        digest: u64,
    },
}

/// Deterministic lookup table backed by a base JSONL file and per-run shard.
///
/// Disk stores load `base.jsonl` and their own shard, append new records before
/// exposing them in memory, and preserve a digest of the base for snapshot
/// replay verification. In-memory stores perform no filesystem I/O.
pub struct ResponseStore {
    store_id: String,
    base_digest: String,
    mode: ReplayMode,
    records: BTreeMap<u64, ResponseRecord>,
    shard_path: Option<PathBuf>,
}

impl ResponseStore {
    /// Creates an empty non-persistent store with the supplied stable identity.
    pub fn in_memory(store_id: &str, mode: ReplayMode) -> Self {
        Self {
            store_id: store_id.to_owned(),
            base_digest: EMPTY_SHA256.to_owned(),
            mode,
            records: BTreeMap::new(),
            shard_path: None,
        }
    }

    /// Opens a disk store and this run's `<run_tag>-<seed>.jsonl` shard.
    ///
    /// Unsafe filename characters in `run_tag` are replaced with underscores.
    /// Existing duplicate records are accepted only when their entire response
    /// data is identical.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for filesystem failures, malformed JSON Lines,
    /// conflicting records, or a digest collision.
    pub fn open(
        dir: impl AsRef<Path>,
        mode: ReplayMode,
        run_tag: &str,
        seed: u64,
    ) -> Result<Self, StoreError> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir)?;
        let shards_dir = dir.join("shards");
        fs::create_dir_all(&shards_dir)?;

        let store_id = read_or_create_store_id(dir)?;
        let base_path = dir.join("base.jsonl");
        let base_bytes = read_optional(&base_path)?;
        let base_digest = sha256_digest(&base_bytes);
        let mut records = BTreeMap::new();
        load_json_lines(&base_bytes, &base_path, &mut records)?;

        let shard_name = format!("{}-{seed}.jsonl", safe_component(run_tag));
        let shard_path = shards_dir.join(shard_name);
        let shard_bytes = read_optional(&shard_path)?;
        load_json_lines(&shard_bytes, &shard_path, &mut records)?;

        Ok(Self {
            store_id,
            base_digest,
            mode,
            records,
            shard_path: Some(shard_path),
        })
    }

    /// Returns the persistent identity shared by the store directory.
    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    /// Returns the `sha256:` digest of `base.jsonl` as it was opened.
    pub fn base_digest(&self) -> &str {
        &self.base_digest
    }

    /// Returns the configured miss policy.
    pub fn mode(&self) -> ReplayMode {
        self.mode
    }

    /// Looks up an exact canonical request, rejecting digest collisions.
    pub fn get(&self, key: &CacheKeyV1) -> Option<&ResponseRecord> {
        let record = self.records.get(&key.digest())?;
        (record.key.canonical_json() == key.canonical_json()).then_some(record)
    }

    /// Add a response to this run's shard and make it immediately queryable.
    ///
    /// Repeating the exact record is idempotent. Reusing a canonical request
    /// key for any different response data is a conflict.
    pub fn insert(&mut self, record: ResponseRecord) -> Result<(), StoreError> {
        let digest = record.key.digest();
        match classify_record(&self.records, digest, &record)? {
            RecordDisposition::Duplicate => return Ok(()),
            RecordDisposition::New => {}
        }

        if let Some(shard_path) = &self.shard_path {
            append_json_line(shard_path, &record)?;
        }
        self.records.insert(digest, record);
        Ok(())
    }

    /// Merges every sorted `shards/*.jsonl` file into `base.jsonl` atomically.
    ///
    /// Exact duplicates are counted, new records are ordered by digest, and
    /// successfully merged shards are removed. Calls in this process are
    /// serialized by a mutex.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] without replacing the base if records conflict,
    /// JSON is invalid, a digest collides, or a filesystem operation fails.
    pub fn merge_shards(dir: impl AsRef<Path>) -> Result<MergeReport, StoreError> {
        let _guard = MERGE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        merge_shards_locked(dir.as_ref())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordDisposition {
    New,
    Duplicate,
}

fn classify_record(
    records: &BTreeMap<u64, ResponseRecord>,
    digest: u64,
    incoming: &ResponseRecord,
) -> Result<RecordDisposition, StoreError> {
    let Some(existing) = records.get(&digest) else {
        return Ok(RecordDisposition::New);
    };
    let canonical = incoming.key.canonical_json();
    if existing.key.canonical_json() != canonical {
        return Err(StoreError::KeyCollision { digest });
    }
    if existing != incoming {
        return Err(StoreError::Conflict { key: canonical });
    }
    Ok(RecordDisposition::Duplicate)
}

fn merge_shards_locked(dir: &Path) -> Result<MergeReport, StoreError> {
    fs::create_dir_all(dir)?;
    let base_path = dir.join("base.jsonl");
    let base_bytes = read_optional(&base_path)?;
    let mut records = BTreeMap::new();
    load_json_lines(&base_bytes, &base_path, &mut records)?;

    let shards_dir = dir.join("shards");
    fs::create_dir_all(&shards_dir)?;
    let mut shard_paths: Vec<_> = fs::read_dir(&shards_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    shard_paths.sort();

    let mut report = MergeReport {
        shards_merged: shard_paths.len(),
        ..MergeReport::default()
    };
    for shard_path in &shard_paths {
        let shard_bytes = match fs::read(shard_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(StoreError::Io(error)),
        };
        for record in parse_json_lines(&shard_bytes, shard_path)? {
            let digest = record.key.digest();
            match classify_record(&records, digest, &record)? {
                RecordDisposition::New => {
                    records.insert(digest, record);
                    report.records_added += 1;
                }
                RecordDisposition::Duplicate => report.duplicate_records += 1,
            }
        }
    }

    if !shard_paths.is_empty() {
        let temporary_path = unique_temporary_path(dir, "base.jsonl");
        write_base(&temporary_path, records.values())?;
        if let Err(error) = fs::rename(&temporary_path, &base_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(StoreError::Io(error));
        }
        for shard_path in shard_paths {
            match fs::remove_file(shard_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }

    Ok(report)
}

fn read_optional(path: &Path) -> Result<Vec<u8>, io::Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn parse_json_lines(bytes: &[u8], path: &Path) -> Result<Vec<ResponseRecord>, StoreError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| StoreError::Json(format!("{} is not UTF-8: {error}", path.display())))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                StoreError::Json(format!("{}:{}: {error}", path.display(), index + 1))
            })
        })
        .collect()
}

fn load_json_lines(
    bytes: &[u8],
    path: &Path,
    records: &mut BTreeMap<u64, ResponseRecord>,
) -> Result<(), StoreError> {
    for record in parse_json_lines(bytes, path)? {
        let digest = record.key.digest();
        if classify_record(records, digest, &record)? == RecordDisposition::New {
            records.insert(digest, record);
        }
    }
    Ok(())
}

fn append_json_line(path: &Path, record: &ResponseRecord) -> Result<(), StoreError> {
    let mut bytes =
        serde_json::to_vec(record).map_err(|error| StoreError::Json(error.to_string()))?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

fn write_base<'a>(
    path: &Path,
    records: impl Iterator<Item = &'a ResponseRecord>,
) -> Result<(), StoreError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    for record in records {
        serde_json::to_writer(&mut file, record)
            .map_err(|error| StoreError::Json(error.to_string()))?;
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    Ok(())
}

fn read_or_create_store_id(dir: &Path) -> Result<String, StoreError> {
    let id_path = dir.join("store_id");
    match fs::read_to_string(&id_path) {
        Ok(id) => return validate_store_id(id, &id_path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::Io(error)),
    }

    let id = format!(
        "store-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let temporary_path = unique_temporary_path(dir, "store_id");
    let mut temporary = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    temporary.write_all(id.as_bytes())?;
    temporary.write_all(b"\n")?;
    temporary.sync_all()?;
    drop(temporary);

    match fs::hard_link(&temporary_path, &id_path) {
        Ok(()) => {
            fs::remove_file(&temporary_path)?;
            Ok(id)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&temporary_path)?;
            validate_store_id(fs::read_to_string(&id_path)?, &id_path)
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary_path);
            Err(StoreError::Io(error))
        }
    }
}

fn validate_store_id(id: String, path: &Path) -> Result<String, StoreError> {
    let id = id.trim().to_owned();
    if id.is_empty() {
        return Err(StoreError::Json(format!(
            "{} contains an empty store id",
            path.display()
        )));
    }
    Ok(id)
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn safe_component(run_tag: &str) -> String {
    let safe: String = run_tag
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() || safe == "." || safe == ".." {
        "run".to_owned()
    } else {
        safe
    }
}

fn unique_temporary_path(dir: &Path, stem: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let thread_id = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(
        ".{stem}.tmp-{}-{thread_id}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(prompt: &str) -> ResponseRecord {
        ResponseRecord {
            key: CacheKeyV1 {
                version: 1,
                backend: BackendIdentity {
                    model: "model".to_owned(),
                    endpoint: "endpoint".to_owned(),
                },
                system: None,
                prompt: prompt.to_owned(),
                temperature_bits: 0,
                seed: 0,
                omit_seed: false,
                max_tokens: None,
                think: None,
                allow_blank: false,
            },
            text: "answer".to_owned(),
            thinking: None,
            usage: None,
            backend_metadata: None,
        }
    }

    #[test]
    fn same_digest_with_different_canonical_key_is_a_collision() {
        let mut records = BTreeMap::new();
        records.insert(17, record("first"));

        assert!(matches!(
            classify_record(&records, 17, &record("second")),
            Err(StoreError::KeyCollision { digest: 17 })
        ));
    }

    #[test]
    fn temporary_paths_include_enough_identity_to_be_unique() {
        let dir = Path::new("temporary-test");
        let paths: Vec<_> = (0..128)
            .map(|_| unique_temporary_path(dir, "base"))
            .collect();
        let unique: std::collections::BTreeSet<_> = paths.iter().collect();

        assert_eq!(unique.len(), paths.len());
        assert!(paths.iter().all(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.contains(&std::process::id().to_string()) && name.contains("ThreadId")
        }));
    }
}
