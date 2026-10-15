//! Raw control sequences deliberately bypass Ratatui's buffer to expose terminal gaps.
use crate::ui::App;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
};
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

pub const RESTORE: &str = "\x1b[0m\x1b[r\x1b[?2026l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[>4;0m\x1b[0 q\x1b[?25h";

pub fn render(frame: &mut Frame, area: Rect, index: usize, app: &App) {
    let names = [
        "Cursor, erase, color and inverse",
        "Protected history / scroll region",
        "Capability queries / cursor safety",
        "Keyboard, mouse, focus and paste",
        "Input event inspector",
    ];
    let details = [
        "Raw specimens: CUP, CHA, CUU/CUD, DECSC/DECRC, CSI s/u, ED/EL, ICH/DCH/ECH and DECSCUSR.\n\nExpected: inverted text swaps colors; SGR 27 restores colors. Erase leaves styled blanks. Character editing transforms ABCDE to the printed target.\n\nICH/DCH/ECH now operate within the current native row. Cursor shapes remain a native gap. On SSH your local terminal handles the sequences.",
        "A transcript repeatedly inserts lines into a restricted scroll region. A header above and composer below stay fixed.\n\nThis is the key Codex inline-history pattern (insert_history.rs). A scroll-region reset is always sent before returning.\n\nUse --inline for a separate Ratatui inline viewport lifecycle specimen.",
        "Queries: CSI 6n (cursor report), CSI c / >c (primary/secondary identity), OSC10/11 (default colors), CSI ?u (keyboard capabilities).\n\nThe saved-cursor anchor must not change when unsupported CSI >u, <u, ?u, or DECSCUSR controls are received.\n\nNative replies: CPR / primary DA. Default-color queries and enhanced keyboard capabilities are presently unsupported. Unsupported queries must remain quiet.",
        "Enables bracketed paste, focus events, button/drag/SGR mouse, Kitty keyboard push flags 7 and modifyOtherKeys. Input events appear in the raw view. Esc returns; a 20-second timeout also restores modes.\n\nNative mouse encodings work; focus/paste/enhanced-key support depends on frontend. SSH passes sequences through to your terminal.\n\nOSC8 link and synchronized output are included. Clipboard/notification/pointer extensions (OSC52/9/22/7501) are documented in README, without modifying your clipboard.",
        "Recent events (debug strings escape control characters). Press a to enable enhanced keyboard/mouse/focus; i to edit.\n\nTry Shift-Enter, Ctrl/Alt keys, held keys, bracketed paste, wheel, drag, focus changes and resize. On SSH these are events from the host terminal; native UI4 uses TRUEOS input translation.",
    ];
    let mut text = format!(
        "{}\n\n{}\n\nCompleted raw runs: {}\n\n",
        names[index], details[index], app.probe_runs
    );
    if !app.input.is_empty() {
        text.push_str(&format!("Input: {:?}\n\n", app.input));
    }
    for event in &app.events {
        text.push_str(event);
        text.push('\n');
    }
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Gray))
            .block(Block::bordered().title(" a opens raw probe · Esc returns ")),
        area,
    );
}

