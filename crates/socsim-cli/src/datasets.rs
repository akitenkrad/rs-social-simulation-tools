//! `socsim datasets` — registry listing, per-dataset acquisition details, and
//! optional on-demand fetch.
//!
//! This module owns the `datasets` subcommand group: the nested [`DatasetsCmd`]
//! clap enum, the command handlers, and all stdout formatting. The dataset
//! registry itself lives in [`socsim_datasets`]; here we only enumerate it
//! ([`socsim_datasets::all`]), resolve keys ([`socsim_datasets::by_key`]), and
//! render human-readable output.
//!
//! The actual network download (`fetch`) is gated behind the CLI's
//! `datasets-acquire` feature (which forwards to `socsim-datasets/acquire`).
//! Without that feature the `fetch` subcommand still parses and resolves the
//! dataset key, but errors with a rebuild hint instead of touching the network.

use anyhow::Result;
use clap::Subcommand;

use socsim_datasets::{DatasetMeta, Source};

#[cfg(feature = "datasets-acquire")]
use std::path::PathBuf;

// ── Subcommand definition ───────────────────────────────────────────────────────

/// Nested `socsim datasets <…>` subcommands.
#[derive(Subcommand, Debug)]
pub enum DatasetsCmd {
    /// List every dataset in the registry with its acquisition kind.
    List,

    /// Show one dataset's provenance and per-file acquisition method.
    Show {
        /// Stable dataset key, e.g. `anes-2020` (see `datasets list`).
        key: String,
    },

