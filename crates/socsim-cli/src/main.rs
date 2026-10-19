//! `socsim` — CLI for the socsim social-simulation platform.
//!
//! # Subcommands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `init`      | Generate a starter scenario TOML |
//! | `run`       | Execute a scenario (single or multi-seed) |
//! | `validate`  | Validate a scenario, then apply the selected pack's own checks |
//! | `list`      | List packs or mechanisms |
//! | `sweep`     | Grid parameter sweep |
//! | `summarize` | Re-aggregate existing JSONL logs |
//! | `datasets`  | List dataset registry, show a dataset's acquisition method, or fetch it |
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

mod datasets;
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

    /// Validate a scenario TOML, then apply the selected pack's own checks.
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
        /// The normal-form pack also accepts `world.*` paths, e.g.
        /// `world.game.temptation=1.6,1.9`.
        /// May be repeated for multi-dimensional sweeps.
        #[arg(long, value_name = "MECH.PARAM|world.PATH=V1,V2,...")]
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

    /// List the dataset registry, show a dataset's acquisition method, or fetch it.
    Datasets {
        #[command(subcommand)]
        cmd: datasets::DatasetsCmd,
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
        Commands::Datasets { cmd } => datasets::run(&cmd),
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

    let pack = packs::dispatch(&scenario.simulation.module_pack)
        .with_context(|| format!("unknown module pack '{}'", scenario.simulation.module_pack))?;

    check_scenario(pack.as_ref(), &scenario, scenario_path)?;

    let seeds: Vec<u64> = if let Some(range_str) = seeds_arg {
        parse_seed_range(range_str)?
    } else {
        vec![scenario.simulation.seed]
    };

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

    check_scenario(pack.as_ref(), &scenario, scenario_path)?;

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

    let pack = packs::dispatch(&scenario.simulation.module_pack)
        .with_context(|| format!("unknown module pack '{}'", scenario.simulation.module_pack))?;

    check_scenario(pack.as_ref(), &scenario, scenario_path)?;

    let seeds = parse_seed_range(seeds_arg)?;

    let axes: Vec<SweepAxis> = param_args
        .iter()
        .map(|s| parse_sweep_axis(s))
        .collect::<Result<_>>()?;

    println!(
        "Sweeping '{}' over {} axes × {} seeds",
        scenario.simulation.name,
        axes.len(),
        seeds.len()
    );
    for axis in &axes {
        println!("  {} = {:?}", axis.param_key, axis.values);
    }

    let rows = pack
        .run_sweep_rows(&scenario, &axes, &seeds, parallel)
        .context("sweep failed")?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output dir '{}'", out_dir.display()))?;

    for (i, row) in rows.iter().enumerate() {
        let combo_label = write_sweep_row(out_dir, i, row)?;

        println!("  combo {i}: {combo_label}");
        print_summary_table(&row.summary);
    }

    println!(
        "\nWrote {} CSV files to '{}'",
        rows.len(),
        out_dir.display()
    );
    Ok(())
}

