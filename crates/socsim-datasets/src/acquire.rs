//! Network acquisition + raw→CSV conversion (feature `acquire`).
//!
//! Two responsibilities:
//!
//! 1. [`raw_to_csv`] — a Rust port of the pipe-delimited ANES raw → CSV
//!    converter (replacing `scripts/anes_raw_to_csv.py`). It reads the raw file
//!    as latin-1, splits each line on the delimiter, validates a consistent
//!    field count, and re-emits a properly-quoted UTF-8 CSV byte-identical to
//!    what Python's `csv.writer` defaults produce.
//! 2. [`fetch`] — download a [`DatasetMeta`]'s files into a local cache,
//!    atomically, verifying `sha256` + `expect_rows`, skipping cache hits, and
//!    surfacing [`Source::Manual`] files as errors with their instructions URL.
//!
//! Everything here is synchronous (blocking `ureq`), matching socsim's style.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::registry::{DataFile, DatasetMeta, Source};

// ---------------------------------------------------------------------------
// (a) raw pipe-delimited -> CSV converter (Rust port of anes_raw_to_csv.py).
// ---------------------------------------------------------------------------

/// Outcome of a [`raw_to_csv`] conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertReport {
    /// Number of columns (header fields).
    pub columns: usize,
    /// Number of data rows (respondents) written.
    pub data_rows: usize,
}

/// Convert a pipe-delimited raw survey file to a properly-quoted UTF-8 CSV.
///
/// Behaviour matches `scripts/anes_raw_to_csv.py` exactly:
///
/// - The input is read as **latin-1** (each byte `0x00..=0xFF` maps to the same
///   Unicode code point).
/// - Line 1 is the header; it is split on `delimiter`, and each field is
///   trimmed if `strip`. The header's field count fixes the expected column
///   count.
/// - Every subsequent line is split on `delimiter`; a field-count mismatch is an
///   error reporting the **1-based** line number (header = line 1, so the first
///   data line is line 2). Fields are trimmed if `strip`.
/// - The CSV is written to match Python's `csv.writer` defaults byte-for-byte —
///   `QuoteStyle::Necessary` (= `QUOTE_MINIMAL`, the csv-crate default) and a
///   CRLF terminator (set explicitly, since the csv crate writes LF by default).
/// - If `expect_rows` is `Some(n)`, a data-row count `!= n` is an error.
///
/// The output is written to a temporary file in the destination directory and
/// then atomically renamed into place.
pub fn raw_to_csv(
    input: &Path,
    output: &Path,
    delimiter: u8,
    strip: bool,
    expect_rows: Option<usize>,
) -> Result<ConvertReport> {
    // Read raw bytes and decode latin-1: each byte -> the same code point.
    let bytes =
        fs::read(input).with_context(|| format!("failed to read input {}", input.display()))?;
    let text: String = bytes.iter().map(|&b| b as char).collect();

    // Match Python's readline()/iteration: split into lines, dropping a trailing
    // "\n" per line (rstrip("\n")). We split on '\n' and strip a trailing '\r'
    // is NOT done by the Python script (it only rstrips "\n"); to stay byte-exact
    // we replicate rstrip("\n") only.
    let mut lines = text.split('\n');

    let delim_char = delimiter as char;
    let split_line = |line: &str| -> Vec<String> {
        // Python: line.rstrip("\n").split(delimiter). The '\n' is already gone
        // from the split; replicate rstrip("\n") which would also strip multiple
        // trailing newlines — but within a single split segment there are none.
        let fields: Vec<String> = line
            .split(delim_char)
            .map(|f| {
                if strip {
                    f.trim().to_string()
                } else {
                    f.to_string()
                }
            })
            .collect();
        fields
    };

    let header_line = lines
        .next()
        .ok_or_else(|| anyhow!("input {} is empty (no header line)", input.display()))?;
    let header = split_line(header_line);
    let ncols = header.len();

    // Write to a temp file in the output's parent dir, then atomic-rename.
    let out_dir = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output dir {}", out_dir.display()))?;
    let tmp = tempfile::Builder::new()
        .prefix(".raw_to_csv.")
        .suffix(".tmp")
        .tempfile_in(out_dir)
        .with_context(|| format!("failed to create temp file in {}", out_dir.display()))?;

    // Match Python `csv.writer` defaults byte-for-byte: QUOTE_MINIMAL (=
    // QuoteStyle::Necessary, the csv default) + a CRLF line terminator. The csv
    // crate's *write* default terminator is LF, so set CRLF explicitly.
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(Vec::<u8>::new());
    wtr.write_record(&header)
        .context("failed to write CSV header")?;

    let mut data_rows = 0usize;
    // Python enumerates the remaining lines with start=2. Its for-loop iterates
    // over readline()-produced lines, which never yields a final empty line for
    // a trailing newline. We replicate by skipping a single trailing empty
    // segment produced by split('\n') when the file ends in '\n'.
    let collected: Vec<&str> = lines.collect();
    let n = collected.len();
    for (idx, line) in collected.iter().enumerate() {
        // The final segment is empty iff the file ended with '\n' (Python's
        // iteration would not have produced it). Skip exactly that one.
        if idx == n - 1 && line.is_empty() {
            continue;
        }
        let lineno = idx + 2; // header was line 1.
        let fields = split_line(line);
        if fields.len() != ncols {
            bail!(
                "line {} has {} fields, expected {}",
                lineno,
                fields.len(),
                ncols
            );
        }
        wtr.write_record(&fields)
            .with_context(|| format!("failed to write CSV data row (line {lineno})"))?;
        data_rows += 1;
    }

    let csv_bytes = wtr.into_inner().context("failed to finalize CSV writer")?;

    if let Some(expected) = expect_rows {
        if data_rows != expected {
            bail!("expected {} rows, got {}", expected, data_rows);
        }
    }

    // Atomically place the file.
    {
        let mut f = tmp.as_file();
        f.write_all(&csv_bytes)
            .context("failed to write CSV bytes to temp file")?;
        f.flush().ok();
    }
    tmp.persist(output)
        .with_context(|| format!("failed to persist CSV to {}", output.display()))?;

    Ok(ConvertReport {
        columns: ncols,
        data_rows,
    })
}

