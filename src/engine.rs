//! Execution model. Independent from the on-disk format (see `format.rs`).
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    X,
    Y,
    M,
    /// Timer contact (T in bit context)
    TS,
    /// Timer current value (T in word context)
    TN,
    CS,
    CN,
    D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dev {
    pub kind: Kind,
    pub n: u32,
}

impl fmt::Display for Dev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            // iQ-R: X/Y are numbered in hexadecimal
            Kind::X => write!(f, "X{:X}", self.n),
            Kind::Y => write!(f, "Y{:X}", self.n),
            Kind::M => write!(f, "M{}", self.n),
            Kind::TS => write!(f, "T{}", self.n),
            Kind::TN => write!(f, "TN{}", self.n),
            Kind::CS => write!(f, "C{}", self.n),
            Kind::CN => write!(f, "CN{}", self.n),
            Kind::D => write!(f, "D{}", self.n),
        }
    }
}

impl Dev {
    pub fn is_bit(&self) -> bool {
        matches!(self.kind, Kind::X | Kind::Y | Kind::M | Kind::TS | Kind::CS)
    }
    /// Parse a device. `word` selects T/C meaning (current value vs contact).
    pub fn parse(s: &str, word: bool) -> Result<Dev, String> {
        let u = s.trim().to_ascii_uppercase();
        let (kind, rest, hex) = if let Some(r) = u.strip_prefix("TS") {
            (Kind::TS, r, false)
        } else if let Some(r) = u.strip_prefix("TN") {
            (Kind::TN, r, false)
        } else if let Some(r) = u.strip_prefix("CS") {
            (Kind::CS, r, false)
        } else if let Some(r) = u.strip_prefix("CN") {
            (Kind::CN, r, false)
        } else if let Some(r) = u.strip_prefix('X') {
            (Kind::X, r, true)
        } else if let Some(r) = u.strip_prefix('Y') {
            (Kind::Y, r, true)
        } else if let Some(r) = u.strip_prefix('M') {
            (Kind::M, r, false)
        } else if let Some(r) = u.strip_prefix('T') {
            (if word { Kind::TN } else { Kind::TS }, r, false)
        } else if let Some(r) = u.strip_prefix('C') {
            (if word { Kind::CN } else { Kind::CS }, r, false)
        } else if let Some(r) = u.strip_prefix('D') {
            (Kind::D, r, false)
        } else {
            return Err(format!("unknown device '{s}'"));
        };
        let n = if hex {
            u32::from_str_radix(rest, 16)
        } else {
            rest.parse::<u32>()
        }
        .map_err(|_| format!("bad device number in '{s}'"))?;
        Ok(Dev { kind, n })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum W {
    K(i32),
    Dev(Dev),
}

impl fmt::Display for W {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            W::K(k) => write!(f, "K{k}"),
            W::Dev(d) => write!(f, "{d}"),
        }
    }
}