/// Label of one sweep combination: `{k}={v:.4}` joined with `_`.
fn sweep_combo_label(params: &[(String, f64)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={v:.4}"))
        .collect::<Vec<_>>()
        .join("_")
}

/// Write the files of one sweep combination into `out_dir` and return its label.
///
/// Always writes the summary CSV `combo_{i:04}_{label}.csv`.  When the row
/// carries an effective scenario, also writes it as TOML to
/// `combo_{i:04}_{label}.scenario.toml` (same label as the CSV).
fn write_sweep_row(out_dir: &Path, i: usize, row: &packs::SweepRow) -> Result<String> {
    let combo_label = sweep_combo_label(&row.params);

    let file_name = format!("combo_{i:04}_{combo_label}.csv");
    let file_path = out_dir.join(&file_name);
    std::fs::write(&file_path, row.summary.to_csv())
        .with_context(|| format!("failed to write '{}'", file_path.display()))?;

    if let Some(effective) = &row.effective {
        let toml_name = format!("combo_{i:04}_{combo_label}.scenario.toml");
        let toml_path = out_dir.join(&toml_name);
        let text = toml::to_string(effective).with_context(|| {
            format!(
                "failed to serialize the effective scenario for '{}'",
                toml_path.display()
            )
        })?;
        std::fs::write(&toml_path, text)
            .with_context(|| format!("failed to write '{}'", toml_path.display()))?;
    }

    Ok(combo_label)
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

fn check_scenario(pack: &dyn packs::CliPack, scenario: &Scenario, path: &Path) -> Result<()> {
    let result: Result<()> = (|| {
        let names = pack.mechanism_names();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        scenario.validate(&name_refs)?;
        pack.validate(scenario)?;
        Ok(())
    })();

    result.with_context(|| format!("validation failed for '{}'", path.display()))
}

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
    let mut events: Vec<socsim_log::EventRow> = Vec::new();

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
                let t = v.get("t").and_then(|x| x.as_u64()).unwrap_or(0);
                let kind = v
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned();
                let payload = v.get("payload").cloned().unwrap_or(serde_json::Value::Null);
                events.push(socsim_log::EventRow { t, kind, payload });
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

    let event_count = events.len();
    Ok(socsim_runner::RunResult {
        seed,
        series,
        final_metrics,
        events,
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

    // Write metric rows in key order, preserving each series' recorded order.
    let mut metric_keys: Vec<&String> = result.series.keys().collect();
    metric_keys.sort();
    for key in metric_keys {
        let ts = &result.series[key];
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

    // Write event rows in the schema documented by `socsim_log::JsonlRecorder`.
    // Events are emitted after metrics so existing consumers that filter on
    // `type == "metric"` see no behavioural change.
    for ev in &result.events {
        let obj = serde_json::json!({
            "type": "event",
            "t": ev.t,
            "kind": ev.kind,
            "payload": ev.payload,
        });
        writeln!(writer, "{}", serde_json::to_string(&obj)?)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    /// Build a minimal scenario with a temp-dir log path.
    fn scenario_with_log(log_template: &str) -> socsim_config::Scenario {
        let toml = format!(
            r#"
[simulation]
name        = "events_jsonl_test"
module_pack = "hr-lifecycle"
t_max       = 1
seed        = 0
scheduler   = "sequential"

[world]

[[mechanism]]
name  = "org_performance"
phase = "reward"

[output]
log_path = "{log_template}"
"#
        );
        socsim_config::Scenario::parse(&toml).expect("test scenario must parse")
    }

    #[test]
    fn write_jsonl_log_emits_metric_and_event_rows() {
        let tmpdir = std::env::temp_dir().join("socsim_jsonl_events_test");
        let _ = fs::remove_dir_all(&tmpdir);
        fs::create_dir_all(&tmpdir).unwrap();
        let log_template = format!("{}/{{name}}_{{seed}}.jsonl", tmpdir.to_str().unwrap());

        let scenario = scenario_with_log(&log_template);

        let mut series: HashMap<String, Vec<(u64, f64)>> = HashMap::new();
        series.insert("alpha".to_owned(), vec![(0, 1.5), (1, 2.5)]);
        let mut final_metrics: HashMap<String, f64> = HashMap::new();
        final_metrics.insert("alpha".to_owned(), 2.5);
        let events = vec![
            socsim_log::EventRow {
                t: 0,
                kind: "spawn".to_owned(),
                payload: serde_json::json!({ "agent_id": 7 }),
            },
            socsim_log::EventRow {
                t: 1,
                kind: "shock".to_owned(),
                payload: serde_json::json!({ "magnitude": 0.42 }),
            },
        ];
        let event_count = events.len();

        let result = socsim_runner::RunResult {
            seed: 0,
            series,
            final_metrics,
            events,
            event_count,
        };

        write_jsonl_log(&scenario, &result).expect("write_jsonl_log");

        let path = tmpdir.join("events_jsonl_test_0.jsonl");
        let content = fs::read_to_string(&path).expect("read back log");

        let lines: Vec<&str> = content.lines().collect();
        let by_type: Vec<serde_json::Value> = lines
            .iter()
            .map(|l| serde_json::from_str(l).expect("each line is JSON"))
            .collect();

        let metric_lines: Vec<_> = by_type
            .iter()
            .filter(|v| v.get("type").and_then(|x| x.as_str()) == Some("metric"))
            .collect();
        let event_lines: Vec<_> = by_type
            .iter()
            .filter(|v| v.get("type").and_then(|x| x.as_str()) == Some("event"))
            .collect();

        assert_eq!(metric_lines.len(), 2, "two metric rows expected");
        assert_eq!(event_lines.len(), 2, "two event rows expected");

        // Event schema must match `socsim_log::JsonlRecorder`'s.
        let spawn_ev = event_lines
            .iter()
            .find(|v| v.get("kind").and_then(|x| x.as_str()) == Some("spawn"))
            .expect("spawn event present");
        assert_eq!(spawn_ev.get("t").and_then(|x| x.as_u64()), Some(0));
        assert_eq!(
            spawn_ev
                .get("payload")
                .and_then(|p| p.get("agent_id"))
                .and_then(|x| x.as_u64()),
            Some(7)
        );

        let _ = fs::remove_dir_all(&tmpdir);
    }

    // ── sweep rows ────────────────────────────────────────────────────────────

    #[test]
    fn write_jsonl_log_is_byte_identical_for_equivalent_run_results() {
        struct TempDir(PathBuf);

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn equivalent_result() -> socsim_runner::RunResult {
            let mut series = HashMap::new();
            for (index, key) in [
                "zeta", "alpha", "theta", "beta", "eta", "gamma", "iota", "delta", "kappa",
                "epsilon", "lambda", "mu",
            ]
            .into_iter()
            .enumerate()
            {
                let value = index as f64;
                series.insert(key.to_owned(), vec![(2, value), (1, value + 0.5)]);
            }

            socsim_runner::RunResult {
                seed: 0,
                final_metrics: series
                    .iter()
                    .map(|(key, values)| (key.clone(), values.last().unwrap().1))
                    .collect(),
                series,
                events: vec![
                    socsim_log::EventRow {
                        t: 2,
                        kind: "first".to_owned(),
                        payload: serde_json::json!({ "order": 1 }),
                    },
                    socsim_log::EventRow {
                        t: 1,
                        kind: "second".to_owned(),
                        payload: serde_json::json!({ "order": 2 }),
                    },
                ],
                event_count: 2,
            }
        }

        let tmpdir = TempDir(fresh_temp_dir("jsonl_determinism"));
        let log_template = format!("{}/{{name}}_{{seed}}.jsonl", tmpdir.0.display());
        let scenario = scenario_with_log(&log_template);
        let path = tmpdir.0.join("events_jsonl_test_0.jsonl");

        write_jsonl_log(&scenario, &equivalent_result()).expect("first JSONL write");
        let first = fs::read(&path).expect("read first JSONL output");

        write_jsonl_log(&scenario, &equivalent_result()).expect("second JSONL write");
        let second = fs::read(&path).expect("read second JSONL output");

        assert_eq!(first, second, "equivalent results must be byte-identical");

        let rows: Vec<serde_json::Value> = String::from_utf8(second)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let metric_key_times: Vec<(&str, u64)> = rows
            .iter()
            .filter(|row| row["type"] == "metric")
            .map(|row| (row["key"].as_str().unwrap(), row["t"].as_u64().unwrap()))
            .collect();
        let expected: Vec<(&str, u64)> = [
            "alpha", "beta", "delta", "epsilon", "eta", "gamma", "iota", "kappa", "lambda", "mu",
            "theta", "zeta",
        ]
        .into_iter()
        .flat_map(|key| [(key, 2), (key, 1)])
        .collect();
        assert_eq!(metric_key_times, expected);

        let event_kinds: Vec<&str> = rows
            .iter()
            .filter(|row| row["type"] == "event")
            .map(|row| row["kind"].as_str().unwrap())
            .collect();
        assert_eq!(event_kinds, ["first", "second"]);
    }

    use packs::{CliPack, SweepRow};
    use socsim_runner::{MetricStats, Summary, SweepPoint};

    /// A unique, freshly created directory under `std::env::temp_dir()`.
    fn fresh_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socsim_cli_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn summary_of(key: &str, mean: f64) -> Summary {
        Summary {
            metrics: vec![MetricStats {
                key: key.to_owned(),
                mean,
                std: 0.0,
                min: mean,
                max: mean,
                n: 2,
            }],
        }
    }

    /// A pack that only implements the legacy `run_sweep`, with fixed output.
    /// Available in every feature configuration (including no packs).
    struct StubSweepPack;

    impl CliPack for StubSweepPack {
        fn name(&self) -> &'static str {
            "stub-sweep"
        }
        fn starter_toml(&self) -> &'static str {
            ""
        }
        fn mechanism_names(&self) -> Vec<String> {
            Vec::new()
        }
        fn run_seeds(
            &self,
            _scenario: &Scenario,
            _seeds: &[u64],
            _parallel: bool,
        ) -> Result<Vec<socsim_runner::RunResult>> {
            Ok(Vec::new())
        }
        fn run_sweep(
            &self,
            _scenario: &Scenario,
            axes: &[SweepAxis],
            _seeds: &[u64],
            _parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            let axis = &axes[0];
            Ok(axis
                .values
                .iter()
                .map(|&v| SweepPoint {
                    params: vec![(axis.param_key.clone(), v)],
                    summary: summary_of("m", v * 10.0),
                })
                .collect())
        }
    }

    fn assert_rows_match_points(rows: &[SweepRow], points: &[SweepPoint]) {
        assert_eq!(rows.len(), points.len());
        for (row, point) in rows.iter().zip(points) {
            assert_eq!(row.params, point.params);
            assert_eq!(
                serde_json::to_value(&row.summary).unwrap(),
                serde_json::to_value(&point.summary).unwrap()
            );
            assert!(
                row.effective.is_none(),
                "default adapter must not report effective"
            );
        }
    }

    #[test]
    fn default_run_sweep_rows_matches_run_sweep_stub() {
        let scenario = scenario_with_log("unused_{name}_{seed}.jsonl");
        let axes = vec![SweepAxis {
            param_key: "m.p".to_owned(),
            values: vec![0.25, 0.5, 0.75],
        }];
        let pack = StubSweepPack;
        let points = pack.run_sweep(&scenario, &axes, &[0, 1], false).unwrap();
        let rows = pack
            .run_sweep_rows(&scenario, &axes, &[0, 1], false)
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_rows_match_points(&rows, &points);
    }

    #[cfg(feature = "pack-hr-lifecycle")]
    #[test]
    fn default_run_sweep_rows_matches_run_sweep_hr_lifecycle() {
        let mut scenario = Scenario::parse(include_str!(
            "../../../scenarios/hr_lifecycle_baseline.toml"
        ))
        .expect("baseline scenario must parse");
        scenario.simulation.t_max = 10;
        let axes = vec![
            SweepAxis {
                param_key: "toxic_spread.p_spread".to_owned(),
                values: vec![0.2, 0.46],
            },
            SweepAxis {
                param_key: "peer_effect.alpha_peer".to_owned(),
                values: vec![0.1],
            },
        ];
        let pack = packs::dispatch("hr-lifecycle").unwrap();
        let points = pack.run_sweep(&scenario, &axes, &[0, 1], false).unwrap();
        let rows = pack
            .run_sweep_rows(&scenario, &axes, &[0, 1], false)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_rows_match_points(&rows, &points);
    }

    #[test]
    fn write_sweep_row_writes_scenario_toml_only_when_effective_is_some() {
        let dir = fresh_temp_dir("sweep_rows");

        let mut effective = scenario_with_log("eff_{name}_{seed}.jsonl");
        effective.simulation.t_max = 7;
        effective.mechanisms[0].params.set_f64("p", 0.3);

        let rows = [
            SweepRow {
                params: vec![("m.p".to_owned(), 0.3)],
                summary: summary_of("m", 3.0),
                effective: Some(effective.clone()),
            },
            SweepRow {
                params: vec![("m.p".to_owned(), 0.6)],
                summary: summary_of("m", 6.0),
                effective: None,
            },
        ];

        let labels: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| write_sweep_row(&dir, i, row).expect("write_sweep_row"))
            .collect();
        assert_eq!(labels, vec!["m.p=0.3000", "m.p=0.6000"]);

        // Both CSVs are written, with the summary CSV as content.
        for (i, row) in rows.iter().enumerate() {
            let csv = dir.join(format!("combo_{i:04}_{}.csv", labels[i]));
            assert_eq!(fs::read_to_string(&csv).unwrap(), row.summary.to_csv());
        }

        // The `Some` row has a `.scenario.toml` with the same label, which
        // reads back with `Scenario::parse`.
        let toml_path = dir.join("combo_0000_m.p=0.3000.scenario.toml");
        let text = fs::read_to_string(&toml_path).expect("scenario toml written");
        let back = Scenario::parse(&text).expect("scenario toml parses back");
        assert_eq!(back.simulation.name, effective.simulation.name);
        assert_eq!(back.simulation.t_max, 7);
        assert_eq!(back.mechanisms.len(), effective.mechanisms.len());
        assert_eq!(back.mechanisms[0].name, "org_performance");
        assert_eq!(back.mechanisms[0].phase.as_deref(), Some("reward"));
        assert_eq!(
            back.mechanisms[0].params.0.get("p"),
            Some(&toml::Value::Float(0.3))
        );
        assert_eq!(back.output.log_path, effective.output.log_path);

        // The `None` row has no `.scenario.toml`; nothing else was written.
        assert!(!dir.join("combo_0001_m.p=0.6000.scenario.toml").exists());
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "combo_0000_m.p=0.3000.csv",
                "combo_0000_m.p=0.3000.scenario.toml",
                "combo_0001_m.p=0.6000.csv",
            ]
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ── scenario validation ─────────────────────────────────────────────────

    use std::sync::{Arc, Mutex};

    struct StubValidationPack {
        calls: Arc<Mutex<Vec<&'static str>>>,
        reject: bool,
    }

    impl CliPack for StubValidationPack {
        fn name(&self) -> &'static str {
            "stub-validation"
        }

        fn starter_toml(&self) -> &'static str {
            ""
        }

        fn mechanism_names(&self) -> Vec<String> {
            self.calls.lock().unwrap().push("mechanism_names");
            vec!["org_performance".to_owned()]
        }

        fn validate(&self, _scenario: &Scenario) -> Result<()> {
            self.calls.lock().unwrap().push("pack_validate");
            if self.reject {
                anyhow::bail!("pack validation rejected the scenario");
            }
            Ok(())
        }

        fn run_seeds(
            &self,
            _scenario: &Scenario,
            _seeds: &[u64],
            _parallel: bool,
        ) -> Result<Vec<socsim_runner::RunResult>> {
            unreachable!("validation tests do not run simulations")
        }

        fn run_sweep(
            &self,
            _scenario: &Scenario,
            _axes: &[SweepAxis],
            _seeds: &[u64],
            _parallel: bool,
        ) -> Result<Vec<SweepPoint>> {
            unreachable!("validation tests do not run sweeps")
        }
    }

    #[test]
    fn check_scenario_stops_before_pack_validation_when_scenario_is_invalid() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pack = StubValidationPack {
            calls: Arc::clone(&calls),
            reject: false,
        };
        let mut scenario = scenario_with_log("unused_{name}_{seed}.jsonl");
        scenario.simulation.module_pack = "stub-validation".to_owned();
        scenario.simulation.t_max = 0;

        let error = check_scenario(&pack, &scenario, Path::new("zero.toml")).unwrap_err();

        assert_eq!(*calls.lock().unwrap(), vec!["mechanism_names"]);
        let message = format!("{error:#}");
        assert!(message.contains("validation failed for 'zero.toml'"));
        assert!(message.contains("simulation.t_max must be > 0"));
    }

    #[test]
    fn check_scenario_calls_pack_validation_once_after_scenario_validation() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pack = StubValidationPack {
            calls: Arc::clone(&calls),
            reject: true,
        };
        let mut scenario = scenario_with_log("unused_{name}_{seed}.jsonl");
        scenario.simulation.module_pack = "stub-validation".to_owned();

        let error = check_scenario(&pack, &scenario, Path::new("pack.toml")).unwrap_err();

        assert_eq!(
            *calls.lock().unwrap(),
            vec!["mechanism_names", "pack_validate"]
        );
        let message = format!("{error:#}");
        assert!(message.contains("validation failed for 'pack.toml'"));
        assert!(message.contains("pack validation rejected the scenario"));
    }
}
