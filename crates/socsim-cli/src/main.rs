//! `socsim` — CLI for the socsim social-simulation platform.
//!
//! # Subcommands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `init`      | Generate a starter scenario TOML |
//! | `run`       | Execute a scenario (single or multi-seed) |
//! | `validate`  | Check scenario TOML against the pack's registry |
//! | `list`      | List packs or mechanisms |
//! | `sweep`     | Grid parameter sweep |
//! | `summarize` | Re-aggregate existing JSONL logs |
//!
//! # Example
//!
//! ```sh
//! socsim run scenarios/hr_lifecycle_baseline.toml
//! socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..5 --parallel
//! socsim sweep scenarios/hr_lifecycle_baseline.toml \
//!     --param "peer_effect.alpha_peer=0.1,0.17,0.3" \
//!     --seeds 0..10 --out runs/sweep/
//! ```

mod packs;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use socsim_config::Scenario;
use socsim_runner::{summarize, SweepAxis};

// ── CLI top level ─────────────────────────────────────────────────────────────

/// socsim — social simulation platform CLI
#[derive(Parser, Debug)]
#[command(name = "socsim", author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate a starter scenario TOML for a module pack.
    Init {
        /// Module pack to use (e.g. `hr-lifecycle`).
        #[arg(long)]
        module_pack: String,

        /// Output path for the generated TOML.
        #[arg(long, short)]
        out: PathBuf,
    },

    /// Run a scenario (single seed or multi-seed).
    Run {
        /// Path to the scenario TOML.
        scenario: PathBuf,

        /// Seed range `A..B` (exclusive upper bound), e.g. `0..5`.
        /// If omitted, runs the single seed specified in the scenario.
        #[arg(long)]
        seeds: Option<String>,

        /// Run seeds in parallel (requires --seeds).
        #[arg(long, default_value_t = false)]
        parallel: bool,
    },

    /// Validate a scenario TOML against the pack's registry.
    Validate {
        /// Path to the scenario TOML.
        scenario: PathBuf,
    },

    /// List available module packs or mechanisms.
    List {
        /// What to list: `packs` or `mechanisms`.
        what: String,
    },

    /// Run a grid parameter sweep.
    Sweep {
        /// Path to the scenario TOML.
        scenario: PathBuf,

        /// Parameter sweep axes, e.g. `peer_effect.alpha_peer=0.1,0.17,0.3`.
        /// May be repeated for multi-dimensional sweeps.
        #[arg(long, value_name = "MECH.PARAM=V1,V2,...")]
        param: Vec<String>,

        /// Seed range `A..B` (e.g. `0..10`).
        #[arg(long, default_value = "0..5")]
        seeds: String,

        /// Output directory for per-combo CSV summaries.
        #[arg(long, short, default_value = "runs/sweep")]
        out: PathBuf,

        /// Run seeds in parallel within each combo.
        #[arg(long, default_value_t = false)]
        parallel: bool,
    },

    /// Re-aggregate existing JSONL run logs into a summary.
    Summarize {
        /// Path to a directory of JSONL files or a single JSONL file.
        path: PathBuf,

        /// Output format: `csv` (default) or `json`.
        #[arg(long, default_value = "csv")]
        format: String,
    },
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { module_pack, out } => cmd_init(&module_pack, &out),
        Commands::Run {
            scenario,
            seeds,
            parallel,
        } => cmd_run(&scenario, seeds.as_deref(), parallel),
        Commands::Validate { scenario } => cmd_validate(&scenario),
        Commands::List { what } => cmd_list(&what),
        Commands::Sweep {
            scenario,
            param,
            seeds,
            out,
            parallel,
        } => cmd_sweep(&scenario, &param, &seeds, &out, parallel),
        Commands::Summarize { path, format } => cmd_summarize(&path, &format),
    }
}

// ── init ──────────────────────────────────────────────────────────────────────

