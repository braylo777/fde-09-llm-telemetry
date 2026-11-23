//! OTLP/JSON export with OpenTelemetry GenAI semantic conventions (gen_ai.*).
//! Trace and span ids are derived from the call id, so re-exporting is idempotent.

use crate::{calls, Call};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn hex_id(seed: &str, bytes: usize) -> String {
    Sha256::digest(seed.as_bytes())
        .iter()
        .take(bytes)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn attr(k: &str, v: Value) -> Value {
    let val = match &v {
        Value::String(s) => json!({"stringValue": s}),
        Value::Bool(b) => json!({"boolValue": b}),
        Value::Number(n) if n.is_u64() || n.is_i64() => json!({"intValue": n.to_string()}),
        Value::Number(n) => json!({"doubleValue": n}),
        Value::Array(a) => {
            json!({"arrayValue": {"values": a.iter().map(|x| json!({"stringValue": x.as_str().unwrap_or("")})).collect::<Vec<_>>()}})
        }
        _ => json!({"stringValue": v.to_string()}),
    };
    json!({"key": k, "value": val})
}

pub fn span(c: &Call, service: &str) -> Value {
    let start_ns = c.ts * 1_000_000;
    let end_ns = start_ns + c.latency_ms * 1_000_000;
    let mut attrs = vec![
        attr("gen_ai.operation.name", json!("chat")),
        attr(
            "gen_ai.system",
            json!(if c.provider.is_empty() {
                "unknown"
            } else {
                &c.provider
            }),
        ),
        attr("gen_ai.request.model", json!(c.model)),
        attr("gen_ai.usage.input_tokens", json!(c.input_tokens)),
        attr("gen_ai.usage.output_tokens", json!(c.output_tokens)),
        attr("gen_ai.response.finish_reasons", json!([c.finish_reason])),
        attr("app.department", json!(c.dept)),
        attr("app.feature", json!(c.feature)),
        attr("enduser.id", json!(c.user)),
    ];
    if c.cached_tokens > 0 {
        attrs.push(attr(
            "gen_ai.usage.cached_input_tokens",
            json!(c.cached_tokens),
        ));
    }
    let error = c.status != "ok";
    if error {
        attrs.push(attr("error.type", json!(c.status)));
    }
    json!({
        "traceId": hex_id(&format!("{service}/{}", c.id), 16),
        "spanId": hex_id(&format!("{service}/{}/span", c.id), 8),
        "name": format!("chat {}", c.model),
        "kind": 3, // SPAN_KIND_CLIENT
        "startTimeUnixNano": start_ns.to_string(),
        "endTimeUnixNano": end_ns.to_string(),
        "attributes": attrs,
        "status": if error { json!({"code": 2, "message": c.status}) } else { json!({"code": 1}) }
    })
}

pub fn export_op(i: &Value) -> Result<Value, String> {
    let cs = calls(i)?;
    let service = i["service_name"].as_str().unwrap_or("llm-gateway");
    let spans: Vec<Value> = cs.iter().map(|c| span(c, service)).collect();
    Ok(json!({"resourceSpans": [{
        "resource": {"attributes": [attr("service.name", json!(service)), attr("deployment.environment", json!(i["environment"].as_str().unwrap_or("prod")))]},
        "scopeSpans": [{"scope": {"name": "fde-09-llm-telemetry", "version": env!("CARGO_PKG_VERSION")}, "spans": spans}]
    }]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn span_has_genai_attrs_and_stable_ids() {
        let c: Call = serde_json::from_value(
            json!({"id": "c1", "ts": 1000, "model": "m", "provider": "anthropic",
            "input_tokens": 10, "output_tokens": 5, "latency_ms": 250, "status": "timeout"}),
        )
        .unwrap();
        let a = span(&c, "svc");
        assert_eq!(a, span(&c, "svc"));
        assert_eq!(a["traceId"].as_str().unwrap().len(), 32);
        assert_eq!(a["spanId"].as_str().unwrap().len(), 16);
        assert_eq!(a["endTimeUnixNano"], "1250000000");
        assert_eq!(a["status"]["code"], 2);
        assert!(a["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["key"] == "gen_ai.usage.input_tokens" && x["value"]["intValue"] == "10"));
    }
}
