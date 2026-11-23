//! Time saved and ROI per feature.
//!
//! For each feature: baseline = minutes a person needed before; review = minutes a person
//! still spends checking the AI output. Only accepted outputs count as saved work; rejected
//! outputs cost review time and save nothing.
//! saved_min = accepted * (baseline - review) - rejected * review

use crate::{calls, cost::call_cost, r2};
use serde_json::{json, Value};

pub fn report_op(i: &Value) -> Result<Value, String> {
    let cs = calls(i)?;
    let prices = i.get("prices").ok_or("missing `prices`")?;
    let rate = i["loaded_hourly_rate_usd"].as_f64().unwrap_or(60.0);
    let baselines = i["baselines"]
        .as_object()
        .ok_or("missing `baselines` {feature: {baseline_min, review_min}}")?;
    let mut rows = Vec::new();
    let (mut tv, mut tc) = (0.0, 0.0);
    for (feature, b) in baselines {
        let base = b["baseline_min"]
            .as_f64()
            .ok_or(format!("{feature}: baseline_min"))?;
        let review = b["review_min"].as_f64().unwrap_or(0.0);
        let fc: Vec<_> = cs
            .iter()
            .filter(|c| &c.feature == feature && c.status == "ok")
            .collect();
        let accepted = fc.iter().filter(|c| c.accepted == Some(true)).count() as f64;
        let rejected = fc.iter().filter(|c| c.accepted == Some(false)).count() as f64;
        let unknown = fc.len() as f64 - accepted - rejected;
        let saved_min = accepted * (base - review) - rejected * review;
        let value = saved_min / 60.0 * rate;
        let cost: f64 = cs
            .iter()
            .filter(|c| &c.feature == feature)
            .filter_map(|c| call_cost(c, prices))
            .sum();
        tv += value;
        tc += cost;
        rows.push(json!({
            "feature": feature, "tasks": fc.len(), "accepted": accepted, "rejected": rejected, "no_feedback": unknown,
            "acceptance_rate": if accepted + rejected > 0.0 { r2(accepted / (accepted + rejected) * 100.0) } else { 0.0 },
            "hours_saved": r2(saved_min / 60.0), "value_usd": r2(value), "llm_cost_usd": r2(cost),
            "roi_multiple": if cost > 0.0 { r2(value / cost) } else { 0.0 },
            "verdict": if saved_min < 0.0 { "losing time: review costs more than it saves" } else if unknown > accepted + rejected { "not enough feedback to trust this number" } else { "saving time" }
        }));
    }
    Ok(
        json!({"features": rows, "total_value_usd": r2(tv), "total_llm_cost_usd": r2(tc), "roi_multiple": if tc > 0.0 { r2(tv / tc) } else { 0.0 },
        "assumptions": {"loaded_hourly_rate_usd": rate, "only_accepted_outputs_count": true}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_heavy_feature_loses_time() {
        let r = report_op(&json!({"prices": {"m": {"input": 1.0, "output": 1.0}}, "loaded_hourly_rate_usd": 60,
            "baselines": {"a": {"baseline_min": 10, "review_min": 2}, "b": {"baseline_min": 3, "review_min": 4}},
            "calls": [
                {"id": "1", "ts": 0, "model": "m", "feature": "a", "input_tokens": 1, "output_tokens": 1, "accepted": true},
                {"id": "2", "ts": 0, "model": "m", "feature": "a", "input_tokens": 1, "output_tokens": 1, "accepted": false},
                {"id": "3", "ts": 0, "model": "m", "feature": "b", "input_tokens": 1, "output_tokens": 1, "accepted": true}]})).unwrap();
        let a = &r["features"][0];
        assert_eq!(a["hours_saved"], 0.1); // (10-2) - 2 = 6 min
        assert!(r["features"][1]["verdict"]
            .as_str()
            .unwrap()
            .starts_with("losing"));
    }
}
