mod engine;
mod format;
mod sim;
mod tui;

use serde_json::{json, Value};
use std::io::{BufRead, Write};

const USAGE: &str = "plcemu - MELSEC iQ-R style ladder simulator
USAGE:
  plcemu tui <program.json> [--scan-ms N]        interactive TUI (? for help)
  plcemu run <program.json> [--scan-ms N] [--scans N] [--set DEV=V[@SCAN]]... [--trace]
  plcemu test <tests.json>                        run test file; exit 1 on failure
  plcemu check <program.json>                     validate program file
  plcemu import <melsec-il|keyence-kv> <file>     convert to plcemu-ladder v1 (stdout)
  plcemu mcp                                      stdio MCP server";

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(match real_main(&a) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", json!({"ok": false, "error": e}));
            2
        }
    });
}

fn opt(a: &[String], k: &str) -> Option<String> {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1).cloned())
}

fn real_main(a: &[String]) -> Result<i32, String> {
    let cmd = a.first().map(String::as_str).unwrap_or("--help");
    let file = || {
        a.get(1)
            .cloned()
            .ok_or_else(|| format!("missing file argument\n{USAGE}"))
    };
    let scan_ms: u32 = opt(a, "--scan-ms")
        .map(|s| s.parse().map_err(|_| "bad --scan-ms"))
        .transpose()?
        .unwrap_or(10);
    if scan_ms == 0 {
        return Err("--scan-ms must be >= 1".into());
    }
    match cmd {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(0)
        }
        "tui" => {
            let p = format::load(&file()?)?;
            tui::run(p, scan_ms).map_err(|e| e.to_string())?;
            Ok(0)
        }
        "run" => {
            let p = format::load(&file()?)?;
            let scans: u64 = opt(a, "--scans")
                .map(|s| s.parse().map_err(|_| "bad --scans"))
                .transpose()?
                .unwrap_or(1);
            let mut inputs = vec![];
            for (i, x) in a.iter().enumerate() {
                if x == "--set" {
                    let s = a.get(i + 1).ok_or("--set needs DEV=V[@SCAN]")?;
                    let (dv, at) = s.split_once('@').unwrap_or((s, "0"));
                    let (d, v) = dv.split_once('=').ok_or("--set needs DEV=V")?;
                    inputs.push((
                        at.parse().map_err(|_| "bad @SCAN")?,
                        engine::Dev::parse(d, true)?,
                        v.parse().map_err(|_| "bad value")?,
                    ));
                }
            }
            let r = sim::simulate(p, scan_ms, scans, &inputs, a.iter().any(|x| x == "--trace"));
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
            Ok(if r["ok"] == true { 0 } else { 1 })
        }
        "test" => {
            let f = file()?;
            let s = std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?;
            let v: Value = serde_json::from_str(&s).map_err(|e| format!("{f}: {e}"))?;
            let base = std::path::Path::new(&f)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            let r = sim::run_tests(&v, &base)?;
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
            Ok(if r["ok"] == true { 0 } else { 1 })
        }
        "check" => {
            let p = format::load(&file()?)?;
            println!(
                "{}",
                json!({"ok": true, "rungs": p.rungs.len(), "devices": p.devices().iter().map(|d| d.to_string()).collect::<Vec<_>>()})
            );
            Ok(0)
        }
        "import" => {
            let id = file()?;
            let path = a.get(2).ok_or("import needs <format> <file>")?;
            let imp = format::importers()
                .into_iter()
                .find(|i| i.id() == id)
                .ok_or_else(|| format!("unknown import format '{id}' (melsec-il, keyence-kv)"))?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let p = imp.import(&text)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&format::to_file(&p)).unwrap()
            );
            Ok(0)
        }
        "mcp" => {
            mcp();
            Ok(0)
        }
        _ => Err(format!("unknown command '{cmd}'\n{USAGE}")),
    }
}

fn program_arg(v: &Value) -> Result<engine::Program, String> {
    match v {
        Value::String(p) => format::load(p),
        Value::Object(_) => format::from_value(v.clone()),
        _ => Err("'program' must be a file path or a plcemu-ladder object".into()),
    }
}

fn tool_call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "simulate" => {
            let p = program_arg(&args["program"])?;
            let inputs = sim::parse_inputs(&args["inputs"])?;
            let scan_ms = args["scan_ms"].as_u64().unwrap_or(10).max(1) as u32;
            Ok(sim::simulate(
                p,
                scan_ms,
                args["scans"].as_u64().unwrap_or(1),
                &inputs,
                args["trace"].as_bool().unwrap_or(false),
            ))
        }
        "run_tests" => match &args["tests"] {
            Value::String(f) => {
                let s = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
                sim::run_tests(
                    &v,
                    std::path::Path::new(f)
                        .parent()
                        .unwrap_or(std::path::Path::new(".")),
                )
            }
            v => sim::run_tests(v, std::path::Path::new(".")),
        },
        "check_program" => {
            let p = program_arg(&args["program"])?;
            Ok(json!({"ok": true, "rungs": p.rungs.len()}))
        }
        _ => Err(format!("unknown tool '{name}'")),
    }
}

fn mcp() {
    let prog = json!({"description": "plcemu-ladder v1 object, or a path to a program JSON file", "type": ["object", "string"]});
    let tools = json!([
        {"name": "simulate", "description": "Run a ladder program deterministically for a fixed number of scans with fixed scan time; returns device values as JSON.",
         "inputSchema": {"type": "object", "required": ["program"], "properties": {
            "program": prog, "scan_ms": {"type": "integer", "default": 10}, "scans": {"type": "integer", "default": 1},
            "trace": {"type": "boolean"},
            "inputs": {"type": "array", "items": {"type": "object", "properties": {"scan": {"type": "integer"}, "device": {"type": "string"}, "value": {"type": "integer"}}}}}}},
        {"name": "run_tests", "description": "Run a plcemu-test v1 file (path) or object; returns pass/fail JSON.",
         "inputSchema": {"type": "object", "required": ["tests"], "properties": {"tests": {"type": ["object", "string"]}}}},
        {"name": "check_program", "description": "Validate a plcemu-ladder program.",
         "inputSchema": {"type": "object", "required": ["program"], "properties": {"program": prog}}}
    ]);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})
                );
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req["method"].as_str().unwrap_or("");
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(
                json!({"protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
                "capabilities": {"tools": {}}, "serverInfo": {"name": "plcemu", "version": env!("CARGO_PKG_VERSION")}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools})),
            "tools/call" => {
                let p = &req["params"];
                let r = tool_call(p["name"].as_str().unwrap_or(""), &p["arguments"]);
                let (txt, err) = match r {
                    Ok(v) => (serde_json::to_string_pretty(&v).unwrap(), v["ok"] == false),
                    Err(e) => (json!({"ok": false, "error": e}).to_string(), true),
                };
                Ok(json!({"content": [{"type": "text", "text": txt}], "isError": err}))
            }
            m if m.starts_with("notifications/") => continue,
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        let Some(id) = id else { continue };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((c, m)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": c, "message": m}}),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests;