// ---------------------------------------------------------------------------
// (b) fetch / cache / verify.
// ---------------------------------------------------------------------------

/// Options for [`fetch`].
pub struct FetchOpts {
    /// Destination directory for the cached files. Defaults to `data/` (the
    /// consuming repo's gitignored data dir).
    pub dest: PathBuf,
    /// Bearer token for restricted Dataverse files. Defaults to the
    /// `DATAVERSE_TOKEN` env var if set.
    pub token: Option<String>,
    /// Re-download even on a cache hit.
    pub force: bool,
}

impl Default for FetchOpts {
    fn default() -> Self {
        FetchOpts {
            dest: PathBuf::from("data"),
            token: std::env::var("DATAVERSE_TOKEN").ok(),
            force: false,
        }
    }
}

/// Download each of `meta`'s files into `opts.dest`, verifying as we go.
///
/// For each [`DataFile`]:
///
/// - [`Source::Manual`] is an error carrying the instructions URL (the file
///   cannot be fetched automatically).
/// - Otherwise the file is GET'd (with a `Bearer` auth header for Dataverse when
///   a token is present), written atomically to `opts.dest/<logical_name>`, then
///   verified: its `sha256` is checked against the registry value when known,
///   and `expect_rows` is checked by counting CSV data rows when set.
/// - A cache hit — an existing file whose `sha256` matches (or whose row count
///   matches when no `sha256` is known) — is skipped unless `opts.force`.
///
/// Returns the paths of the files now present on disk.
pub fn fetch(meta: &DatasetMeta, opts: &FetchOpts) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(&opts.dest)
        .with_context(|| format!("failed to create dest dir {}", opts.dest.display()))?;

    let mut written = Vec::with_capacity(meta.files.len());
    for file in meta.files {
        let path = opts.dest.join(file.logical_name);

        // Cache hit?
        if !opts.force && path.exists() && cache_hit(&path, file)? {
            written.push(path);
            continue;
        }

        let url = match &file.source {
            Source::Manual { instructions_url } => {
                bail!(
                    "{} must be obtained manually: {}",
                    file.logical_name,
                    instructions_url
                );
            }
            other => other
                .download_url()
                .ok_or_else(|| anyhow!("no download URL for {}", file.logical_name))?,
        };

        let body = download(&url, &file.source, opts.token.as_deref())
            .with_context(|| format!("failed to download {}", file.logical_name))?;

        atomic_write(&path, &body)?;
        verify(&path, file)?;
        written.push(path);
    }
    Ok(written)
}

/// Whether `path` already satisfies `file`'s verification (cache hit).
fn cache_hit(path: &Path, file: &DataFile) -> Result<bool> {
    if let Some(expected) = file.sha256 {
        return Ok(sha256_hex(path)?.eq_ignore_ascii_case(expected));
    }
    if let Some(expected_rows) = file.expect_rows {
        return Ok(count_csv_data_rows(path)? == expected_rows);
    }
    // No way to validate -> treat an existing file as a hit.
    Ok(true)
}

/// GET `url`, adding a Bearer header for Dataverse sources when `token` is set.
fn download(url: &str, source: &Source, token: Option<&str>) -> Result<Vec<u8>> {
    let mut req = ureq::get(url);
    if let (Source::Dataverse { .. }, Some(tok)) = (source, token) {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    let resp = req.call().context("HTTP request failed")?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .context("failed to read response body")?;
    Ok(buf)
}

/// Verify a freshly-written file against its registry record.
fn verify(path: &Path, file: &DataFile) -> Result<()> {
    if let Some(expected) = file.sha256 {
        let got = sha256_hex(path)?;
        if !got.eq_ignore_ascii_case(expected) {
            bail!(
                "sha256 mismatch for {}: expected {}, got {}",
                file.logical_name,
                expected,
                got
            );
        }
    }
    if let Some(expected_rows) = file.expect_rows {
        let got = count_csv_data_rows(path)?;
        if got != expected_rows {
            bail!(
                "row-count mismatch for {}: expected {}, got {}",
                file.logical_name,
                expected_rows,
                got
            );
        }
    }
    Ok(())
}

/// Atomically write `bytes` to `path` (temp file in the same dir + rename).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("failed to create dir {}", dir.display()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".fetch.")
        .suffix(".tmp")
        .tempfile_in(dir)
        .with_context(|| format!("failed to create temp file in {}", dir.display()))?;
    tmp.write_all(bytes)
        .context("failed to write downloaded bytes")?;
    tmp.flush().ok();
    tmp.persist(path)
        .with_context(|| format!("failed to persist file to {}", path.display()))?;
    Ok(())
}

/// SHA-256 (lowercase hex) of the bytes of `path`.
fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Count the data rows (records, excluding the header) of a CSV file.
fn count_csv_data_rows(path: &Path) -> Result<usize> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("failed to open CSV {}", path.display()))?;
    let mut n = 0usize;
    for rec in reader.records() {
        rec.context("failed to parse CSV record")?;
        n += 1;
    }
    Ok(n)
}
