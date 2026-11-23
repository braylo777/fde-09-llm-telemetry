//! Silent-failure detection: problems that dashboards of HTTP status codes do not show.
//! Compares a recent window against a baseline window, per feature.

use crate::{calls, r2, Call};
use serde_json::{json, Value};

fn rate(cs: &[&Call], f: impl Fn(&Call) -> bool) -> f64 {
    if cs.is_empty() {
        0.0
    } else {
        cs.iter().filter(|c| f(c)).count() as f64 / cs.len() as f64
    }
}

fn p95(cs: &[&Call]) -> u64 {
    let mut v: Vec<u64> = cs.iter().map(|c| c.latency_ms).collect();
    v.sort();
    v.get(((v.len() as f64 * 0.95).ceil() as usize).saturating_sub(1))
        .copied()
        .unwrap_or(0)
}

pub fn detect_op(i: &Value) -> Result<Value, String> {
    let cs = calls(i)?;
    let split = i["window_start_ts"]
        .as_u64()
        .ok_or("missing `window_start_ts`: calls at or after it are the recent window")?;
    let mut features: Vec<String> = cs.iter().map(|c| c.feature.clone()).collect();
    features.sort();
    features.dedup();
    let mut alerts = Vec::new();
    for f in &features {
        let base: Vec<&Call> = cs
            .iter()
            .filter(|c| &c.feature == f && c.ts < split)
            .collect();
        let now: Vec<&Call> = cs
            .iter()
            .filter(|c| &c.feature == f && c.ts >= split)
            .collect();
        let mut alert = |sev: &str, kind: &str, msg: String, b: f64, n: f64| {
            alerts.push(json!({"feature": f, "severity": sev, "signal": kind, "message": msg, "baseline": r2(b), "recent": r2(n)}));
        };
        if !base.is_empty() && now.is_empty() {
            alert(
                "high",
                "traffic_stopped",
                format!(
                    "{} calls before, 0 in window: integration may be broken upstream",
                    base.len()
                ),
                base.len() as f64,
                0.0,
            );
            continue;
        }
        if now.len() < 5 {
            continue; // too little data to judge
        }
        let checks: [(&str, &str, Box<dyn Fn(&Call) -> bool>, f64, f64); 4] = [
            (
                "empty_output",
                "empty or near-empty answers returned as success",
                Box::new(|c: &Call| c.status == "ok" && c.output_chars < 5),
                0.05,
                2.0,
            ),
            (
                "refusals",
                "refusal rate jumped (prompt or policy change?)",
                Box::new(|c: &Call| c.refusal),
                0.10,
                2.0,
            ),
            (
                "truncation",
                "answers cut off at max_tokens (finish_reason=length)",
                Box::new(|c: &Call| c.finish_reason == "length"),
                0.05,
                2.0,
            ),
            (
                "invalid_json",
                "structured output failing to parse",
                Box::new(|c: &Call| c.json_valid == Some(false)),
                0.05,
                2.0,
            ),
        ];
        for (kind, msg, pred, abs_floor, mult) in checks.iter() {
            let (b, n) = (rate(&base, pred), rate(&now, pred));
            if n >= *abs_floor && n >= b * mult + 0.02 {
                alert(
                    if n >= 0.25 { "high" } else { "medium" },
                    kind,
                    format!("{msg}: {:.0}% vs {:.0}% before", n * 100.0, b * 100.0),
                    b * 100.0,
                    n * 100.0,
                );
            }
        }
        let (pb, pn) = (p95(&base), p95(&now));
        if pb > 0 && pn as f64 > pb as f64 * 1.5 {
            alert(
                "medium",
                "latency_p95",
                format!("p95 latency {pn} ms vs {pb} ms before"),
                pb as f64,
                pn as f64,
            );
        }
        let avg = |v: &[&Call]| {
            v.iter().map(|c| c.output_tokens as f64).sum::<f64>() / v.len().max(1) as f64
        };
        let (ob, on) = (avg(&base), avg(&now));
        if ob > 0.0 && (on < ob * 0.5 || on > ob * 2.0) {
            alert(
                "medium",
                "output_length_drift",
                format!("avg output {on:.0} tokens vs {ob:.0} before (model or prompt changed?)"),
                ob,
                on,
            );
        }
        let mods = |v: &[&Call]| {
            let mut m: Vec<&str> = v.iter().map(|c| c.model.as_str()).collect();
            m.sort();
            m.dedup();
            m.join(",")
        };
        if !base.is_empty() && mods(&base) != mods(&now) {
            alerts.push(json!({"feature": f, "severity": "info", "signal": "model_changed", "message": format!("model set changed: {} -> {}", mods(&base), mods(&now))}));
        }
    }
    let high = alerts.iter().filter(|a| a["severity"] == "high").count();
    Ok(
        json!({"healthy": alerts.iter().all(|a| a["severity"] == "info"), "high": high, "alerts": alerts, "features_checked": features.len()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(id: u64, ts: u64, f: &str, extra: Value) -> Value {
        let mut v = json!({"id": id.to_string(), "ts": ts, "model": "m", "feature": f, "input_tokens": 100, "output_tokens": 200, "output_chars": 800, "latency_ms": 900});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }
    #[test]
    fn finds_empty_outputs_and_dead_feature() {
        let mut calls = vec![];
        for k in 0..20 {
            calls.push(c(k, 100, "summarize", json!({})));
            calls.push(c(100 + k, 100, "classify", json!({})));
        }
        for k in 0..10 {
            calls.push(c(
                200 + k,
                2000,
                "summarize",
                if k < 4 {
                    json!({"output_chars": 0, "output_tokens": 0})
                } else {
                    json!({})
                },
            ));
        }
        let r = detect_op(&json!({"calls": calls, "window_start_ts": 1000})).unwrap();
        let sig: Vec<&str> = r["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["signal"].as_str().unwrap())
            .collect();
        assert!(sig.contains(&"traffic_stopped"));
        assert!(sig.contains(&"empty_output"));
        assert_eq!(r["healthy"], false);
    }
    #[test]
    fn steady_traffic_is_healthy() {
        let calls: Vec<Value> = (0..40)
            .map(|k| c(k, if k < 20 { 100 } else { 2000 }, "a", json!({})))
            .collect();
        assert_eq!(
            detect_op(&json!({"calls": calls, "window_start_ts": 1000})).unwrap()["healthy"],
            true
        );
    }
}
