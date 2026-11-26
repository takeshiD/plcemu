use crate::engine::{Dev, Instr, Plc, Program, Rung};

fn plc(rungs: &[&[&str]], scan_ms: u32) -> Plc {
    let rungs = rungs
        .iter()
        .map(|r| Rung {
            comment: String::new(),
            instrs: r.iter().map(|l| Instr::parse(l).unwrap()).collect(),
        })
        .collect();
    Plc::new(
        Program {
            name: "t".into(),
            timer_unit_ms: 100,
            rungs,
        },
        scan_ms,
    )
}
fn d(s: &str) -> Dev {
    Dev::parse(s, true).unwrap()
}

#[test]
fn self_hold_and_b_contact() {
    let mut p = plc(&[&["LD X0", "OR Y0", "ANI X1", "OUT Y0"]], 10);
    p.set(d("X0"), 1).unwrap();
    assert_eq!(p.get(d("Y0")), 0, "input not visible before scan");
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 1);
    p.set(d("X0"), 0).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 1, "self-hold");
    p.set(d("X1"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 0);
}

#[test]
fn scan_order_visibility() {
    // rung0 reads M0 before rung1 writes it -> one-scan delay; rung2 sees it in the same scan.
    let mut p = plc(
        &[
            &["LD M0", "OUT Y0"],
            &["LD X0", "OUT M0"],
            &["LD M0", "OUT Y1"],
        ],
        10,
    );
    p.set(d("X0"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!((p.get(d("Y0")), p.get(d("Y1"))), (0, 1));
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 1);
}

#[test]
fn edges_one_scan() {
    let mut p = plc(&[&["LDP X0", "OUT Y0"], &["LDF X0", "OUT Y1"]], 10);
    p.set(d("X0"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 1);
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 0);
    p.set(d("X0"), 0).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("Y1")), 1);
    p.scan().unwrap();
    assert_eq!(p.get(d("Y1")), 0);
}

#[test]
fn set_rst_anb_orb() {
    let mut p = plc(
        &[
            &["LD X0", "SET M0"],
            &["LD X1", "RST M0"],
            &["LD X2", "OR X3", "LD X4", "OR X5", "ANB", "OUT Y0"],
        ],
        10,
    );
    p.set(d("X0"), 1).unwrap();
    p.scan().unwrap();
    p.set(d("X0"), 0).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("M0")), 1);
    p.set(d("X1"), 1).unwrap();
    p.set(d("X3"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!((p.get(d("M0")), p.get(d("Y0"))), (0, 0));
    p.set(d("X5"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 1);
}

#[test]
fn timer_boundary() {
    // K10 x 100ms = 1000ms; 100ms scans: contact turns on at the 11th execution (1000ms after coil-on)
    let mut p = plc(&[&["LD X0", "OUT T0 K10"], &["LD T0", "OUT Y0"]], 100);
    p.set(d("X0"), 1).unwrap();
    for _ in 0..10 {
        p.scan().unwrap();
    }
    assert_eq!((p.get(d("TN0")), p.get(d("Y0"))), (9, 0));
    p.scan().unwrap();
    assert_eq!((p.get(d("TN0")), p.get(d("Y0"))), (10, 1));
    p.set(d("X0"), 0).unwrap();
    p.scan().unwrap();
    assert_eq!((p.get(d("TN0")), p.get(d("Y0"))), (0, 0));
}

#[test]
fn counter_rising_edge_and_reset() {
    let mut p = plc(
        &[
            &["LD X0", "OUT C0 K3"],
            &["LD C0", "OUT Y0"],
            &["LD X1", "RST C0"],
        ],
        10,
    );
    for _ in 0..5 {
        p.set(d("X0"), 1).unwrap();
        p.scan().unwrap();
        p.scan().unwrap();
        p.set(d("X0"), 0).unwrap();
        p.scan().unwrap();
    }
    assert_eq!(
        (p.get(d("CN0")), p.get(d("Y0"))),
        (3, 1),
        "stops at set value"
    );
    p.set(d("X1"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!(
        (p.get(d("CN0")), p.get(d("Y0"))),
        (0, 1),
        "RST in later rung: Y0 updates next scan"
    );
    p.scan().unwrap();
    assert_eq!(p.get(d("Y0")), 0);
}

#[test]
fn arithmetic_boundaries() {
    let mut p = plc(
        &[&[
            "LD X0",
            "MOV K32767 D0",
            "+ D0 K1 D1",
            "- K-32768 K1 D2",
            "* K300 K300 D3",
            "/ K-7 K2 D5",
        ]],
        10,
    );
    p.set(d("X0"), 1).unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("D1")), -32768, "16-bit wraps");
    assert_eq!(p.get(d("D2")), 32767);
    let v = (p.get(d("D4")) << 16) | (p.get(d("D3")) & 0xFFFF);
    assert_eq!(v, 90000, "16-bit * stores 32-bit in D3,D4");
    assert_eq!((p.get(d("D5")), p.get(d("D6"))), (-3, -1));
}

#[test]
fn div_by_zero_is_error_and_pulse_once() {
    let mut p = plc(&[&["LD X0", "+P K1 D0"], &["LD X1", "/ K1 K0 D2"]], 10);
    p.set(d("X0"), 1).unwrap();
    p.scan().unwrap();
    p.scan().unwrap();
    assert_eq!(p.get(d("D0")), 1);
    p.set(d("X1"), 1).unwrap();
    assert!(p.scan().is_err());
    assert!(p.scan().is_err(), "stays in error");
}

#[test]
fn compare_and_parse_errors() {
    let mut p = plc(&[&["LD> D0 K5", "OUT Y0"], &["LD<= D0 K5", "OUT Y1"]], 10);
    p.set(d("D0"), 6).unwrap();
    p.scan().unwrap();
    assert_eq!((p.get(d("Y0")), p.get(d("Y1"))), (1, 0));
    assert!(Instr::parse("OUT X0").is_err());
    assert!(Instr::parse("MOV K40000 D0").is_err());
    assert!(Instr::parse("FOO X0").is_err());
    assert_eq!(Dev::parse("X1F", false).unwrap().n, 31, "X is hexadecimal");
}

#[test]
fn import_melsec_il_and_keyence_stub() {
    use crate::format::{Importer, KeyenceKv, MelsecIl};
    let p = MelsecIl
        .import("0 LD X0\n1 OUT Y0\n2 LDI X1\n3 OUT Y1\n4 END\n")
        .unwrap();
    assert_eq!(p.rungs.len(), 2);
    assert!(KeyenceKv.import("x").is_err());
}
