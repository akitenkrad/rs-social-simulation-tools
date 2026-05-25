//! Output helpers for the socsim **lightweight library mode**.
//!
//! The `socsim` runner/config/log family (`Recorder` etc.) is intentionally
//! unused by the lightweight replications: each replication ships its own
//! `main.rs` + clap CLI and writes outputs directly. Those replications all
//! hand-roll the same boilerplate — a timestamped results directory, a
//! `latest` symlink that re-points at the newest run, and small serde CSV/JSON
//! writers. This crate factors that boilerplate out so a replication can opt
//! in with a single git-dependency line.
//!
//! It is a **leaf crate**: it depends only on `std`, `serde`, `serde_json`,
//! `csv`, and `chrono`. It deliberately does **not** depend on any other
//! `socsim-*` crate, so pulling it in never drags in `socsim-config`,
//! `socsim-runner`, or `socsim-log`.
//!
//! # Domain-agnostic by design
//!
//! This crate provides only generic serialization primitives. Domain types
//! (e.g. the LLM `RunMetadata` produced by `socsim-llm`) live in their owning
//! crates; callers serialize them here via [`write_json`].
//!
//! # Example
//!
//! ```no_run
//! use serde::Serialize;
//! use socsim_results::{create_run_dir, refresh_latest_symlink, timestamp, write_csv, write_json};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(Serialize)]
//! struct MetricRow { step: usize, value: f64 }
//!
//! let ts = timestamp();
//! let run_dir = create_run_dir("results")?;
//! write_csv(&[MetricRow { step: 0, value: 1.0 }], run_dir.join("metrics.csv"))?;
//! write_json(&serde_json::json!({ "seed": 42 }), run_dir.join("config.json"))?;
//! refresh_latest_symlink("results", &ts)?;
//! # Ok(())
//! # }
//! ```

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::Serialize;

/// Current local time as a `"YYYYMMDD_HHMMSS"` stamp.
///
/// This is the exact convention used across the lightweight replications
/// (`Local::now().format("%Y%m%d_%H%M%S")`), suitable for use as a run
/// subdirectory name and as the `target` passed to
/// [`refresh_latest_symlink`].
pub fn timestamp() -> String {
    Local::now().format("%Y%m%d_%H%M%S").to_string()
}

/// Ensure that `path` exists as a directory, creating it (and any missing
/// parents) if necessary.
///
/// Idempotent: succeeds if the directory already exists.
pub fn ensure_dir(path: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Create a timestamped run directory `base/<timestamp>` (creating `base` and
/// any missing parents along the way) and return its [`PathBuf`].
///
/// The timestamp is produced by [`timestamp`]. To re-point a `latest` symlink
/// at the new run afterwards, pass the same stamp to
/// [`refresh_latest_symlink`]; the returned path's final component is that
/// stamp.
pub fn create_run_dir(base: impl AsRef<Path>) -> io::Result<PathBuf> {
    let dir = base.as_ref().join(timestamp());
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// (Re)point `base/latest` at `target`.
///
/// `target` is the symlink's contents — typically a sibling run-directory name
/// (e.g. the stamp returned by [`timestamp`]) so the link resolves relative to
/// `base`. Any existing `base/latest` symlink is removed first; a missing link
/// (or `base` itself not yet existing) is treated as success.
///
/// # Platform behaviour
///
/// On Unix this creates a symbolic link via
/// [`std::os::unix::fs::symlink`]. On non-Unix platforms it is a best-effort
/// no-op (matching what the replications do, which only create the symlink
/// under `#[cfg(unix)]`).
pub fn refresh_latest_symlink(base: impl AsRef<Path>, target: &str) -> io::Result<()> {
    let link_path = base.as_ref().join("latest");

    // Remove any existing symlink; a missing one is fine.
    if link_path.is_symlink() {
        match fs::remove_file(&link_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &link_path)?;
    }
    #[cfg(not(unix))]
    {
        // Best-effort no-op on non-Unix platforms; `target` is unused there.
        let _ = target;
    }

    Ok(())
}

/// Serialize a slice of serde rows to CSV at `path`.
///
/// Each element of `rows` becomes one CSV record; the column layout (long- or
/// wide-format) is entirely a property of the caller's row type. A header row
/// is written automatically from the first record's field names (standard
/// `csv` crate behaviour). The file is fully flushed before returning.
pub fn write_csv<T: Serialize>(rows: &[T], path: impl AsRef<Path>) -> Result<(), WriteError> {
    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(BufWriter::new(file));
    for row in rows {
        wtr.serialize(row)?;
    }
    wtr.flush()?;
    Ok(())
}

/// Serialize any serde value to pretty-printed JSON at `path`.
///
/// Suitable for the small metadata sidecars the replications emit
/// (`config.json`, `llm_meta.json`, `run_metadata.json`, …). The file is
/// fully flushed before returning.
pub fn write_json<T: Serialize>(value: &T, path: impl AsRef<Path>) -> Result<(), WriteError> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    io::Write::flush(&mut writer)?;
    Ok(())
}

/// Error returned by the serializing writers ([`write_csv`], [`write_json`]).
///
/// Wraps the three underlying failure sources without pulling in a derive
/// macro, keeping this crate's dependency surface minimal.
#[derive(Debug)]
pub enum WriteError {
    /// An I/O error (creating or flushing the output file).
    Io(io::Error),
    /// A CSV serialization error.
    Csv(csv::Error),
    /// A JSON serialization error.
    Json(serde_json::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Io(e) => write!(f, "I/O error: {e}"),
            WriteError::Csv(e) => write!(f, "CSV serialization error: {e}"),
            WriteError::Json(e) => write!(f, "JSON serialization error: {e}"),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WriteError::Io(e) => Some(e),
            WriteError::Csv(e) => Some(e),
            WriteError::Json(e) => Some(e),
        }
    }
}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        WriteError::Io(e)
    }
}