    /// Fetch a dataset's auto-downloadable files into a local directory.
    ///
    /// Requires the CLI to be built with `--features datasets-acquire`.
    Fetch {
        /// Stable dataset key, e.g. `ces-2022` (see `datasets list`).
        key: String,

        /// Destination directory for the downloaded files.
        #[arg(long, default_value = "data")]
        dest: std::path::PathBuf,

        /// Re-download even when a verified cache hit already exists.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

/// Dispatch a `datasets` subcommand to its handler.
pub fn run(cmd: &DatasetsCmd) -> Result<()> {
    match cmd {
        DatasetsCmd::List => cmd_list(),
        DatasetsCmd::Show { key } => cmd_show(key),
        DatasetsCmd::Fetch { key, dest, force } => cmd_fetch(key, dest, *force),
    }
}

// ── acquisition-kind derivation ─────────────────────────────────────────────────

/// Summarise how a dataset's files are obtained, by inspecting each file's
/// [`Source`]:
///
/// - `"manual"` — every file is [`Source::Manual`] (license-gated).
/// - `"auto"` — every file is auto-downloadable ([`Source::Dataverse`] /
///   [`Source::Url`]).
/// - `"mixed"` — a mix of the two.
///
/// An empty file list is reported as `"manual"` (nothing is auto-downloadable).
pub fn acquisition_kind(meta: &DatasetMeta) -> &'static str {
    let mut any_manual = false;
    let mut any_auto = false;
    for file in meta.files {
        match file.source {
            Source::Manual { .. } => any_manual = true,
            Source::Dataverse { .. } | Source::Url { .. } => any_auto = true,
        }
    }
    match (any_manual, any_auto) {
        (true, true) => "mixed",
        (false, true) => "auto",
        // Either all-manual, or no files at all -> nothing auto-downloadable.
        _ => "manual",
    }
}

/// `"1 file"` / `"N files"` for a file count.
fn file_count_label(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}

// ── list ────────────────────────────────────────────────────────────────────────

/// `socsim datasets list` — one row per registered dataset.
fn cmd_list() -> Result<()> {
    let datasets = socsim_datasets::all();
    println!("Datasets ({}):", datasets.len());

    // Width the key/name columns to the widest entry for readable alignment.
    let key_w = datasets.iter().map(|m| m.key.len()).max().unwrap_or(0);
    let name_w = datasets.iter().map(|m| m.name.len()).max().unwrap_or(0);

    for m in &datasets {
        let count = file_count_label(m.files.len());
        println!(
            "  {:key_w$}   {:name_w$}   {:>7}   {}",
            m.key,
            m.name,
            count,
            acquisition_kind(m),
            key_w = key_w,
            name_w = name_w,
        );
    }
    println!("Run `socsim datasets show <KEY>` for acquisition details.");
    Ok(())
}

// ── show ────────────────────────────────────────────────────────────────────────

/// `socsim datasets show <KEY>` — provenance overview + per-file acquisition.
fn cmd_show(key: &str) -> Result<()> {
    let meta = resolve(key)?;

    // Overview block.
    println!("{}", meta.name);
    println!("  key:        {}", meta.key);
    println!("  DOI:        {}", meta.doi.unwrap_or("—"));
    println!("  Source URL: {}", meta.source_url);
    println!("  Citation:   {}", meta.citation);
    println!("  License:    {}", meta.license);

    // Per-file acquisition methods.
    println!("\nFiles ({}):", meta.files.len());
    for file in meta.files {
        println!("  {}", file.logical_name);
        match &file.source {
            Source::Manual { instructions_url } => {
                println!("    Acquisition: manual download required");
                println!("    Instructions: {instructions_url}");
                println!(
                    "    Note: obtain this file per the dataset's terms and place it in the \
                     consuming repo's `data/` dir as `{}`.",
                    file.logical_name
                );
            }
            source => {
                // Dataverse / Url: auto-downloadable.
                let url = source
                    .download_url()
                    .expect("non-manual source always has a download URL");
                println!("    Acquisition: auto-downloadable");
                println!("    Download URL: {url}");
                println!(
                    "    Fetch:  socsim datasets fetch {} --dest data/",
                    meta.key
                );
            }
        }
        if let Some(sha) = file.sha256 {
            println!("    sha256: {sha}");
        }
        if let Some(rows) = file.expect_rows {
            println!("    expect_rows: {rows}");
        }
    }
    Ok(())
}

// ── fetch ───────────────────────────────────────────────────────────────────────

/// `socsim datasets fetch <KEY>` — download a dataset's auto files.
///
/// Key resolution is feature-independent: an unknown key errors with the valid
/// key list whether or not `datasets-acquire` is compiled in. Only the actual
/// download is gated behind that feature.
fn cmd_fetch(key: &str, dest: &std::path::Path, force: bool) -> Result<()> {
    let meta = resolve(key)?;
    fetch_gated(meta, dest, force)
}

/// The download itself, when `datasets-acquire` is enabled.
#[cfg(feature = "datasets-acquire")]
fn fetch_gated(meta: &DatasetMeta, dest: &std::path::Path, force: bool) -> Result<()> {
    use socsim_datasets::acquire::{self, FetchOpts};

    let opts = FetchOpts {
        dest: PathBuf::from(dest),
        force,
        ..FetchOpts::default()
    };
    println!(
        "Fetching '{}' ({} file(s)) into '{}'…",
        meta.key,
        meta.files.len(),
        dest.display()
    );
    let written = acquire::fetch(meta, &opts)?;
    for path in &written {
        println!("  wrote {}", path.display());
    }
    println!("Done — {} file(s) present.", written.len());
    Ok(())
}

/// Fallback when the CLI was built without acquisition support.
#[cfg(not(feature = "datasets-acquire"))]
fn fetch_gated(_meta: &DatasetMeta, _dest: &std::path::Path, _force: bool) -> Result<()> {
    anyhow::bail!(
        "this build has no acquisition support; rebuild the CLI with --features datasets-acquire"
    )
}

// ── helpers ─────────────────────────────────────────────────────────────────────

/// Resolve a dataset key to its [`DatasetMeta`], or error with the valid keys.
fn resolve(key: &str) -> Result<&'static DatasetMeta> {
    socsim_datasets::by_key(key).ok_or_else(|| {
        let valid: Vec<&str> = socsim_datasets::all().iter().map(|m| m.key).collect();
        anyhow::anyhow!(
            "unknown dataset key '{key}'; valid keys: {}",
            valid.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_kind_anes_2020_is_manual() {
        assert_eq!(
            acquisition_kind(&socsim_datasets::anes::ANES_2020),
            "manual"
        );
    }

    #[test]
    fn acquisition_kind_ces_2022_is_auto() {
        // CES 2022 Common Content is CC0 and fetched from the Harvard Dataverse
        // access API, so its single file is auto-downloadable.
        assert_eq!(
            acquisition_kind(&socsim_datasets::ces::CES_2022_META),
            "auto"
        );
    }

    #[test]
    fn file_count_label_singular_and_plural() {
        assert_eq!(file_count_label(1), "1 file");
        assert_eq!(file_count_label(0), "0 files");
        assert_eq!(file_count_label(3), "3 files");
    }

    #[test]
    fn resolve_unknown_key_errors() {
        let err = resolve("does-not-exist").unwrap_err().to_string();
        assert!(err.contains("unknown dataset key"));
        assert!(err.contains("anes-2020"));
    }

    #[test]
    fn resolve_known_key_ok() {
        assert_eq!(resolve("anes-2020").unwrap().key, "anes-2020");
    }
}
