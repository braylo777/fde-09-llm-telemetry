//! Stage 9 — Telemetry, ROI & Customer Health.
//!
//! Input everywhere is a list of LLM call records (what a gateway or SDK wrapper logs):
//! `{id, ts, model, dept, user, feature, input_tokens, output_tokens, cached_tokens,
//!   latency_ms, finish_reason, status, output_chars, refusal, json_valid, accepted}`.
//!
//! * `otel`    — map records to OTLP/JSON spans using the OpenTelemetry GenAI conventions.
//! * `cost`    — price each call and attribute spend to department / feature / user.
//! * `roi`     — time saved and value per feature from baselines and acceptance rates.
//! * `health`  — silent-failure detectors: errors that return HTTP 200.
//! * `billing` — usage meter with idempotent events, included quota and tiered prices.

mod api;
pub mod billing;
pub mod cost;
pub mod health;
pub mod otel;
pub mod roi;

pub use api::dispatch;
use serde::Deserialize;
use serde_json::Value;

pub const OPS: &[&str] = &["to_otel", "cost_report", "roi_report", "detect_silent_failures", "billing_meter"];

#[derive(Debug, Clone, Deserialize)]
pub struct Call {
    pub id: String,
    pub ts: u64, // unix ms
    pub model: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub dept: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub feature: String,
    #[serde(default)]
    pub customer: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default = "stop")]
    pub finish_reason: String,
    #[serde(default = "okv")]
    pub status: String,
    #[serde(default)]
    pub output_chars: u64,
    #[serde(default)]
    pub refusal: bool,
    #[serde(default)]
    pub json_valid: Option<bool>,
    #[serde(default)]
    pub accepted: Option<bool>,
}
fn stop() -> String {
    "stop".into()
}
fn okv() -> String {
    "ok".into()
}

pub(crate) fn calls(i: &Value) -> Result<Vec<Call>, String> {
    serde_json::from_value(i.get("calls").cloned().ok_or("missing `calls`")?).map_err(|e| format!("calls: {e}"))
}

pub(crate) fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
pub(crate) fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

pub fn handle(op: &str, i: &Value) -> Result<Value, String> {
    match op {
        "to_otel" => otel::export_op(i),
        "cost_report" => cost::report_op(i),
        "roi_report" => roi::report_op(i),
        "detect_silent_failures" => health::detect_op(i),
        "billing_meter" => billing::meter_op(i),
        _ => Err(format!("unknown op `{op}`; known: {}", OPS.join(", "))),
    }
}