fn cmd_init(module_pack: &str, out: &Path) -> Result<()> {
    let template = packs::starter_toml(module_pack)
        .with_context(|| format!("unknown module pack '{module_pack}'"))?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
        }
    }
    std::fs::write(out, template)
        .with_context(|| format!("failed to write scenario to '{}'", out.display()))?;
    println!("Wrote starter scenario to '{}'", out.display());
    Ok(())
}

// ── run ───────────────────────────────────────────────────────────────────────

fn cmd_run(scenario_path: &Path, seeds_arg: Option<&str>, parallel: bool) -> Result<()> {
    let scenario = Scenario::from_path(scenario_path)
        .with_context(|| format!("failed to load '{}'", scenario_path.display()))?;

    let seeds: Vec<u64> = if let Some(range_str) = seeds_arg {
        parse_seed_range(range_str)?
    } else {
        vec![scenario.simulation.seed]
    };

    let pack = packs::dispatch(&scenario.simulation.module_pack)
        .with_context(|| format!("unknown module pack '{}'", scenario.simulation.module_pack))?;

    println!(
        "Running '{}' (pack={}, t_max={}, seeds=[{}], parallel={})",
        scenario.simulation.name,
        scenario.simulation.module_pack,
        scenario.simulation.t_max,
        format_seeds(&seeds),
        parallel
    );

    let results = pack
        .run_seeds(&scenario, &seeds, parallel)
        .context("simulation run failed")?;

    if results.len() == 1 {
        // Single-seed: print metric time series (condensed).
        let r = &results[0];
        println!("\nSeed {} — {} events recorded\n", r.seed, r.event_count);
        print_series(r);
    } else {
        // Multi-seed: print cross-seed summary.
        let summary = summarize(&results);
        println!("\nCross-seed summary ({} seeds):\n", results.len());
        print_summary_table(&summary);
    }

    // Write JSONL logs.
    for r in &results {
        write_jsonl_log(&scenario, r)?;
    }

    Ok(())
}

// ── validate ──────────────────────────────────────────────────────────────────

fn cmd_validate(scenario_path: &Path) -> Result<()> {
    let scenario = Scenario::from_path(scenario_path)
        .with_context(|| format!("failed to load '{}'", scenario_path.display()))?;

    let pack = packs::dispatch(&scenario.simulation.module_pack)
        .with_context(|| format!("unknown module pack '{}'", scenario.simulation.module_pack))?;

    let names = pack.mechanism_names();

    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    scenario
        .validate(&name_refs)
        .with_context(|| format!("validation failed for '{}'", scenario_path.display()))?;

    println!("OK — scenario '{}' is valid.", scenario_path.display());
    Ok(())
}

// ── list ──────────────────────────────────────────────────────────────────────

fn cmd_list(what: &str) -> Result<()> {
    match what {
        "packs" => {
            println!("Available module packs:");
            for name in packs::known_packs() {
                println!("  {name}");
            }
        }
        "mechanisms" => {
            println!("Mechanisms by pack:");
            for pack in packs::packs() {
                println!("  [{}]", pack.name());
                for name in pack.mechanism_names() {
                    println!("    {name}");
                }
            }
        }
        other => {
            anyhow::bail!("unknown list target '{other}'; valid values: packs, mechanisms");
        }
    }
    Ok(())
}

// ── sweep ─────────────────────────────────────────────────────────────────────

