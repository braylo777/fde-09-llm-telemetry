//! CLI: `fde-09-llm-telemetry <op> [file.json]` (reads JSON input from the file or stdin).
//! `fde-09-llm-telemetry ops` lists the operations.
use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let op = args.get(1).map(String::as_str).unwrap_or("ops");
    if op == "ops" || op == "--help" || op == "-h" {
        println!("fde-09-llm-telemetry: OpenTelemetry spans for LLM calls, cost per department, time-saved ROI, silent-failure alerts and usage billing.");
        println!("usage: fde-09-llm-telemetry <op> [input.json]   (input JSON from file or stdin)");
        for o in fde_09_llm_telemetry::OPS {
            println!("  {o}");
        }
        return;
    }
    let mut raw = String::new();
    match args.get(2) {
        Some(p) => raw = std::fs::read_to_string(p).expect("read input file"),
        None => {
            std::io::stdin().read_to_string(&mut raw).expect("read stdin");
        }
    }
    let input: serde_json::Value = serde_json::from_str(if raw.trim().is_empty() { "null" } else { &raw })
        .expect("input must be JSON");
    let req = serde_json::json!({"op": op, "input": input}).to_string();
    let out: serde_json::Value = serde_json::from_str(&fde_09_llm_telemetry::dispatch(&req)).unwrap();
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    if out["ok"] != true {
        std::process::exit(1);
    }
}
