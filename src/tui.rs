//! Interactive TUI: ladder monitor, run/stop, single scan, input toggling.
use crate::engine::{Dev, Instr, Kind, Plc, Program};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::time::{Duration, Instant};

const HELP: &[&str] = &[
    "Space    実行(RUN) / 停止(STOP) 切替",
    "s        1スキャン実行 (停止中)",
    "↑/↓ j/k  入力(X)を選択",
    "Enter/t  選択した入力を ON/OFF",
    "r        リセット (全デバイス初期化)",
    "PgUp/PgDn ラダーをスクロール",
    "?        このヘルプ   q/Esc 終了",
    "",
    "表示: 反転/┫X0┣/((Y0))=ON(導通) 薄字=OFF",
    "┤X0├ a接点 ┤/X0├ b接点 P/F 立上り/下り",
];

pub fn run(prog: Program, scan_ms: u32) -> std::io::Result<()> {
    let mut term = ratatui::init();
    let r = app(&mut term, prog, scan_ms);
    ratatui::restore();
    r
}

fn app(term: &mut ratatui::DefaultTerminal, prog: Program, scan_ms: u32) -> std::io::Result<()> {
    let mut plc = Plc::new(prog, scan_ms);
    let devs = plc.prog.devices();
    let xs: Vec<Dev> = devs.iter().copied().filter(|d| d.kind == Kind::X).collect();
    let (mut sel, mut running, mut help, mut scroll) = (0usize, false, false, 0u16);
    let mut msg = String::from("STOP中: Spaceで実行, sで1スキャン, ?でヘルプ");
    let mut last = Instant::now();
    loop {
        term.draw(|f| {
            let a = f.area();
            let [top, mid, bot] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(2),
            ])
            .areas(a);
            let mode = if plc.error.is_some() {
                Span::styled(" ERROR ", Style::new().bg(Color::Red).fg(Color::White))
            } else if running {
                Span::styled(
                    " RUN ",
                    Style::new()
                        .bg(Color::Green)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    " STOP ",
                    Style::new()
                        .bg(Color::Yellow)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
            };
            f.render_widget(
                Line::from(vec![
                    mode,
                    Span::raw(format!(
                        " scan#{} t={}ms ({}ms/scan) {}",
                        plc.scans, plc.time_ms, scan_ms, plc.prog.name
                    )),
                ]),
                top,
            );
            let side = if a.width >= 70 { 26 } else { 20 };
            let [left, right] =
                Layout::horizontal([Constraint::Min(20), Constraint::Length(side)]).areas(mid);
            let lines = ladder(&plc);
            f.render_widget(
                Paragraph::new(lines)
                    .scroll((scroll, 0))
                    .block(Block::new().borders(Borders::ALL).title(" ラダー ")),
                left,
            );
            let mut mon = vec![Line::styled(
                "入力 (Enterで切替)",
                Style::new().add_modifier(Modifier::BOLD),
            )];
            for (i, x) in xs.iter().enumerate() {
                let on = *plc.inputs.get(&x.n).unwrap_or(&false);
                let mut st = onoff(on);
                if i == sel {
                    st = st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
                }
                mon.push(Line::from(vec![
                    Span::raw(if i == sel { "▶" } else { " " }),
                    Span::styled(format!("{x} {}", if on { "ON " } else { "OFF" }), st),
                ]));
            }
            mon.push(Line::styled(
                "監視",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            for d in devs
                .iter()
                .filter(|d| !matches!(d.kind, Kind::X | Kind::TN | Kind::CN))
            {
                let s = match d.kind {
                    Kind::TS => format!(
                        "T{} {}/{} {}",
                        d.n,
                        plc.get(Dev {
                            kind: Kind::TN,
                            n: d.n
                        }),
                        plc.timer_set(d.n).unwrap_or(0),
                        if plc.bit(*d) { "ON" } else { "OFF" }
                    ),
                    Kind::CS => {
                        let (c, on) = plc.counter_val(d.n);
                        format!("C{} {} {}", d.n, c, if on { "ON" } else { "OFF" })
                    }
                    Kind::D => format!("{d} = {}", plc.word(*d)),
                    _ => format!("{d} {}", if plc.bit(*d) { "ON" } else { "OFF" }),
                };
                let st = if d.kind == Kind::D {
                    Style::new()
                } else {
                    onoff(plc.bit(*d))
                };
                mon.push(Line::styled(s, st));
            }
            f.render_widget(
                Paragraph::new(mon).block(Block::new().borders(Borders::ALL).title(" 入力/監視 ")),
                right,
            );
            let keys = "?:ヘルプ Space:実行/停止 s:1スキャン ↑↓:選択 Enter:入力切替 q:終了";
            f.render_widget(
                Paragraph::new(vec![
                    Line::raw(msg.clone()),
                    Line::styled(keys, Style::new().fg(Color::Cyan)),
                ])
                .wrap(Wrap { trim: true }),
                bot,
            );
            if help {
                let w = 46.min(a.width);
                let h = (HELP.len() as u16 + 2).min(a.height);
                let r = Rect::new((a.width - w) / 2, (a.height - h) / 2, w, h);
                f.render_widget(Clear, r);
                f.render_widget(
                    Paragraph::new(HELP.iter().map(|s| Line::raw(*s)).collect::<Vec<_>>())
                        .block(Block::bordered().title(" ヘルプ (?/Escで閉じる) ")),
                    r,
                );
            }
        })?;
        if running {
            let due = (last.elapsed().as_millis() / scan_ms as u128).min(1000) as u64;
            for _ in 0..due {
                if let Err(e) = plc.scan() {
                    running = false;
                    msg = e;
                    break;
                }
                last += Duration::from_millis(scan_ms as u64);
            }
        }
        if !event::poll(Duration::from_millis(30))? {
            continue;
        }
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        if help {
            if matches!(
                k.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')
            ) {
                help = false;
            }
            continue;
        }
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('?') => help = true,
            KeyCode::Char(' ') => {
                running = !running && plc.error.is_none();
                last = Instant::now();
                msg = if running {
                    "RUN中: Spaceで停止".into()
                } else {
                    "STOPしました".into()
                };
            }
            KeyCode::Char('s') => {
                running = false;
                msg = match plc.scan() {
                    Ok(()) => format!("1スキャン実行しました (scan#{})", plc.scans),
                    Err(e) => e,
                };
            }
            KeyCode::Char('r') => {
                plc.reset();
                running = false;
                msg = "リセットしました".into();
            }
            KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(xs.len().saturating_sub(1)),
            KeyCode::PageUp => scroll = scroll.saturating_sub(5),
            KeyCode::PageDown => scroll = scroll.saturating_add(5),
            KeyCode::Enter | KeyCode::Char('t') => {
                if let Some(x) = xs.get(sel) {
                    let v = !*plc.inputs.get(&x.n).unwrap_or(&false);
                    plc.inputs.insert(x.n, v);
                    msg = format!(
                        "{x} を {} にしました (次スキャンで反映)",
                        if v { "ON" } else { "OFF" }
                    );
                }
            }
            _ => {}
        }
    }
}

