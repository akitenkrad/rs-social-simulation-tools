//! Prompt → response cache keyed on `hash(prompt + model)`.
//!
//! The cache is the heart of the determinism contract: a warm cache makes a
//! non-deterministic backend replay identical responses.  It can live purely
//! in memory or be backed by a JSON file (load on construction, save on
//! demand).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::LlmError;

/// Compute the cache key for a `(prompt, model)` pair.
///
/// Uses a 64-bit [FNV-1a](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function)
/// mix over `model` then a separator then `prompt`, rendered as lowercase hex.
/// Distinct `(prompt, model)` pairs map to distinct keys; the same pair always
/// maps to the same key, so re-runs hit the cache.
pub fn cache_key(prompt: &str, model: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut h = FNV_OFFSET;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
    };
    mix(model.as_bytes());
    mix(b"\x00"); // separator so model="ab",prompt="c" != model="a",prompt="bc"
    mix(prompt.as_bytes());
    format!("{h:016x}")
}

/// On-disk representation of the cache: a flat `key → response` map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CacheData {
    entries: HashMap<String, String>,
}

/// A prompt-keyed response cache.
///
/// Construct one with [`PromptCache::in_memory`] (no persistence) or
/// [`PromptCache::open`] (load a JSON file, creating an empty cache if it does
/// not yet exist).  Call [`PromptCache::save`] to persist.
#[derive(Debug, Clone, Default)]
pub struct PromptCache {
    data: CacheData,
    path: Option<PathBuf>,
}

impl PromptCache {
    /// An empty, non-persisted cache.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Open (or initialise) a JSON-file-backed cache at `path`.
    ///
    /// If the file exists it is loaded; if it does not, an empty cache bound to
    /// that path is returned (nothing is written until [`save`](Self::save)).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LlmError> {
        let path = path.as_ref().to_path_buf();
        let data = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| LlmError::Config(format!("reading cache {}: {e}", path.display())))?;
            serde_json::from_str(&text)
                .map_err(|e| LlmError::Config(format!("parsing cache {}: {e}", path.display())))?
        } else {
            CacheData::default()
        };
        Ok(Self {
            data,
            path: Some(path),
        })
    }

    /// Look up a cached response by key.
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.entries.get(key).cloned()
    }

    /// Insert (or overwrite) a cached response.
    pub fn insert(&mut self, key: String, response: String) {
        self.data.entries.insert(key, response);
    }

    /// Convenience: look up by `(prompt, model)`.
    pub fn get_for(&self, prompt: &str, model: &str) -> Option<String> {
        self.get(&cache_key(prompt, model))
    }

    /// Convenience: insert by `(prompt, model)`.
    pub fn insert_for(&mut self, prompt: &str, model: &str, response: String) {
        self.insert(cache_key(prompt, model), response);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.data.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.data.entries.is_empty()
    }

    /// Persist the cache to its file.
    ///
    /// Returns an error if the cache was created with [`in_memory`](Self::in_memory)
    /// (no path).  Writes atomically (to a temp file, then rename).
    pub fn save(&self) -> Result<(), LlmError> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| LlmError::Config("in-memory cache has no path to save to".into()))?;
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|e| LlmError::Config(format!("serialising cache: {e}")))?;
        // Atomic write: temp file in the same dir, then rename over.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .map_err(|e| LlmError::Config(format!("writing cache {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| LlmError::Config(format!("renaming cache {}: {e}", path.display())))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_distinct() {
        assert_eq!(cache_key("p", "m"), cache_key("p", "m"));
        assert_ne!(cache_key("p", "m"), cache_key("p", "n"));
        assert_ne!(cache_key("p", "m"), cache_key("q", "m"));
        // The separator prevents prompt/model boundary collisions.
        assert_ne!(cache_key("bc", "a"), cache_key("c", "ab"));
    }

    #[test]
    fn in_memory_get_set() {
        let mut c = PromptCache::in_memory();
        assert!(c.get_for("hi", "m").is_none());
        c.insert_for("hi", "m", "yo".into());
        assert_eq!(c.get_for("hi", "m"), Some("yo".into()));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");

        let mut c = PromptCache::open(&path).unwrap();
        assert!(c.is_empty());
        c.insert_for("hi", "m", "yo".into());
        c.save().unwrap();

        let reloaded = PromptCache::open(&path).unwrap();
        assert_eq!(reloaded.get_for("hi", "m"), Some("yo".into()));
    }

    #[test]
    fn save_in_memory_errors() {
        let c = PromptCache::in_memory();
        assert!(c.save().is_err());
    }
}
