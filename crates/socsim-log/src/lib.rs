//! [`Recorder`] implementations for `socsim`.
//!
//! Two implementations are provided:
//!
//! - [`InMemoryRecorder`] — stores all metrics and events in `Vec`s; suitable
//!   for unit and integration tests.
//! - [`JsonlRecorder`] — writes one JSON object per line to any [`Write`]
//!   sink; suitable for production runs.

use socsim_core::Recorder;
use std::io::{self, Write};

// ── Shared row types ─────────────────────────────────────────────────────────

/// A recorded scalar metric row.
#[derive(Debug, Clone)]
pub struct MetricRow {
    /// Simulation time step.
    pub t: u64,
    /// Metric key.
    pub key: String,
    /// Metric value.
    pub value: f64,
}

/// A recorded event row.
#[derive(Debug, Clone)]
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
}
