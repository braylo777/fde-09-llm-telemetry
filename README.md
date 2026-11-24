# fde-09-llm-telemetry

**Forward Deployed Engineering — Stage 9 of 12: Telemetry, ROI & Customer Health**

Turn raw LLM call logs into OpenTelemetry traces, a cost-per-department report, a time-saved ROI number, alerts for failures that return HTTP 200, and an invoice.

**Also built as Project 9 of 12, "Department-Level FinOps & ROI Dashboard",** from the companion [12 FDE projects post](https://x.com/suraj_sharma14/status/2125197762752806998).

**Live demo:** https://fde-09-llm-telemetry.braylo777.workers.dev — the Rust core runs in your browser as WebAssembly. No server, no tracking.

## Why this matters

After go-live the customer asks two questions: is it working, and is it worth it? Status-code dashboards miss the real failures (empty answers, truncated JSON, a feature nobody calls any more), and a token bill means nothing to a CFO. This repo turns the same call log into answers both the on-call engineer and the budget owner can use.

## What it does

- OTLP/JSON export with OpenTelemetry GenAI attributes (gen_ai.request.model, gen_ai.usage.*, finish reasons, error.type); stable trace ids
- Cost per call with prompt-cache pricing; attribution by department, feature, model and top users; unpriced models reported, not dropped
- ROI per feature: only accepted outputs count; review time is charged; flags features that lose time or lack feedback
- Silent-failure detectors vs. a baseline window: traffic stopped, empty outputs, refusals, truncation, invalid JSON, p95 latency, output drift, model change
- Billing meter: idempotent event ids, failed calls free, included units, graduated tiers, integer micro-dollar money

## What it does not do (yet)

- No collector or dashboard: the OTLP JSON is ready to POST to /v1/traces on any OTel collector, but none is run here
- Prices in the demo are example numbers; load your contract rates
- Baselines for ROI come from the customer (time studies); this repo only does the math

## Operations

Every operation takes JSON in and gives JSON out. The CLI, the tests and the demo page all use the same entry point.

| Operation | What it does |
|---|---|
| `to_otel` | Six calls exported as OTLP/JSON with GenAI attributes. POST the output to any collector's /v1/traces. |
| `cost_report` | Two weeks of calls priced per model, with cached input at the cache rate. Legal's long contracts dominate spend. |
| `roi_report` | Value = accepted outputs x (manual minutes - review minutes) at a loaded hourly rate. sales_email barely saves time: review is almost as slow as writing. |
| `detect_silent_failures` | Last 4 days vs. the 10 before. Every call returned HTTP 200, yet support replies come back empty, invoice JSON is truncated, and sales_email traffic stopped. |
| `billing_meter` | Per-customer invoice in 1k-token units: 2,000 included, then graduated tiers. The duplicate retry event and failed calls are not billed. |

## Run it

You need Rust (stable). No Node, no npm.

```bash
git clone https://github.com/braylo777/fde-09-llm-telemetry
cd fde-09-llm-telemetry
cargo test
cargo run -- ops
```

Try the first demo example from the command line:

```bash
python3 -c 'import json;print(json.dumps(json.load(open("web/examples.json"))["ops"][0]["input"]))' | cargo run -q -- to_otel
```

## Send spans to a collector

```bash
cargo run -q -- to_otel calls.json | python3 -c 'import json,sys;print(json.dumps(json.load(sys.stdin)["result"]))' \
  | curl -s -X POST -H 'Content-Type: application/json' --data-binary @- http://localhost:4318/v1/traces
```

## Build and deploy the demo

```bash
./build.sh    # tests, then compiles the core to web/app.wasm
./deploy.sh   # serves web/ as a Cloudflare Workers static-assets site (needs a Cloudflare login)
```

`web/` is a plain static site (`index.html`, `app.js`, `app.wasm`). It has no build step, so any static host works.

## Layout

```
src/lib.rs        operation list and router
src/api.rs        JSON entry point + the 3 WASM exports (alloc, dealloc, run)
src/main.rs       CLI
src/*.rs          one module per capability, each with unit tests
tests/            integration tests (every demo example must pass)
web/              demo page; examples.json drives both the page and this README
```

## Design choices

- **Rust, few dependencies.** Only `serde`, `serde_json`, `sha2` and `hmac`, pinned to exact versions with a committed `Cargo.lock`.
- **Deterministic.** Time, randomness and keys are inputs, so every result can be replayed in a test or an audit.
- **Fail closed.** Unknown input, missing fields or an unmatched policy gives an error or a denial, never a silent pass.

## Part of a series

This is one of 12 repos that work through a public Forward Deployed Engineering roadmap ([12 stages](https://x.com/suraj_sharma14/status/2124827808535048566), [12 projects](https://x.com/suraj_sharma14/status/2125197762752806998)). All 12: [github.com/braylo777?q=fde-](https://github.com/braylo777?tab=repositories&q=fde-).

## License

MIT — see [LICENSE](LICENSE). Built by [Braylo SI](https://braylo.si).
