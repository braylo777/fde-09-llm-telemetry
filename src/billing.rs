//! Usage-based billing meter.
//!
//! * Idempotent: an event id is counted once, however often the gateway retries the report.
//! * Unit: 1k tokens (input + output), rounded up per customer per period.
//! * Plan: included units, then graduated tiers: [{"up_to": 1000, "usd_per_unit": 0.01}, {"up_to": null, ...}].
//! * Money in integer micro-dollars to avoid float drift on invoices.

use crate::calls;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

fn usd(micro: u64) -> String {
    let cents = (micro + 5_000) / 10_000; // round half up to the cent, carry into dollars
    format!("{}.{:02}", cents / 100, cents % 100)
}

pub fn price(units: u64, included: u64, tiers: &[Value]) -> Result<(u64, Vec<Value>), String> {
    let mut billable = units.saturating_sub(included);
    let mut lines = vec![];
    let mut floor = 0u64;
    let mut total = 0u64;
    for t in tiers {
        if billable == 0 {
            break;
        }
        let cap = t["up_to"].as_u64();
        let width = cap.map(|c| c.saturating_sub(floor)).unwrap_or(u64::MAX);
        let n = billable.min(width);
        let price = t["usd_per_unit"].as_f64().ok_or("tier needs usd_per_unit")?;
        if !price.is_finite() || !(0.0..=1000.0).contains(&price) {
            return Err(format!("usd_per_unit {price} must be between 0 and 1000"));
        }
        let unit_micro = (price * 1e6).round() as u64;
        let amt = n.checked_mul(unit_micro).ok_or("invoice amount overflows; check units and prices")?;
        lines.push(json!({"units": n, "unit_price": t["usd_per_unit"], "amount_usd": usd(amt)}));
        total = total.checked_add(amt).ok_or("invoice total overflows")?;
        billable -= n;
        floor = cap.unwrap_or(floor);
    }
    if billable > 0 {
        return Err("tiers do not cover all usage: last tier needs up_to: null".into());
    }
    Ok((total, lines))
}

pub fn meter_op(i: &Value) -> Result<Value, String> {
    let cs = calls(i)?;
    let plan = i.get("plan").ok_or("missing `plan`")?;
    let included = plan["included_units"].as_u64().unwrap_or(0);
    let tiers = plan["tiers"].as_array().ok_or("plan.tiers required")?;
    let mut seen = HashSet::new();
    let mut dupes = 0;
    let mut tokens: BTreeMap<String, u64> = BTreeMap::new();
    for c in &cs {
        if !seen.insert(c.id.clone()) {
            dupes += 1;
            continue;
        }
        if c.status != "ok" {
            continue; // failed calls are not billed
        }
        *tokens
            .entry(if c.customer.is_empty() {
                "(none)".into()
            } else {
                c.customer.clone()
            })
            .or_default();
        let e = tokens.get_mut(if c.customer.is_empty() { "(none)" } else { c.customer.as_str() }).unwrap();
        *e = e
            .checked_add(c.input_tokens)
            .and_then(|x| x.checked_add(c.output_tokens))
            .ok_or(format!("token count overflow for event {}", c.id))?;
    }
    let mut invoices = vec![];
    for (cust, t) in tokens {
        let units = t.div_ceil(1000);
        let (total, lines) = price(units, included, tiers)?;
        invoices.push(json!({"customer": cust, "tokens": t, "units_1k": units, "included_units": included, "lines": lines, "total_usd": usd(total)}));
    }
    Ok(
        json!({"invoices": invoices, "duplicate_events_ignored": dupes, "failed_calls_not_billed": cs.iter().filter(|c| c.status != "ok").count()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graduated_tiers() {
        let tiers = vec![
            json!({"up_to": 100, "usd_per_unit": 0.02}),
            json!({"up_to": null, "usd_per_unit": 0.01}),
        ];
        let (t, lines) = price(250, 50, &tiers).unwrap();
        // 200 billable: 100 * 0.02 + 100 * 0.01 = 3.00
        assert_eq!(usd(t), "3.00");
        assert_eq!(lines.len(), 2);
        assert_eq!(usd(999_999), "1.00");
        assert_eq!(usd(1_234_567), "1.23");
    }
    #[test]
    fn duplicates_and_failures_not_billed() {
        let r = meter_op(&json!({"plan": {"included_units": 0, "tiers": [{"up_to": null, "usd_per_unit": 1.0}]}, "calls": [
            {"id": "a", "ts": 0, "model": "m", "customer": "x", "input_tokens": 1500, "output_tokens": 0},
            {"id": "a", "ts": 0, "model": "m", "customer": "x", "input_tokens": 1500, "output_tokens": 0},
            {"id": "b", "ts": 0, "model": "m", "customer": "x", "input_tokens": 9000, "output_tokens": 0, "status": "error"}]})).unwrap();
        assert_eq!(r["invoices"][0]["units_1k"], 2);
        assert_eq!(r["duplicate_events_ignored"], 1);
    }
    #[test]
    fn bad_prices_and_overflow_are_errors() {
        assert!(price(10, 0, &[json!({"up_to": null, "usd_per_unit": -1.0})]).is_err());
        assert!(price(5_000_000, 0, &[json!({"up_to": null, "usd_per_unit": 1e15})]).is_err());
        let r = meter_op(&json!({"plan": {"tiers": [{"up_to": null, "usd_per_unit": 1.0}]}, "calls": [
            {"id": "a", "ts": 0, "model": "m", "input_tokens": u64::MAX, "output_tokens": 1}]}));
        assert!(r.is_err());
    }
    #[test]
    fn open_ended_tier_required() {
        assert!(price(500, 0, &[json!({"up_to": 100, "usd_per_unit": 1.0})]).is_err());
    }
}
