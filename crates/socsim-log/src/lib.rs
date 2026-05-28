//! [`Recorder`] implementations for `socsim`.
//!
//! Three implementations are provided:
//!
//! - [`InMemoryRecorder`] — stores all metrics and events in `Vec`s; suitable
//!   for unit and integration tests.
//! - [`JsonlRecorder`] — writes one JSON object per line to any [`Write`]
//!   sink; suitable for production runs.
//! - [`CsvRecorder`] — accumulates *wide rows* (via [`Recorder::record_row`])
//!   grouped by table and renders each as column-aligned CSV; the natural sink
//!   for `metrics.csv`-style tabular output consumed by pandas/Excel.

use serde::{Deserialize, Serialize};
use socsim_core::Recorder;
use std::collections::BTreeMap;
use std::io::{self, Write};

// ── Shared row types ─────────────────────────────────────────────────────────

/// A recorded scalar metric row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRow {
    /// Simulation time step.
    pub t: u64,
    /// Metric key.
    pub key: String,
    /// Metric value.
    pub value: f64,
}

/// A recorded event row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    /// Simulation time step.
    pub t: u64,
    /// Event kind tag.
    pub kind: String,
    /// Arbitrary JSON payload.
    pub payload: serde_json::Value,
}

// ── InMemoryRecorder ─────────────────────────────────────────────────────────

/// An in-memory [`Recorder`] that collects metrics and events into `Vec`s.
///
/// Intended for tests: after the simulation completes you can inspect
/// [`InMemoryRecorder::metrics`] and [`InMemoryRecorder::events`] directly.
#[derive(Default, Debug)]
pub struct InMemoryRecorder {
    metrics: Vec<MetricRow>,
    events: Vec<EventRow>,
}

impl InMemoryRecorder {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// All recorded metric rows in insertion order.
    pub fn metrics(&self) -> &[MetricRow] {
        &self.metrics
    }

    /// All recorded event rows in insertion order.
    pub fn events(&self) -> &[EventRow] {
        &self.events
    }
}

impl Recorder for InMemoryRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.metrics.push(MetricRow {
            t,
            key: key.to_owned(),
            value,
        });
    }

    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        self.events.push(EventRow {
            t,
            kind: kind.to_owned(),
            payload,
        });
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// ── JsonlRecorder ─────────────────────────────────────────────────────────────

/// A [`Recorder`] that writes one JSON object per line to a [`Write`] sink.
///
/// Each metric produces a line of the form:
/// ```json
/// {"type":"metric","t":1,"key":"value","value":42.0}
/// ```
/// Each event produces:
/// ```json
/// {"type":"event","t":1,"kind":"agent_hired","payload":{...}}
/// ```
///
/// Write errors are silently ignored to keep the `Recorder` trait infallible;
/// use [`JsonlRecorder::take_error`] after the run to inspect any accumulated
/// error.
pub struct JsonlRecorder<W: Write> {
    sink: W,
    last_error: Option<io::Error>,
}

impl<W: Write> JsonlRecorder<W> {
    /// Create a new recorder writing to `sink`.
    pub fn new(sink: W) -> Self {
        Self {
            sink,
            last_error: None,
        }
    }

    /// Take and return any I/O error that occurred during recording.
    pub fn take_error(&mut self) -> Option<io::Error> {
        self.last_error.take()
    }

    fn write_line(&mut self, obj: serde_json::Value) {
        match serde_json::to_string(&obj) {
            Ok(mut line) => {
                line.push('\n');
                if let Err(e) = self.sink.write_all(line.as_bytes()) {
                    self.last_error = Some(e);
                }
            }
            Err(e) => {
                self.last_error = Some(io::Error::other(e));
            }
        }
    }
}

impl<W: Write> Recorder for JsonlRecorder<W> {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        let obj = serde_json::json!({
            "type": "metric",
            "t": t,
            "key": key,
            "value": value,
        });
        self.write_line(obj);
    }

    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        let obj = serde_json::json!({
            "type": "event",
            "t": t,
            "kind": kind,
            "payload": payload,
        });
        self.write_line(obj);
    }
}