impl W {
    pub fn parse(s: &str) -> Result<W, String> {
        let u = s.trim().to_ascii_uppercase();
        if let Some(r) = u.strip_prefix('K') {
            let v: i32 = r.parse().map_err(|_| format!("bad constant '{s}'"))?;
            if !(-32768..=32767).contains(&v) {
                return Err(format!("constant out of 16-bit range: '{s}'"));
            }
            return Ok(W::K(v));
        }
        if let Some(r) = u.strip_prefix('H') {
            let v = u16::from_str_radix(r, 16).map_err(|_| format!("bad hex constant '{s}'"))?;
            return Ok(W::K(v as i16 as i32));
        }
        let d = Dev::parse(&u, true)?;
        if d.is_bit() {
            return Err(format!("word operand expected, got bit device '{s}'"));
        }
        Ok(W::Dev(d))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Ld,
    And,
    Or,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    None,
    Rise,
    Fall,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Instr {
    Contact {
        op: Op,
        neg: bool,
        edge: Edge,
        dev: Dev,
    },
    Compare {
        op: Op,
        cmp: Cmp,
        a: W,
        b: W,
    },
    Anb,
    Orb,
    Mps,
    Mrd,
    Mpp,
    Out(Dev),
    OutT {
        n: u32,
        k: W,
    },
    OutC {
        n: u32,
        k: W,
    },
    Set(Dev),
    Rst(Dev),
    Mov {
        pulse: bool,
        s: W,
        d: Dev,
    },
    Arith {
        op: Arith,
        pulse: bool,
        s1: W,
        s2: W,
        d: Dev,
    },
}

impl Instr {
    pub fn is_output(&self) -> bool {
        matches!(
            self,
            Instr::Out(_)
                | Instr::OutT { .. }
                | Instr::OutC { .. }
                | Instr::Set(_)
                | Instr::Rst(_)
                | Instr::Mov { .. }
                | Instr::Arith { .. }
        )
    }

    /// Parse one MELSEC-style mnemonic line, e.g. `LD X0`, `OUT T0 K10`, `+ D0 K1 D0`.
    pub fn parse(line: &str) -> Result<Instr, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return Err("empty instruction".into());
        }
        let m = parts[0].to_ascii_uppercase();
        let args = &parts[1..];
        let need = |n: usize| -> Result<(), String> {
            if args.len() == n {
                Ok(())
            } else {
                Err(format!("'{line}': {m} takes {n} operand(s)"))
            }
        };
        let bit = |s: &str| -> Result<Dev, String> {
            let d = Dev::parse(s, false)?;
            if !d.is_bit() {
                return Err(format!("'{line}': bit device expected, got '{s}'"));
            }
            Ok(d)
        };
        let contact = |op, neg, edge| -> Result<Instr, String> {
            need(1)?;
            Ok(Instr::Contact {
                op,
                neg,
                edge,
                dev: bit(args[0])?,
            })
        };
        match m.as_str() {
            "LD" => contact(Op::Ld, false, Edge::None),
            "LDI" => contact(Op::Ld, true, Edge::None),
            "AND" => contact(Op::And, false, Edge::None),
            "ANI" => contact(Op::And, true, Edge::None),
            "OR" => contact(Op::Or, false, Edge::None),
            "ORI" => contact(Op::Or, true, Edge::None),
            "LDP" => contact(Op::Ld, false, Edge::Rise),
            "LDF" => contact(Op::Ld, false, Edge::Fall),
            "ANDP" => contact(Op::And, false, Edge::Rise),
            "ANDF" => contact(Op::And, false, Edge::Fall),
            "ORP" => contact(Op::Or, false, Edge::Rise),
            "ORF" => contact(Op::Or, false, Edge::Fall),
            "ANB" => need(0).map(|_| Instr::Anb),
            "ORB" => need(0).map(|_| Instr::Orb),
            "MPS" => need(0).map(|_| Instr::Mps),
            "MRD" => need(0).map(|_| Instr::Mrd),
            "MPP" => need(0).map(|_| Instr::Mpp),
            "OUT" => {
                if args.len() == 2 {
                    let d = Dev::parse(args[0], false)?;
                    let k = W::parse(args[1])?;
                    match d.kind {
                        Kind::TS => Ok(Instr::OutT { n: d.n, k }),
                        Kind::CS => Ok(Instr::OutC { n: d.n, k }),
                        _ => Err(format!("'{line}': set value only allowed for T/C")),
                    }
                } else {
                    need(1)?;
                    let d = bit(args[0])?;
                    match d.kind {
                        Kind::Y | Kind::M => Ok(Instr::Out(d)),
                        Kind::TS | Kind::CS => {
                            Err(format!("'{line}': OUT T/C needs a set value (e.g. K10)"))
                        }
                        _ => Err(format!("'{line}': OUT target must be Y or M")),
                    }
                }
            }
            "SET" => {
                need(1)?;
                let d = bit(args[0])?;
                if !matches!(d.kind, Kind::Y | Kind::M) {
                    return Err(format!("'{line}': SET target must be Y or M"));
                }
                Ok(Instr::Set(d))
            }
            "RST" => {
                need(1)?;
                let d = Dev::parse(args[0], false)?;
                if d.kind == Kind::X {
                    return Err(format!("'{line}': RST cannot target X"));
                }
                Ok(Instr::Rst(d))
            }
            "MOV" | "MOVP" => {
                need(2)?;
                Ok(Instr::Mov {
                    pulse: m.ends_with('P'),
                    s: W::parse(args[0])?,
                    d: word_dest(args[1])?,
                })
            }
            _ => {
                if let Some(a) = arith_of(&m) {
                    let pulse = m.ends_with('P');
                    return match args.len() {
                        2 => {
                            let d = word_dest(args[1])?;
                            Ok(Instr::Arith {
                                op: a,
                                pulse,
                                s1: W::Dev(d),
                                s2: W::parse(args[0])?,
                                d,
                            })
                        }
                        3 => Ok(Instr::Arith {
                            op: a,
                            pulse,
                            s1: W::parse(args[0])?,
                            s2: W::parse(args[1])?,
                            d: word_dest(args[2])?,
                        }),
                        _ => Err(format!("'{line}': {m} takes 2 or 3 operands")),
                    };
                }
                for (p, op) in [("LD", Op::Ld), ("AND", Op::And), ("OR", Op::Or)] {
                    if let Some(c) = m.strip_prefix(p) {
                        let cmp = match c {
                            "=" => Cmp::Eq,
                            "<>" => Cmp::Ne,
                            ">" => Cmp::Gt,
                            "<" => Cmp::Lt,
                            ">=" => Cmp::Ge,
                            "<=" => Cmp::Le,
                            _ => continue,
                        };
                        need(2)?;
                        return Ok(Instr::Compare {
                            op,
                            cmp,
                            a: W::parse(args[0])?,
                            b: W::parse(args[1])?,
                        });
                    }
                }
                Err(format!("unsupported instruction '{}'", parts[0]))
            }
        }
    }
}

