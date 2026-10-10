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
pub struct BackendIdentity {
    pub model: String,
    pub endpoint: String,
}

/// Versioned, complete identity of one LLM request.
///
/// Declaration order is part of the cache format because serde emits struct
/// fields in this order when [`Self::canonical_json`] is called.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CacheKeyV1 {
    pub version: u32,
    pub backend: BackendIdentity,
    pub system: Option<String>,
    pub prompt: String,
    pub temperature_bits: u32,
    pub seed: u64,
    pub omit_seed: bool,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
    pub allow_blank: bool,
}

impl CacheKeyV1 {
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).expect("CacheKeyV1 contains only JSON-serializable fields")
    }

    pub fn digest(&self) -> u64 {
        self.canonical_json()
            .bytes()
            .fold(FNV1A_OFFSET_BASIS, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(FNV1A_PRIME)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseRecord {
    pub key: CacheKeyV1,
    pub text: String,
    pub thinking: Option<String>,
    pub usage: Option<TokenUsage>,
    pub backend_metadata: Option<CallMetadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayMode {
    ReplayOnly,
    RecordMisses,
    Live,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeReport {
    pub records_added: usize,
    pub duplicate_records: usize,
    pub shards_merged: usize,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("invalid response-store JSON: {0}")]
    Json(String),
    #[error("conflicting responses for cache key {key}")]
    Conflict { key: String },
    #[error("FNV-1a cache-key collision for digest {digest:016x}")]
    KeyCollision { digest: u64 },
}

pub struct ResponseStore {
    store_id: String,
    base_digest: String,
    mode: ReplayMode,
    records: BTreeMap<u64, ResponseRecord>,
    shard_path: Option<PathBuf>,
}

impl ResponseStore {
    pub fn in_memory(store_id: &str, mode: ReplayMode) -> Self {
        Self {
            store_id: store_id.to_owned(),
            base_digest: EMPTY_SHA256.to_owned(),
            mode,
            records: BTreeMap::new(),
            shard_path: None,
        }
    }

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

    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    pub fn base_digest(&self) -> &str {
        &self.base_digest
    }

    pub fn mode(&self) -> ReplayMode {
        self.mode
    }

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