// ── CsvRecorder ────────────────────────────────────────────────────────────────

/// A wide table accumulated by [`CsvRecorder`].
///
/// The column schema is fixed by the first row recorded for the table;
/// subsequent rows are aligned to it by column name (missing columns become
/// `NaN`, unknown columns are ignored).
#[derive(Debug, Clone, Default)]
struct WideTable {
    columns: Vec<String>,
    rows: Vec<(u64, Vec<f64>)>,
}

/// A [`Recorder`] that collects [`record_row`](Recorder::record_row) calls into
/// per-table wide CSV, plus scalar metrics in long (`t,key,value`) form.
///
/// Nothing is written to disk implicitly; render with [`CsvRecorder::table_csv`]
/// / [`CsvRecorder::metrics_csv`] and write the strings wherever you like.
///
/// # Example
/// ```
/// use socsim_core::Recorder;
/// use socsim_log::CsvRecorder;
///
/// let mut rec = CsvRecorder::new();
/// rec.record_row(0, "metrics", &[("avg", 0.5), ("moved", 3.0)]);
/// rec.record_row(1, "metrics", &[("avg", 0.7), ("moved", 1.0)]);
/// let csv = rec.table_csv("metrics").unwrap();
/// assert_eq!(csv.lines().next().unwrap(), "t,avg,moved");
/// ```
#[derive(Debug, Default)]
pub struct CsvRecorder {
    tables: BTreeMap<String, WideTable>,
    /// Caller-pinned column schemas, keyed by table name.  When present for a
    /// table, [`CsvRecorder::table_csv`] uses exactly these columns in this
    /// order instead of the auto-discovered schema.
    schemas: BTreeMap<String, Vec<String>>,
    metrics: Vec<MetricRow>,
    events: Vec<EventRow>,
}

impl CsvRecorder {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Names of all tables recorded via [`record_row`](Recorder::record_row),
    /// in sorted order.
    pub fn tables(&self) -> Vec<&str> {
        self.tables.keys().map(String::as_str).collect()
    }

    /// Pin the column order (and the exact set of columns) for `table`'s CSV
    /// output.
    ///
    /// After this call, [`CsvRecorder::table_csv`] for `table` emits the header
    /// `t,<columns...>` in exactly the order given here, regardless of the order
    /// in which columns were first observed in the data.  Columns not listed in
    /// the schema are omitted from the output; schema columns absent from a
    /// recorded row render as empty fields.
    ///
    /// Calling this does not affect which rows are stored — only how they are
    /// rendered.  When no schema is set for a table, [`CsvRecorder::table_csv`]
    /// keeps its default auto-discovered behaviour.
    pub fn set_columns(&mut self, table: &str, columns: &[&str]) {
        self.schemas.insert(
            table.to_owned(),
            columns.iter().map(|c| (*c).to_owned()).collect(),
        );
    }

    /// Render `table` as CSV with header `t,<col1>,<col2>,...`, or `None` if no
    /// rows were recorded for it.
    ///
    /// If a column schema was pinned via [`CsvRecorder::set_columns`], the
    /// header uses exactly those columns in that order, omitting any columns not
    /// in the schema and rendering schema columns missing from a row as empty
    /// fields.  Otherwise the columns are those auto-discovered from the data.
    pub fn table_csv(&self, table: &str) -> Option<String> {
        let t = self.tables.get(table)?;
        match self.schemas.get(table) {
            Some(schema) => Some(self.table_csv_with_schema(t, schema)),
            None => Some(Self::table_csv_auto(t)),
        }
    }

    /// Auto-discovered rendering: columns in the order they were first observed.
    fn table_csv_auto(t: &WideTable) -> String {
        let mut out = String::new();
        out.push('t');
        for col in &t.columns {
            out.push(',');
            out.push_str(col);
        }
        out.push('\n');
        for (time, values) in &t.rows {
            out.push_str(&time.to_string());
            for v in values {
                out.push(',');
                out.push_str(&fmt_f64(*v));
            }
            out.push('\n');
        }
        out
    }