fn arith_of(m: &str) -> Option<Arith> {
    Some(match m {
        "+" | "+P" | "ADD" | "ADDP" => Arith::Add,
        "-" | "-P" | "SUB" | "SUBP" => Arith::Sub,
        "*" | "*P" | "MUL" | "MULP" => Arith::Mul,
        "/" | "/P" | "DIV" | "DIVP" => Arith::Div,
        _ => return None,
    })
}

fn word_dest(s: &str) -> Result<Dev, String> {
    match W::parse(s)? {
        W::Dev(d) if d.kind == Kind::D => Ok(d),
        _ => Err(format!("destination must be a data register D, got '{s}'")),
    }
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opn = |op: &Op| match op {
            Op::Ld => "LD",
            Op::And => "AND",
            Op::Or => "OR",
        };
        match self {
            Instr::Contact { op, neg, edge, dev } => {
                let base = match (op, neg) {
                    (Op::Ld, true) => "LDI",
                    (Op::And, true) => "ANI",
                    (Op::Or, true) => "ORI",
                    (o, false) => opn(o),
                };
                let e = match edge {
                    Edge::None => "",
                    Edge::Rise => "P",
                    Edge::Fall => "F",
                };
                write!(f, "{base}{e} {dev}")
            }
            Instr::Compare { op, cmp, a, b } => {
                let c = match cmp {
                    Cmp::Eq => "=",
                    Cmp::Ne => "<>",
                    Cmp::Gt => ">",
                    Cmp::Lt => "<",
                    Cmp::Ge => ">=",
                    Cmp::Le => "<=",
                };
                write!(f, "{}{c} {a} {b}", opn(op))
            }
            Instr::Anb => write!(f, "ANB"),
            Instr::Orb => write!(f, "ORB"),
            Instr::Mps => write!(f, "MPS"),
            Instr::Mrd => write!(f, "MRD"),
            Instr::Mpp => write!(f, "MPP"),
            Instr::Out(d) => write!(f, "OUT {d}"),
            Instr::OutT { n, k } => write!(f, "OUT T{n} {k}"),
            Instr::OutC { n, k } => write!(f, "OUT C{n} {k}"),
            Instr::Set(d) => write!(f, "SET {d}"),
            Instr::Rst(d) => write!(f, "RST {d}"),
            Instr::Mov { pulse, s, d } => write!(f, "MOV{} {s} {d}", if *pulse { "P" } else { "" }),
            Instr::Arith {
                op,
                pulse,
                s1,
                s2,
                d,
            } => {
                let o = match op {
                    Arith::Add => "+",
                    Arith::Sub => "-",
                    Arith::Mul => "*",
                    Arith::Div => "/",
                };
                write!(f, "{o}{} {s1} {s2} {d}", if *pulse { "P" } else { "" })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rung {
    pub comment: String,
    pub instrs: Vec<Instr>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub name: String,
    pub timer_unit_ms: u32,
    pub rungs: Vec<Rung>,
}

impl Program {
    /// All devices referenced by the program, sorted.
    pub fn devices(&self) -> Vec<Dev> {
        let mut v = std::collections::BTreeSet::new();
        let addw = |w: &W, v: &mut std::collections::BTreeSet<Dev>| {
            if let W::Dev(d) = w {
                v.insert(*d);
            }
        };
        for r in &self.rungs {
            for i in &r.instrs {
                match i {
                    Instr::Contact { dev, .. }
                    | Instr::Out(dev)
                    | Instr::Set(dev)
                    | Instr::Rst(dev) => {
                        v.insert(*dev);
                    }
                    Instr::Compare { a, b, .. } => {
                        addw(a, &mut v);
                        addw(b, &mut v);
                    }
                    Instr::OutT { n, .. } => {
                        v.insert(Dev {
                            kind: Kind::TS,
                            n: *n,
                        });
                        v.insert(Dev {
                            kind: Kind::TN,
                            n: *n,
                        });
                    }
                    Instr::OutC { n, .. } => {
                        v.insert(Dev {
                            kind: Kind::CS,
                            n: *n,
                        });
                        v.insert(Dev {
                            kind: Kind::CN,
                            n: *n,
                        });
                    }
                    Instr::Mov { s, d, .. } => {
                        addw(s, &mut v);
                        v.insert(*d);
                    }
                    Instr::Arith { op, s1, s2, d, .. } => {
                        addw(s1, &mut v);
                        addw(s2, &mut v);
                        v.insert(*d);
                        if matches!(op, Arith::Mul | Arith::Div) {
                            v.insert(Dev {
                                kind: Kind::D,
                                n: d.n + 1,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        v.into_iter().collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Timer {
    pub ms: u64,
    pub was_on: bool,
}

#[derive(Clone, Debug)]
pub struct Plc {
    pub prog: Program,
    pub scan_ms: u32,
    pub scans: u64,
    pub time_ms: u64,
    /// External input image (applied at the start of each scan: input refresh).
    pub inputs: BTreeMap<u32, bool>,
    bits: BTreeMap<Dev, bool>,
    words: BTreeMap<Dev, i16>,
    timers: BTreeMap<u32, Timer>,
    counter_prev: BTreeMap<u32, bool>,
    /// Per-instruction previous state for edge contacts and pulse instructions.
    prev: Vec<Vec<bool>>,
    pub error: Option<String>,
}

impl Plc {
    pub fn new(prog: Program, scan_ms: u32) -> Plc {
        let prev = prog
            .rungs
            .iter()
            .map(|r| vec![false; r.instrs.len()])
            .collect();
        Plc {
            prog,
            scan_ms,
            scans: 0,
            time_ms: 0,
            inputs: BTreeMap::new(),
            bits: BTreeMap::new(),
            words: BTreeMap::new(),
            timers: BTreeMap::new(),
            counter_prev: BTreeMap::new(),
            prev,
            error: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Plc::new(self.prog.clone(), self.scan_ms);
    }

    /// Set an external value. X goes to the input image (visible next scan);
    /// other devices are written directly (like a device write from a monitor).
    pub fn set(&mut self, d: Dev, v: i64) -> Result<(), String> {
        match d.kind {
            Kind::X => {
                self.inputs.insert(d.n, v != 0);
            }
            Kind::Y | Kind::M => {
                self.bits.insert(d, v != 0);
            }
            Kind::D => {
                if !(-32768..=32767).contains(&v) {
                    return Err(format!("{d}: value {v} out of 16-bit range"));
                }
                self.words.insert(d, v as i16);
            }
            _ => return Err(format!("{d}: cannot be set externally")),
        }
        Ok(())
    }

    pub fn bit(&self, d: Dev) -> bool {
        match d.kind {
            Kind::TS => self.timer_done(d.n),
            Kind::CS => self.counter_val(d.n).1,
            _ => *self.bits.get(&d).unwrap_or(&false),
        }
    }

    pub fn word(&self, d: Dev) -> i16 {
        match d.kind {
            Kind::TN => self.timer_cur(d.n),
            Kind::CN => self.counter_val(d.n).0,
            _ => *self.words.get(&d).unwrap_or(&0),
        }
    }

    pub fn get(&self, d: Dev) -> i64 {
        if d.is_bit() {
            self.bit(d) as i64
        } else {
            self.word(d) as i64
        }
    }

    pub fn timer_set(&self, n: u32) -> Option<i16> {
        for r in &self.prog.rungs {
            for i in &r.instrs {
                if let Instr::OutT { n: m, k } = i {
                    if *m == n {
                        return Some(self.wval(k));
                    }
                }
            }
        }
        None
    }
    fn counter_set(&self, n: u32) -> Option<i16> {
        for r in &self.prog.rungs {
            for i in &r.instrs {
                if let Instr::OutC { n: m, k } = i {
                    if *m == n {
                        return Some(self.wval(k));
                    }
                }
            }
        }
        None
    }
    fn timer_cur(&self, n: u32) -> i16 {
        let t = self.timers.get(&n).cloned().unwrap_or_default();
        let units = t.ms / self.prog.timer_unit_ms.max(1) as u64;
        let set = self.timer_set(n).unwrap_or(i16::MAX).max(0) as u64;
        units.min(set).min(i16::MAX as u64) as i16
    }
    fn timer_done(&self, n: u32) -> bool {
        let t = self.timers.get(&n).cloned().unwrap_or_default();
        let set = self.timer_set(n).unwrap_or(i16::MAX).max(0) as u64;
        t.was_on && t.ms >= set * self.prog.timer_unit_ms as u64
    }
    pub fn counter_val(&self, n: u32) -> (i16, bool) {
        let cur = *self.words.get(&Dev { kind: Kind::CN, n }).unwrap_or(&0);
        let set = self.counter_set(n).unwrap_or(i16::MAX);
        (cur, cur >= set)
    }

    fn wval(&self, w: &W) -> i16 {
        match w {
            W::K(k) => *k as i16,
            W::Dev(d) => self.word(*d),
        }
    }

    /// Instruction-level contact state (for the monitor): is this contact conducting?
    pub fn contact_state(&self, ri: usize, ii: usize) -> bool {
        match &self.prog.rungs[ri].instrs[ii] {
            Instr::Contact { neg, edge, dev, .. } => match edge {
                Edge::None => self.bit(*dev) != *neg,
                // edge contacts are shown as the device state
                _ => self.bit(*dev),
            },
            Instr::Compare { cmp, a, b, .. } => compare(*cmp, self.wval(a), self.wval(b)),
            Instr::Out(d) | Instr::Set(d) => self.bit(*d),
            Instr::OutT { n, .. } => self.timers.get(n).map(|t| t.was_on).unwrap_or(false),
            Instr::OutC { n, .. } => self.counter_prev.get(n).copied().unwrap_or(false),
            _ => self.prev[ri][ii],
        }
    }

    /// Execute one full scan: input refresh -> program (rung order, top to bottom) -> END.
    pub fn scan(&mut self) -> Result<(), String> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        for (n, v) in self.inputs.clone() {
            self.bits.insert(Dev { kind: Kind::X, n }, v);
        }
        for ri in 0..self.prog.rungs.len() {
            if let Err(e) = self.exec_rung(ri) {
                let msg = format!(
                    "operation error at rung {} (scan {}): {e}",
                    ri,
                    self.scans + 1
                );
                self.error = Some(msg.clone());
                return Err(msg);
            }
        }
        self.scans += 1;
        self.time_ms += self.scan_ms as u64;
        Ok(())
    }

    fn exec_rung(&mut self, ri: usize) -> Result<(), String> {
        let mut blocks: Vec<bool> = Vec::new();
        let mut mstack: Vec<bool> = Vec::new();
        let n = self.prog.rungs[ri].instrs.len();
        for ii in 0..n {
            let ins = self.prog.rungs[ri].instrs[ii].clone();
            let top = blocks.last().copied().unwrap_or(false);
            match ins {
                Instr::Contact { op, neg, edge, dev } => {
                    let cur = self.bit(dev);
                    let v = match edge {
                        Edge::None => cur != neg,
                        Edge::Rise => cur && !self.prev[ri][ii],
                        Edge::Fall => !cur && self.prev[ri][ii],
                    };
                    self.prev[ri][ii] = cur;
                    apply(&mut blocks, op, v)?;
                }
                Instr::Compare { op, cmp, a, b } => {
                    let v = compare(cmp, self.wval(&a), self.wval(&b));
                    apply(&mut blocks, op, v)?;
                }
                Instr::Anb | Instr::Orb => {
                    let b = blocks.pop().ok_or("ANB/ORB without block")?;
                    let a = blocks.last_mut().ok_or("ANB/ORB needs two blocks")?;
                    if ins == Instr::Anb {
                        *a = *a && b
                    } else {
                        *a = *a || b
                    }
                }
                Instr::Mps => mstack.push(top),
                Instr::Mrd => {
                    *blocks.last_mut().ok_or("MRD without condition")? =
                        *mstack.last().ok_or("MRD without MPS")?
                }
                Instr::Mpp => {
                    *blocks.last_mut().ok_or("MPP without condition")? =
                        mstack.pop().ok_or("MPP without MPS")?
                }
                Instr::Out(d) => {
                    self.bits.insert(d, top);
                }
                Instr::OutT { n, .. } => {
                    let unit = self.scan_ms as u64;
                    let t = self.timers.entry(n).or_default();
                    if top {
                        if t.was_on {
                            t.ms = t.ms.saturating_add(unit);
                        }
                        t.was_on = true;
                    } else {
                        *t = Timer::default();
                    }
                }
                Instr::OutC { n, k } => {
                    let set = self.wval(&k);
                    let prev = self.counter_prev.insert(n, top).unwrap_or(false);
                    if top && !prev {
                        let cn = Dev { kind: Kind::CN, n };
                        let cur = self.word(cn);
                        if cur < set {
                            self.words.insert(cn, cur + 1);
                        }
                    }
                }
                Instr::Set(d) => {
                    if top {
                        self.bits.insert(d, true);
                    }
                }
                Instr::Rst(d) => {
                    if top {
                        match d.kind {
                            Kind::TS | Kind::TN => {
                                self.timers.insert(d.n, Timer::default());
                            }
                            Kind::CS | Kind::CN => {
                                self.words.insert(
                                    Dev {
                                        kind: Kind::CN,
                                        n: d.n,
                                    },
                                    0,
                                );
                            }
                            Kind::D => {
                                self.words.insert(d, 0);
                            }
                            _ => {
                                self.bits.insert(d, false);
                            }
                        }
                    }
                }
                Instr::Mov { pulse, s, d } => {
                    if self.fire(ri, ii, pulse, top) {
                        let v = self.wval(&s);
                        self.words.insert(d, v);
                    }
                }
                Instr::Arith {
                    op,
                    pulse,
                    s1,
                    s2,
                    d,
                } => {
                    if self.fire(ri, ii, pulse, top) {
                        let a = self.wval(&s1);
                        let b = self.wval(&s2);
                        let hi = Dev {
                            kind: Kind::D,
                            n: d.n + 1,
                        };
                        match op {
                            Arith::Add => {
                                self.words.insert(d, a.wrapping_add(b));
                            }
                            Arith::Sub => {
                                self.words.insert(d, a.wrapping_sub(b));
                            }
                            Arith::Mul => {
                                let r = a as i32 * b as i32;
                                self.words.insert(d, r as i16);
                                self.words.insert(hi, (r >> 16) as i16);
                            }
                            Arith::Div => {
                                if b == 0 {
                                    return Err(format!(
                                        "division by zero in '{}'",
                                        self.prog.rungs[ri].instrs[ii]
                                    ));
                                }
                                // quotient -> D, remainder -> D+1 (truncation toward zero).
                                // -32768 / -1 overflows 16 bits; wrap it.
                                self.words.insert(d, (a as i32 / b as i32) as i16);
                                self.words.insert(hi, (a as i32 % b as i32) as i16);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn fire(&mut self, ri: usize, ii: usize, pulse: bool, top: bool) -> bool {
        let prev = self.prev[ri][ii];
        self.prev[ri][ii] = top;
        if pulse {
            top && !prev
        } else {
            top
        }
    }
}

fn apply(blocks: &mut Vec<bool>, op: Op, v: bool) -> Result<(), String> {
    match op {
        Op::Ld => blocks.push(v),
        Op::And => {
            let t = blocks.last_mut().ok_or("AND without LD")?;
            *t = *t && v
        }
        Op::Or => {
            let t = blocks.last_mut().ok_or("OR without LD")?;
            *t = *t || v
        }
    }
    Ok(())
}

fn compare(c: Cmp, a: i16, b: i16) -> bool {
    match c {
        Cmp::Eq => a == b,
        Cmp::Ne => a != b,
        Cmp::Gt => a > b,
        Cmp::Lt => a < b,
        Cmp::Ge => a >= b,
        Cmp::Le => a <= b,
    }
}
