//! Headless deterministic runs and file-defined tests.
use crate::engine::{Dev, Plc, Program};
use crate::format;
use serde_json::{json, Value};

/// inputs: [{"scan":N,"device":"X0","value":1}] applied before scan N (0-based).
pub fn simulate(
    prog: Program,
    scan_ms: u32,
    scans: u64,
    inputs: &[(u64, Dev, i64)],
    trace: bool,
) -> Value {
    let mut plc = Plc::new(prog, scan_ms);
    let devs = plc.prog.devices();
    let mut tr = vec![];
    let mut err = None;
    for s in 0..scans {
        for (at, d, v) in inputs {
            if *at == s {
                if let Err(e) = plc.set(*d, *v) {
                    err = Some(e);
                }
            }
        }
        if err.is_some() {
            break;
        }
        if let Err(e) = plc.scan() {
            err = Some(e);
            break;
        }
        if trace {
            tr.push(json!({"scan": plc.scans, "time_ms": plc.time_ms, "devices": snapshot(&plc, &devs)}));
        }
    }
    let mut out = json!({"ok": err.is_none(), "scan_ms": scan_ms, "scans": plc.scans, "time_ms": plc.time_ms, "devices": snapshot(&plc, &devs), "error": err});
    if trace {
        out["trace"] = Value::Array(tr);
    }
    out
}

pub fn snapshot(plc: &Plc, devs: &[Dev]) -> Value {
    let mut m = serde_json::Map::new();
    for d in devs {
        let key = match d.kind {
            crate::engine::Kind::TS => format!("TS{}", d.n),
            crate::engine::Kind::CS => format!("CS{}", d.n),
            _ => d.to_string(),
        };
        m.insert(key, json!(plc.get(*d)));
    }
    Value::Object(m)
}

pub fn parse_inputs(v: &Value) -> Result<Vec<(u64, Dev, i64)>, String> {
    let mut out = vec![];
    for i in v.as_array().cloned().unwrap_or_default() {
        let d = Dev::parse(i["device"].as_str().ok_or("input.device required")?, true)?;
        out.push((i["scan"].as_u64().unwrap_or(0), d, val(&i["value"])?));
    }
    Ok(out)
}

fn val(v: &Value) -> Result<i64, String> {
    v.as_i64()
        .or_else(|| v.as_bool().map(|b| b as i64))
        .ok_or_else(|| format!("bad value {v}"))
}

/// Test file (`plcemu-test` v1). `base` resolves a relative `program` path.
pub fn run_tests(spec: &Value, base: &std::path::Path) -> Result<Value, String> {
    if spec["format"] != "plcemu-test" || spec["version"] != 1 {
        return Err("test file must have format 'plcemu-test' and version 1".into());
    }
    let prog = match &spec["program"] {
        Value::String(p) => format::load(base.join(p).to_str().unwrap_or(p))?,
        v @ Value::Object(_) => format::from_value(v.clone())?,
        _ => return Err("'program' (path or object) required".into()),
    };
    let scan_ms = spec["scan_ms"].as_u64().unwrap_or(10) as u32;
    let mut results = vec![];
    let (mut passed, mut failed) = (0, 0);
    for t in spec["tests"].as_array().ok_or("'tests' array required")? {
        let name = t["name"].as_str().unwrap_or("(unnamed)").to_string();
        let mut plc = Plc::new(prog.clone(), scan_ms);
        let mut fails = vec![];
        for (si, st) in t["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let r: Result<(), String> = (|| {
                if let Some(o) = st["set"].as_object() {
                    for (k, v) in o {
                        plc.set(Dev::parse(k, true)?, val(v)?)?;
                    }
                }
                let n = if let Some(n) = st["scans"].as_u64() {
                    n
                } else if let Some(ms) = st["ms"].as_u64() {
                    ms.div_ceil(scan_ms as u64)
                } else {
                    0
                };
                for _ in 0..n {
                    plc.scan()?;
                }
                if let Some(o) = st["expect"].as_object() {
                    for (k, v) in o {
                        let d = Dev::parse(
                            k,
                            k.len() > 1
                                && (k.starts_with("TN")
                                    || k.starts_with("CN")
                                    || k.starts_with('D')),
                        )?;
                        let (e, a) = (val(v)?, plc.get(d));
                        if e != a {
                            fails.push(json!({"step": si, "device": k, "expected": e, "actual": a, "scan": plc.scans, "time_ms": plc.time_ms}));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = r {
                fails.push(json!({"step": si, "error": e, "scan": plc.scans}));
                break;
            }
        }
        if fails.is_empty() {
            passed += 1
        } else {
            failed += 1
        }
        results.push(json!({"name": name, "ok": fails.is_empty(), "failures": fails}));
    }
    Ok(json!({"ok": failed == 0, "passed": passed, "failed": failed, "tests": results}))
}