/// ON = reverse video (green) + bold so it is visible even without color; OFF = dim gray.
fn onoff(on: bool) -> Style {
    // NO_COLOR (https://no-color.org): keep reverse/bold/dim, drop colors.
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return if on {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
    }
    if on {
        Style::new()
            .fg(Color::Green)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    }
}

fn width(row: &[Span]) -> usize {
    row.iter().map(|s| s.content.chars().count()).sum()
}

/// Ladder-like rendering built from the instruction list.
/// LD starts a new row, OR adds a branch row below ("┗"), and an AND/output that follows
/// branches joins them ("┘"/"┤") and continues on the block's first row. Outputs sit on the right.
/// ANB/ORB/MPS/MRD/MPP are shown as markers. Each element is highlighted with its monitored state.
fn ladder(plc: &Plc) -> Vec<Line<'static>> {
    use crate::engine::Op;
    let mut out = vec![];
    for (ri, r) in plc.prog.rungs.iter().enumerate() {
        let c = if r.comment.is_empty() {
            String::new()
        } else {
            format!(" ; {}", r.comment)
        };
        out.push(Line::styled(
            format!("{ri:>3}{c}"),
            Style::new().fg(Color::Cyan),
        ));
        let mut rows: Vec<Vec<Span<'static>>> = vec![vec![Span::raw("  ┃")]];
        let mut cur = 0usize;
        let mut out_col = 0usize;
        for (ii, ins) in r.instrs.iter().enumerate() {
            let on = plc.contact_state(ri, ii);
            let (op, txt) = match ins {
                Instr::Contact { op, neg, edge, dev } => {
                    let e = match edge {
                        crate::engine::Edge::Rise => " P",
                        crate::engine::Edge::Fall => " F",
                        _ => "",
                    };
                    {
                        let (l, rr) = if on { ("┫", "┣") } else { ("┤", "├") };
                        (
                            *op,
                            format!("{l}{}{dev}{e}{rr}", if *neg { "/" } else { "" }),
                        )
                    }
                }
                Instr::Compare { op, a, b, .. } => {
                    let s = ins.to_string();
                    let sym = s
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_start_matches(['L', 'D', 'A', 'N', 'O', 'R'])
                        .to_string();
                    {
                        let (l, rr) = if on { ("┫", "┣") } else { ("┤", "├") };
                        (*op, format!("{l}{a}{sym}{b}{rr}"))
                    }
                }
                Instr::Anb | Instr::Orb | Instr::Mps | Instr::Mrd | Instr::Mpp => {
                    rows[cur].push(Span::styled(
                        format!(" [{ins}] "),
                        Style::new().fg(Color::Magenta),
                    ));
                    continue;
                }
                Instr::Out(d) => (
                    Op::And,
                    if on {
                        format!("─(( {d} ))")
                    } else {
                        format!("─( {d} )")
                    },
                ),
                _ => (Op::And, format!("─[{ins}]")),
            };
            match op {
                Op::Ld if ii > 0 => {
                    out.extend(rows.drain(..).map(Line::from));
                    rows.push(vec![Span::raw("  ┃")]);
                    cur = 0;
                }
                Op::Or => {
                    rows.push(vec![Span::raw("  ┗")]);
                    cur = rows.len() - 1;
                }
                _ => {
                    if ins.is_output() && ii > 0 && r.instrs[ii - 1].is_output() {
                        // additional output in parallel: new row aligned under the first output
                        rows.push(vec![Span::raw(format!(
                            "  ┃{}",
                            " ".repeat(out_col.saturating_sub(3))
                        ))]);
                        cur = rows.len() - 1;
                    } else if cur > 0 && !r.instrs[ii - 1].is_output() {
                        // join OR branches, then continue in series on the first row
                        let w = rows.iter().map(|x| width(x)).max().unwrap_or(0);
                        let n = rows.len();
                        for (k, row) in rows.iter_mut().enumerate() {
                            let pad = w - width(row);
                            let end = if k == 0 {
                                "─"
                            } else if k == n - 1 {
                                "┘"
                            } else {
                                "┤"
                            };
                            row.push(Span::raw(format!("{}{end}", "─".repeat(pad))));
                        }
                        cur = 0;
                    }
                }
            }
            if ins.is_output() && (ii == 0 || !r.instrs[ii - 1].is_output()) {
                out_col = width(&rows[cur]);
            }
            rows[cur].push(Span::raw("─"));
            rows[cur].push(Span::styled(txt, onoff(on)));
        }
        out.extend(rows.into_iter().map(Line::from));
    }
    out
}
