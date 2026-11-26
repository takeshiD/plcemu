//! Versioned on-disk format (`plcemu-ladder` v1) and import entry points.
//! The execution model lives in `engine.rs`; this module only converts.
use crate::engine::{Instr, Program, Rung};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "plcemu-ladder";
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileV1 {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_unit")]
    pub timer_unit_ms: u32,
    pub rungs: Vec<RungV1>,
}
fn default_unit() -> u32 {
    100
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RungV1 {
    #[serde(default)]
    pub comment: String,
    pub il: Vec<String>,
}

pub fn from_value(v: serde_json::Value) -> Result<Program, String> {
    let f: FileV1 = serde_json::from_value(v).map_err(|e| format!("invalid program file: {e}"))?;
    if f.format != FORMAT {
        return Err(format!(
            "unknown format '{}' (expected '{FORMAT}')",
            f.format
        ));
    }
    if f.version != VERSION {
        return Err(format!(
            "unsupported version {} (this build supports {VERSION})",
            f.version
        ));
    }
    let mut rungs = vec![];
    for (i, r) in f.rungs.iter().enumerate() {
        let instrs =
            r.il.iter()
                .map(|l| Instr::parse(l))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("rung {i}: {e}"))?;
        if instrs.is_empty() || !instrs.iter().any(|x| x.is_output()) {
            return Err(format!("rung {i}: needs at least one output instruction"));
        }
        rungs.push(Rung {
            comment: r.comment.clone(),
            instrs,
        });
    }
    Ok(Program {
        name: f.name,
        timer_unit_ms: f.timer_unit_ms.max(1),
        rungs,
    })
}

pub fn load(path: &str) -> Result<Program, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&s).map_err(|e| format!("{path}: {e}"))?;
    from_value(v)
}

pub fn to_file(p: &Program) -> FileV1 {
    FileV1 {
        format: FORMAT.into(),
        version: VERSION,
        name: p.name.clone(),
        timer_unit_ms: p.timer_unit_ms,
        rungs: p
            .rungs
            .iter()
            .map(|r| RungV1 {
                comment: r.comment.clone(),
                il: r.instrs.iter().map(|i| i.to_string()).collect(),
            })
            .collect(),
    }
}

/// Import entry point for vendor formats.
pub trait Importer {
    fn id(&self) -> &'static str;
    fn import(&self, text: &str) -> Result<Program, String>;
}

/// MELSEC instruction-list text (as exported/copied from a mnemonic list):
/// one instruction per line, optional leading step number, `;` comments.
/// A new rung starts at a LD-type instruction that follows an output.
pub struct MelsecIl;
impl Importer for MelsecIl {
    fn id(&self) -> &'static str {
        "melsec-il"
    }
    fn import(&self, text: &str) -> Result<Program, String> {
        let mut rungs: Vec<Rung> = vec![];
        let mut cur: Vec<Instr> = vec![];
        for (ln, raw) in text.lines().enumerate() {
            let line = raw.split(';').next().unwrap_or("").trim();
            let mut toks: Vec<&str> = line.split_whitespace().collect();
            if toks
                .first()
                .map(|t| t.chars().all(|c| c.is_ascii_digit()))
                .unwrap_or(false)
            {
                toks.remove(0);
            }
            if toks.is_empty() || matches!(toks[0].to_ascii_uppercase().as_str(), "END" | "NOP") {
                continue;
            }
            let ins = Instr::parse(&toks.join(" ")).map_err(|e| format!("line {}: {e}", ln + 1))?;
            let starts = matches!(
                ins,
                Instr::Contact {
                    op: crate::engine::Op::Ld,
                    ..
                } | Instr::Compare {
                    op: crate::engine::Op::Ld,
                    ..
                }
            );
            if starts && cur.last().map(|i| i.is_output()).unwrap_or(false) {
                rungs.push(Rung {
                    comment: String::new(),
                    instrs: std::mem::take(&mut cur),
                });
            }
            cur.push(ins);
        }
        if !cur.is_empty() {
            rungs.push(Rung {
                comment: String::new(),
                instrs: cur,
            });
        }
        Ok(Program {
            name: "imported".into(),
            timer_unit_ms: 100,
            rungs,
        })
    }
}

/// Placeholder: KEYENCE KV format is accepted at the entry point but not converted yet.
pub struct KeyenceKv;
impl Importer for KeyenceKv {
    fn id(&self) -> &'static str {
        "keyence-kv"
    }
    fn import(&self, _text: &str) -> Result<Program, String> {
        Err("keyence-kv import is not implemented yet (entry point only)".into())
    }
}

pub fn importers() -> Vec<Box<dyn Importer>> {
    vec![Box::new(MelsecIl), Box::new(KeyenceKv)]
}