pub fn specimen(index: usize, cols: u16, rows: u16) -> String {
    let mut out = String::from("\x1b[0m\x1b[r\x1b[2J\x1b[H\x1b[?25lRATATEST RAW ANSI PROBE");
    if cols < 40 || rows < 18 {
        out.push_str("\r\nResize to at least 40 columns x 18 rows. Esc returns.");
        return out;
    }
    match index {
        0 => {
            out.push_str(
                "\x1b[3;1HSGR 7/27: \x1b[33;44mnormal \x1b[7minverse\x1b[27m normal\x1b[0m reset",
            );
            out.push_str(
                "\x1b[4;1HSGR 38/48 RGB: \x1b[38;2;100;210;255;48;2;25;40;65m RGB sample \x1b[0m",
            );
            out.push_str("\x1b[5;1HIndexed colors: \x1b[38;5;196mred \x1b[38;5;46mgreen \x1b[38;5;33mblue\x1b[0m");
            out.push_str("\x1b[7;1HErase K: stale suffix\x1b[7;10Hnew\x1b[K\x1b[8;1HExpected previous row: Erase K: new");
            out.push_str("\x1b[10;1HABCDE\x1b[10;3H\x1b[2P\x1b[11;1HDCH expected: ABE");
            out.push_str("\x1b[12;1HABCDE\x1b[12;3H\x1b[2@XY\x1b[13;1HICH expected: ABXYCDE");
            out.push_str("\x1b[14;1HABCDE\x1b[14;3H\x1b[2X\x1b[15;1HECH expected: AB  E");
            out.push_str("\x1b[16;1HSaved cursor: \x1b[s\x1b[2;1Hsave/restore active\x1b[uOK");
            out.push_str("\x1b[17;1HCursor style: bar\x1b[6 q\x1b[?25h");
        }
        1 => {
            out.push_str("\x1b[3;1HFIXED HEADER - must not scroll");
            let bottom = rows - 5;
            out.push_str(&format!(
                "\x1b[{};1HFIXED COMPOSER > keep this text\x1b[6;{bottom}r\x1b[{bottom};1H",
                bottom + 2
            ));
            for line in 1..=24 {
                out.push_str(&format!("history line {line:02}\r\n"));
            }
            out.push_str("\x1b[r");
        }
        2 => {
            out.push_str("\x1b[3;1HNo query may overwrite the ANCHOR below.");
            out.push_str(
                "\x1b[5;1Hsaved position:\x1b[s\x1b[7;1HANCHOR:\x1b[?u\x1b[>0u\x1b[<u\x1b[0 qOK",
            );
            out.push_str("\x1b[9;1HExpected: ANCHOR:OK; no OK beside saved position.");
            out.push_str("\x1b[11;1HQueries sent: CPR / DA1 / DA2 / OSC10 / OSC11 / CSI ?u");
            out.push_str("\x1b[6n\x1b[c\x1b[>c\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[?u");
        }
        _ => {
            out.push_str(
                "\x1b[?2026h\x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h\x1b[>7u\x1b[>4;2m",
            );
            out.push_str("\x1b[3;1HType, paste, resize, focus, click, drag or use wheel.");
            out.push_str("\x1b[5;1HOSC8: \x1b]8;;https://ratatui.rs\x1b\\Ratatui hyperlink\x1b]8;;\x1b\\ (click support varies)");
            out.push_str(
                "\x1b[7;1HSynchronized update should appear as one completed frame.\x1b[?2026l",
            );
        }
    }
    out.push_str(&format!(
        "\x1b[{};1HEsc returns / 20 second timeout (modes restored)",
        rows - 1
    ));
    out
}

pub fn run(out: &mut impl Write, index: usize) -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let result = (|| {
        out.write_all(specimen(index, cols, rows).as_bytes())?;
        out.flush()?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20) {
            if !event::poll(Duration::from_millis(80))? {
                continue;
            }
            let event = event::read()?;
            if matches!(&event, Event::Key(key) if key.kind != KeyEventKind::Release && matches!(key.code, KeyCode::Esc | KeyCode::Char('q')))
            {
                break;
            }
            let label = format!("{event:?}");
            let label: String = label
                .chars()
                .take(cols.saturating_sub(1) as usize)
                .collect();
            write!(
                out,
                "\x1b[{};1H\x1b[2K{label}",
                rows.saturating_sub(2).max(1)
            )?;
            out.flush()?;
        }
        Ok(())
    })();
    // Pop only the keyboard flags pushed by this probe, then restore owned modes.
    if index >= 3 && cols >= 40 && rows >= 18 {
        let _ = out.write_all(b"\x1b[<u");
    }
    let _ = out.write_all(RESTORE.as_bytes());
    let _ = out.write_all(b"\x1b[?2004h"); // App event loop continues to accept bracketed paste.
    let _ = out.flush();
    result
}