fn cmd_sweep(
    scenario_path: &Path,
    param_args: &[String],
    seeds_arg: &str,
    out_dir: &Path,
    parallel: bool,
) -> Result<()> {
    let scenario = Scenario::from_path(scenario_path)
        .with_context(|| format!("failed to load '{}'", scenario_path.display()))?;

    let seeds = parse_seed_range(seeds_arg)?;

    let axes: Vec<SweepAxis> = param_args
        .iter()
        .map(|s| parse_sweep_axis(s))
        .collect::<Result<_>>()?;

    let pack = packs::dispatch(&scenario.simulation.module_pack)
        .with_context(|| format!("unknown module pack '{}'", scenario.simulation.module_pack))?;

    println!(
        "Sweeping '{}' over {} axes × {} seeds",
        scenario.simulation.name,
        axes.len(),
        seeds.len()
    );
    for axis in &axes {
        println!("  {} = {:?}", axis.param_key, axis.values);
    }

    let points = pack
        .run_sweep(&scenario, &axes, &seeds, parallel)
        .context("sweep failed")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output dir '{}'", out_dir.display()))?;

    for (i, point) in points.iter().enumerate() {
        let combo_label = point
            .params
            .iter()
            .map(|(k, v)| format!("{k}={v:.4}"))
            .collect::<Vec<_>>()
            .join("_");
        let file_name = format!("combo_{i:04}_{combo_label}.csv");
        let file_path = out_dir.join(&file_name);
        std::fs::write(&file_path, point.summary.to_csv())
            .with_context(|| format!("failed to write '{}'", file_path.display()))?;

        println!("  combo {i}: {combo_label}");
        print_summary_table(&point.summary);
    }

    println!(
        "\nWrote {} CSV files to '{}'",
        points.len(),
        out_dir.display()
    );
    Ok(())
}

// ── summarize ─────────────────────────────────────────────────────────────────

fn cmd_summarize(path: &Path, format: &str) -> Result<()> {
    let jsonl_files = collect_jsonl_files(path)?;
    if jsonl_files.is_empty() {
        anyhow::bail!("no JSONL files found at '{}'", path.display());
    }

    // Parse each file into a RunResult by re-aggregating metrics.
    let mut results = Vec::new();
    for file in &jsonl_files {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read '{}'", file.display()))?;
        let r = parse_jsonl_to_run_result(file, &content)?;
        results.push(r);
    }

    let summary = summarize(&results);

    match format {
        "csv" => print!("{}", summary.to_csv()),
        "json" => println!("{}", summary.to_json()),
        other => anyhow::bail!("unknown format '{other}'; valid: csv, json"),
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Parse a seed range string `"A..B"` into a sorted `Vec<u64>`.
fn parse_seed_range(s: &str) -> Result<Vec<u64>> {
    let parts: Vec<&str> = s.splitn(2, "..").collect();
    if parts.len() != 2 {
        anyhow::bail!("seed range must be 'A..B' (exclusive upper bound), got '{s}'");
    }
    let start: u64 = parts[0]
        .trim()
        .parse()
        .with_context(|| format!("invalid seed range start '{}'", parts[0]))?;
    let end: u64 = parts[1]
        .trim()
        .parse()
        .with_context(|| format!("invalid seed range end '{}'", parts[1]))?;
    if start >= end {
        anyhow::bail!("seed range start ({start}) must be < end ({end})");
    }
    Ok((start..end).collect())
}

/// Parse `"mech.param=v1,v2,v3"` into a [`SweepAxis`].
fn parse_sweep_axis(s: &str) -> Result<SweepAxis> {
    let (key_part, values_part) = s
        .split_once('=')
        .with_context(|| format!("sweep param must be 'mech.param=v1,v2,...', got '{s}'"))?;

    let values: Vec<f64> = values_part
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<f64>()
                .with_context(|| format!("invalid float '{v}' in sweep param"))
        })
        .collect::<Result<_>>()?;

    if values.is_empty() {
        anyhow::bail!("sweep axis '{key_part}' has no values");
    }

    Ok(SweepAxis {
        param_key: key_part.to_owned(),
        values,
    })
}

/// Collect all `.jsonl` files under a path (recursively if directory).
fn collect_jsonl_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    if !path.is_dir() {
        anyhow::bail!("'{}' is neither a file nor a directory", path.display());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)
        .with_context(|| format!("failed to read directory '{}'", path.display()))?
    {
        let entry = entry.context("directory entry error")?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(p);
        }
    }
    files.sort();
    Ok(files)
}