    /// Schema-pinned rendering: columns exactly as given, missing values empty.
    fn table_csv_with_schema(&self, t: &WideTable, schema: &[String]) -> String {
        // Map each schema column to its index in the stored (auto) column
        // order, or `None` if the table never saw that column.
        let indices: Vec<Option<usize>> = schema
            .iter()
            .map(|col| t.columns.iter().position(|c| c == col))
            .collect();

        let mut out = String::new();
        out.push('t');
        for col in schema {
            out.push(',');
            out.push_str(col);
        }
        out.push('\n');
        for (time, values) in &t.rows {
            out.push_str(&time.to_string());
            for idx in &indices {
                out.push(',');
                match idx.and_then(|i| values.get(i)) {
                    Some(v) if v.is_nan() => {} // missing value → empty field
                    Some(v) => out.push_str(&fmt_f64(*v)),
                    None => {} // column absent from this table → empty field
                }
            }
            out.push('\n');
        }
        out
    }

    /// Render all scalar metrics (recorded via
    /// [`record_metric`](Recorder::record_metric)) in long format with header
    /// `t,key,value`.
    pub fn metrics_csv(&self) -> String {
        let mut out = String::from("t,key,value\n");
        for m in &self.metrics {
            out.push_str(&format!("{},{},{}\n", m.t, m.key, fmt_f64(m.value)));
        }
        out
    }

    /// All recorded event rows in insertion order.
    pub fn events(&self) -> &[EventRow] {
        &self.events
    }
}

