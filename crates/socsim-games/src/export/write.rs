use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ExportError, GameSpec, SolutionSet};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Atomically writes a validated specification to `path` as `game_spec.json`
/// schema version 1.
///
/// The parent directory must already exist. The JSON uses two-space pretty
/// indentation and exactly one trailing newline. A unique temporary file is
/// synchronized and then renamed over the destination.
pub fn write_game_spec(spec: &GameSpec, path: &Path) -> Result<(), ExportError> {
    let mut data = Vec::new();
    spec.write_pretty(&mut data)
        .map_err(|error| ExportError::Json(error.to_string()))?;
    data.push(b'\n');
    atomic_write(path, &data)
}

/// Atomically writes a validated solution set to `path` as `solutions.json`
/// schema version 1.
///
/// The parent directory must already exist. The JSON uses two-space pretty
/// indentation and exactly one trailing newline. A unique temporary file is
/// synchronized and then renamed over the destination.
pub fn write_solutions(set: &SolutionSet, path: &Path) -> Result<(), ExportError> {
    let mut data = Vec::new();
    set.write_pretty(&mut data)
        .map_err(|error| ExportError::Json(error.to_string()))?;
    data.push(b'\n');
    atomic_write(path, &data)
}

/// Writes explicit game artifacts below `dir/socsim_games/`.
///
/// This function creates the `socsim_games` directory, writes
/// `game_spec.json`, and writes `solutions.json` when a solution set is
/// supplied. A supplied solution set must have been created from the same
/// specification. No artifacts are written unless this function is called.
pub fn write_artifacts(
    dir: &Path,
    spec: &GameSpec,
    solutions: Option<&SolutionSet>,
) -> Result<(), ExportError> {
    if solutions.is_some_and(|set| !set.matches(spec)) {
        return Err(ExportError::Mismatch(
            "the solution set was created from a different game specification".to_owned(),
        ));
    }
    let artifact_dir = dir.join("socsim_games");
    fs::create_dir_all(&artifact_dir).map_err(|error| io_error(&artifact_dir, error))?;
    write_game_spec(spec, &artifact_dir.join("game_spec.json"))?;
    if let Some(set) = solutions {
        write_solutions(set, &artifact_dir.join("solutions.json"))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), ExportError> {
    let parent = path.parent().ok_or_else(|| ExportError::Io {
        path: path.to_path_buf(),
        message: "destination has no parent directory".to_owned(),
    })?;
    let temporary = unique_temporary_path(parent, path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(data)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn unique_temporary_path(dir: &Path, destination: &Path) -> PathBuf {
    let file = destination
        .file_name()
        .map_or_else(|| "artifact".into(), |name| name.to_string_lossy());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{file}.tmp-{pid}-{nanos}-{sequence}"))
}

fn io_error(path: &Path, error: std::io::Error) -> ExportError {
    ExportError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
