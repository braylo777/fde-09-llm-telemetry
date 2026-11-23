//! Cost per call from a price table, attributed to department, feature and user.
//! Prices are USD per million tokens: {"model": {"input": x, "output": y, "cached_input": z}}.

use crate::{calls, r2, r4, Call};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn call_cost(c: &Call, prices: &Value) -> Option<f64> {
    let p = prices.get(&c.model)?;
    let input = p["input"].as_f64()?;
    let output = p["output"].as_f64()?;
    let cached = p["cached_input"].as_f64().unwrap_or(input);
    let fresh = c.input_tokens.saturating_sub(c.cached_tokens) as f64;
    Some((fresh * input + c.cached_tokens as f64 * cached + c.output_tokens as f64 * output) / 1e6)
}

pub fn report_op(i: &Value) -> Result<Value, String> {
    let cs = calls(i)?;
    let prices = i.get("prices").ok_or("missing `prices`")?;
    let mut total = 0.0;
    let mut unpriced: Vec<String> = vec![];
    let mut by: BTreeMap<&str, BTreeMap<String, (f64, u64, u64)>> = BTreeMap::new();
    for c in &cs {
        let Some(cost) = call_cost(c, prices) else {
            if !unpriced.contains(&c.model) {
                unpriced.push(c.model.clone());
            }
            continue;
        };
        total += cost;
        for (dim, key) in [
            ("department", &c.dept),
            ("feature", &c.feature),
            ("model", &c.model),
            ("user", &c.user),
        ] {
            let e = by
                .entry(dim)
                .or_default()
                .entry(if key.is_empty() {
                    "(untagged)".into()
                } else {
                    key.clone()
                })
                .or_default();
            e.0 += cost;
            e.1 += 1;
            e.2 += c.input_tokens + c.output_tokens;
        }
    }
    let mut out = Map::new();
    for (dim, m) in by {
        let mut rows: Vec<(String, (f64, u64, u64))> = m.into_iter().collect();
        rows.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap());
        let take = if dim == "user" { 5 } else { rows.len() };
        out.insert(
            dim.to_string(),
            json!(rows.iter().take(take).map(|(k, (c, n, t))| json!({"name": k, "cost_usd": r4(*c), "share": r2(c / total.max(1e-12) * 100.0),
                "calls": n, "tokens": t, "cost_per_call": r4(c / *n as f64)})).collect::<Vec<_>>()),
        );
    }
    let cache_saving: f64 = cs
        .iter()
        .filter_map(|c| {
            let p = prices.get(&c.model)?;
            Some(
                c.cached_tokens as f64
                    * (p["input"].as_f64()?
                        - p["cached_input"].as_f64().unwrap_or(p["input"].as_f64()?))
                    / 1e6,
            )
        })
        .sum();
    Ok(
        json!({"total_usd": r4(total), "calls": cs.len(), "prompt_cache_saved_usd": r4(cache_saving), "by": out,
        "unpriced_models": unpriced, "warning": if unpriced.is_empty() { Value::Null } else { json!("some calls have no price and are excluded; add them to `prices`") }}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_tokens_priced_lower() {
        let c: Call = serde_json::from_value(json!({"id": "1", "ts": 0, "model": "m", "input_tokens": 1_000_000, "output_tokens": 100_000, "cached_tokens": 800_000})).unwrap();
        let p = json!({"m": {"input": 3.0, "output": 15.0, "cached_input": 0.3}});
        // 200k fresh * 3 + 800k * 0.3 + 100k * 15 = 0.6 + 0.24 + 1.5
        assert!((call_cost(&c, &p).unwrap() - 2.34).abs() < 1e-9);
    }
    #[test]
    fn attribution_shares_sum_to_100() {
        let r = report_op(&json!({"prices": {"m": {"input": 1.0, "output": 2.0}}, "calls": [
            {"id": "1", "ts": 0, "model": "m", "dept": "a", "input_tokens": 1000, "output_tokens": 0},
            {"id": "2", "ts": 0, "model": "m", "dept": "b", "input_tokens": 3000, "output_tokens": 0},
            {"id": "3", "ts": 0, "model": "x", "dept": "b", "input_tokens": 3000, "output_tokens": 0}]})).unwrap();
        let s: f64 = r["by"]["department"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["share"].as_f64().unwrap())
            .sum();
        assert!((s - 100.0).abs() < 0.02);
        assert_eq!(r["unpriced_models"], json!(["x"]));
    }
}