/// Format an `f64` for CSV without a trailing `.0` surprise: integers print as
/// integers, everything else uses the default float formatting.
fn fmt_f64(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl Recorder for CsvRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.metrics.push(MetricRow {
            t,
            key: key.to_owned(),
            value,
        });
    }

    fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
        self.events.push(EventRow {
            t,
            kind: kind.to_owned(),
            payload,
        });
    }

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        let entry = self.tables.entry(table.to_owned()).or_default();
        if entry.columns.is_empty() {
            // First row fixes the column schema.
            entry.columns = row.iter().map(|(c, _)| (*c).to_owned()).collect();
        }
        // Align the incoming row to the established column order by name.
        let values: Vec<f64> = entry
            .columns
            .iter()
            .map(|col| {
                row.iter()
                    .find(|(c, _)| c == col)
                    .map(|(_, v)| *v)
                    .unwrap_or(f64::NAN)
            })
            .collect();
        entry.rows.push((t, values));
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_records_metrics() {
        let mut rec = InMemoryRecorder::new();
        rec.record_metric(1, "score", 2.71);
        assert_eq!(rec.metrics().len(), 1);
        assert_eq!(rec.metrics()[0].key, "score");
        assert!((rec.metrics()[0].value - 2.71).abs() < 1e-9);
    }

    #[test]
    fn in_memory_records_events() {
        let mut rec = InMemoryRecorder::new();
        rec.record_event(2, "hired", serde_json::json!({"agent": 7}));
        assert_eq!(rec.events().len(), 1);
        assert_eq!(rec.events()[0].kind, "hired");
    }

    #[test]
    fn jsonl_recorder_writes_valid_jsonl() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut rec = JsonlRecorder::new(&mut buf);
            rec.record_metric(0, "x", 1.0);
            rec.record_event(1, "tick", serde_json::json!({}));
            assert!(rec.take_error().is_none());
        }
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        // Each line must parse as JSON.
        for line in &lines {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }

    #[test]
    fn csv_recorder_renders_wide_rows() {
        let mut rec = CsvRecorder::new();
        rec.record_row(0, "metrics", &[("avg", 0.5), ("moved", 3.0)]);
        rec.record_row(1, "metrics", &[("avg", 0.75), ("moved", 1.0)]);
        let csv = rec.table_csv("metrics").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,avg,moved");
        assert_eq!(lines[1], "0,0.5,3");
        assert_eq!(lines[2], "1,0.75,1");
        assert_eq!(rec.tables(), vec!["metrics"]);
        assert!(rec.table_csv("missing").is_none());
    }

    #[test]
    fn csv_recorder_aligns_columns_by_name_and_fills_missing() {
        let mut rec = CsvRecorder::new();
        rec.record_row(0, "m", &[("a", 1.0), ("b", 2.0)]);
        // Reordered columns + a missing one → aligned to first-row schema.
        rec.record_row(1, "m", &[("b", 9.0)]);
        let csv = rec.table_csv("m").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,a,b");
        assert_eq!(lines[1], "0,1,2");
        // a missing → NaN, b=9
        assert!(lines[2].starts_with("1,NaN,9"));
    }

    #[test]
    fn csv_recorder_metrics_long_format() {
        let mut rec = CsvRecorder::new();
        rec.record_metric(0, "score", 1.5);
        rec.record_metric(1, "score", 2.0);
        let csv = rec.metrics_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,key,value");
        assert_eq!(lines[1], "0,score,1.5");
        assert_eq!(lines[2], "1,score,2");
    }

    #[test]
    fn csv_recorder_set_columns_pins_header_order_and_omits_extras() {
        let mut rec = CsvRecorder::new();
        // Data observed in this order: moved, avg, extra.
        rec.record_row(0, "metrics", &[("moved", 3.0), ("avg", 0.5), ("extra", 9.0)]);
        rec.record_row(1, "metrics", &[("moved", 1.0), ("avg", 0.75), ("extra", 8.0)]);
        // Pin a different order and drop "extra".
        rec.set_columns("metrics", &["avg", "moved"]);
        let csv = rec.table_csv("metrics").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,avg,moved");
        assert_eq!(lines[1], "0,0.5,3");
        assert_eq!(lines[2], "1,0.75,1");
    }

    #[test]
    fn csv_recorder_set_columns_renders_missing_as_empty() {
        let mut rec = CsvRecorder::new();
        // "b" is never recorded for this table; "a" is.
        rec.record_row(0, "m", &[("a", 1.0)]);
        rec.record_row(1, "m", &[("a", 2.0)]);
        // Schema requests a, b, c — only "a" exists in the data.
        rec.set_columns("m", &["a", "b", "c"]);
        let csv = rec.table_csv("m").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,a,b,c");
        // b and c absent → empty fields.
        assert_eq!(lines[1], "0,1,,");
        assert_eq!(lines[2], "1,2,,");
    }

    #[test]
    fn csv_recorder_set_columns_missing_value_in_some_rows_is_empty() {
        let mut rec = CsvRecorder::new();
        // Establish schema with both columns on the first row, then omit "b".
        rec.record_row(0, "m", &[("a", 1.0), ("b", 2.0)]);
        rec.record_row(1, "m", &[("a", 5.0)]); // b missing → stored as NaN
        rec.set_columns("m", &["a", "b"]);
        let csv = rec.table_csv("m").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,a,b");
        assert_eq!(lines[1], "0,1,2");
        // b missing in row 1 → empty field (not "NaN").
        assert_eq!(lines[2], "1,5,");
    }

    #[test]
    fn csv_recorder_auto_discovery_unchanged_without_schema() {
        // No set_columns call → behaviour identical to the default.
        let mut rec = CsvRecorder::new();
        rec.record_row(0, "metrics", &[("avg", 0.5), ("moved", 3.0)]);
        rec.record_row(1, "metrics", &[("avg", 0.75), ("moved", 1.0)]);
        let csv = rec.table_csv("metrics").unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "t,avg,moved");
        assert_eq!(lines[1], "0,0.5,3");
        assert_eq!(lines[2], "1,0.75,1");
    }

    #[test]
    fn default_record_row_fans_out_to_metrics() {
        // InMemoryRecorder does not override record_row, so the trait default
        // should fan out to record_metric with "table.col" keys.
        let mut rec = InMemoryRecorder::new();
        rec.record_row(3, "metrics", &[("avg", 0.5), ("moved", 2.0)]);
        let keys: Vec<&str> = rec.metrics().iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, vec!["metrics.avg", "metrics.moved"]);
        assert_eq!(rec.metrics()[0].t, 3);
    }
}