impl From<csv::Error> for WriteError {
    fn from(e: csv::Error) -> Self {
        WriteError::Csv(e)
    }
}

impl From<serde_json::Error> for WriteError {
    fn from(e: serde_json::Error) -> Self {
        WriteError::Json(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique temp subdir per test invocation (avoids cross-test collisions).
    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("socsim-results-{tag}-{pid}-{n}"))
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Row {
        step: usize,
        value: f64,
        label: String,
    }

    #[test]
    fn timestamp_has_expected_shape() {
        let ts = timestamp();
        // "YYYYMMDD_HHMMSS" => 15 chars, underscore at index 8, all else digits.
        assert_eq!(ts.len(), 15, "timestamp = {ts}");
        assert_eq!(ts.as_bytes()[8], b'_');
        for (i, c) in ts.char_indices() {
            if i == 8 {
                continue;
            }
            assert!(c.is_ascii_digit(), "non-digit at {i} in {ts}");
        }
    }

    #[test]
    fn ensure_dir_is_idempotent() {
        let dir = unique_dir("ensure");
        ensure_dir(&dir).unwrap();
        assert!(dir.is_dir());
        // Second call on the existing dir must still succeed.
        ensure_dir(&dir).unwrap();
        assert!(dir.is_dir());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn create_run_dir_creates_timestamped_subdir() {
        let base = unique_dir("runbase");
        let run = create_run_dir(&base).unwrap();
        assert!(run.is_dir());
        assert_eq!(run.parent().unwrap(), base.as_path());
        // The final component must be a valid timestamp shape.
        let name = run.file_name().unwrap().to_string_lossy();
        assert_eq!(name.len(), 15);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refresh_latest_symlink_creates_then_repoints() {
        let base = unique_dir("latest");
        ensure_dir(&base).unwrap();
        // Two run directories to point at, in turn.
        ensure_dir(base.join("run_a")).unwrap();
        ensure_dir(base.join("run_b")).unwrap();

        // First point: ok even though no prior link exists.
        refresh_latest_symlink(&base, "run_a").unwrap();
        // Re-point: existing link is removed and recreated.
        refresh_latest_symlink(&base, "run_b").unwrap();

        #[cfg(unix)]
        {
            let link = base.join("latest");
            assert!(link.is_symlink());
            let dest = fs::read_link(&link).unwrap();
            assert_eq!(dest, Path::new("run_b"));
            // Resolves to the actual run_b directory.
            assert!(link.is_dir());
        }

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn write_csv_round_trips() {
        let dir = unique_dir("csv");
        ensure_dir(&dir).unwrap();
        let path = dir.join("rows.csv");

        let rows = vec![
            Row {
                step: 0,
                value: 1.5,
                label: "a".to_string(),
            },
            Row {
                step: 1,
                value: -2.0,
                label: "b".to_string(),
            },
        ];
        write_csv(&rows, &path).unwrap();

        let mut rdr = csv::Reader::from_path(&path).unwrap();
        let parsed: Vec<Row> = rdr.deserialize().map(|r| r.unwrap()).collect();
        assert_eq!(parsed, rows);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_json_writes_parseable_json() {
        let dir = unique_dir("json");
        ensure_dir(&dir).unwrap();
        let path = dir.join("cfg.json");

        let value = Row {
            step: 7,
            value: 9.5,
            label: "cfg".to_string(),
        };
        write_json(&value, &path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let parsed: Row = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, value);

        fs::remove_dir_all(&dir).unwrap();
    }
}