/// Parse JSONL content into a minimal [`socsim_runner::RunResult`] by
/// aggregating metric lines.
fn parse_jsonl_to_run_result(path: &Path, content: &str) -> Result<socsim_runner::RunResult> {
    use std::collections::HashMap;
    let mut series: HashMap<String, Vec<(u64, f64)>> = HashMap::new();
    let mut event_count = 0usize;

    for (lineno, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).with_context(|| {
            format!(
                "invalid JSON on line {} of '{}'",
                lineno + 1,
                path.display()
            )
        })?;
        match v.get("type").and_then(|t| t.as_str()) {
            Some("metric") => {
                let t = v.get("t").and_then(|x| x.as_u64()).unwrap_or(0);
                let key = v
                    .get("key")
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown")
                    .to_owned();
                let value = v.get("value").and_then(|x| x.as_f64()).unwrap_or(0.0);
                series.entry(key).or_default().push((t, value));
            }
            Some("event") => {
                event_count += 1;
            }
            _ => {}
        }
    }

    // Derive seed from filename stem (best effort).
    let seed: u64 = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.split('_').next_back())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut final_metrics: HashMap<String, f64> = HashMap::new();
    for (key, ts) in &series {
        if let Some(&(_, last_val)) = ts.last() {
            final_metrics.insert(key.clone(), last_val);
        }
    }

    Ok(socsim_runner::RunResult {
        seed,
        series,
        final_metrics,
        event_count,
    })
}

/// Format a seeds list for display, truncating long lists.
fn format_seeds(seeds: &[u64]) -> String {
    if seeds.len() <= 6 {
        seeds
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        format!(
            "{}..{} ({} seeds)",
            seeds[0],
            seeds[seeds.len() - 1] + 1,
            seeds.len()
        )
    }
}

/// Write a JSONL log for one run result.
fn write_jsonl_log(scenario: &Scenario, result: &socsim_runner::RunResult) -> Result<()> {
    let log_path = scenario
        .output
        .resolve_log_path(&scenario.simulation.name, result.seed);
    let path = Path::new(&log_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create log directory '{}'", parent.display())
            })?;
        }
    }

    use std::io::Write;
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create log file '{}'", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);

    // Write metric rows.
    for (key, ts) in &result.series {
        for &(t, value) in ts {
            let obj = serde_json::json!({
                "type": "metric",
                "t": t,
                "key": key,
                "value": value,
            });
            writeln!(writer, "{}", serde_json::to_string(&obj)?)?;
        }
    }

    Ok(())
}

/// Print a condensed metric series table for a single run.
fn print_series(result: &socsim_runner::RunResult) {
    let mut keys: Vec<&str> = result.series.keys().map(|s| s.as_str()).collect();
    keys.sort();

    // Header
    print!("{:<6}", "t");
    for k in &keys {
        print!("  {:>16}", k);
    }
    println!();

    // Find max t.
    let t_max = result
        .series
        .values()
        .flat_map(|ts| ts.iter().map(|&(t, _)| t))
        .max()
        .unwrap_or(0);

    // Print every 10th step, plus the last.
    for &(t, _) in result
        .series
        .values()
        .next()
        .map(|ts| ts.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter(|&&(t, _)| t % 10 == 0 || t == t_max)
    {
        print!("{:<6}", t);
        for k in &keys {
            let val = result
                .series
                .get(*k)
                .and_then(|ts| ts.iter().find(|&&(tx, _)| tx == t))
                .map(|&(_, v)| v);
            match val {
                Some(v) => print!("  {:>16.4}", v),
                None => print!("  {:>16}", "-"),
            }
        }
        println!();
    }
}

/// Print a cross-seed summary table.
fn print_summary_table(summary: &socsim_runner::Summary) {
    println!(
        "{:<20}  {:>10}  {:>10}  {:>10}  {:>10}  {:>5}",
        "metric", "mean", "std", "min", "max", "n"
    );
    println!("{}", "-".repeat(72));
    for m in &summary.metrics {
        println!(
            "{:<20}  {:>10.4}  {:>10.4}  {:>10.4}  {:>10.4}  {:>5}",
            m.key, m.mean, m.std, m.min, m.max, m.n
        );
    }
}
