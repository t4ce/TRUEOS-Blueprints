use crate::audio::{self, Command, DEFAULT_BPM, MONITOR_FRAMES, Status};
use crate::drum;
use crate::examples::{DEFAULT_PATCH, PATCHES};
use crate::highlight::Highlight;
use crate::mcp::{self, Session, SharedSession};
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Chart, Clear, Dataset, GraphType, List, ListState, Paragraph, Sparkline, Wrap,
};
use ratatui::{DefaultTerminal, Frame};
use ratatui_textarea::{CursorMove, TextArea};
use rustfft::{FftPlanner, num_complex::Complex32};
use std::error::Error;
use std::io;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, Receiver, Sender};

pub fn run() -> Result<(), Box<dyn Error>> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("the editor requires a terminal".into());
    }
    let session = Arc::new(Mutex::new(Session::new(
        DEFAULT_PATCH.trim_end_matches('\n').to_owned(),
    )));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()?;
    let endpoint = runtime.block_on(mcp::start(session.clone()));
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let mut lease = trueos::vshell::terminal_initial_lease()
        .map_err(|error| io::Error::other(error.to_string()))?;

    let (commands_tx, commands_rx) = mpsc::channel(32);
    let (status_tx, status_rx) = mpsc::channel(32);
    let (monitor_tx, monitor_rx) = mpsc::channel(1);

    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut terminal = ratatui::try_init()?;
        let result = (|| -> Result<(), Box<dyn Error>> {
            terminal.backend_mut().execute(EnableBracketedPaste)?;
            let (ui_result, audio_result) = runtime.block_on(async {
                tokio::join!(
                    async {
                        let result = editor(
                            &mut terminal,
                            &commands_tx,
                            status_rx,
                            monitor_rx,
                            &session,
                            endpoint.as_ref().err(),
                            || {
                                #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                                lease
                                    .acknowledge_ready()
                                    .map_err(|error| io::Error::other(error.to_string()))?;
                                Ok(())
                            },
                        )
                        .await;
                        drop(commands_tx);
                        result
                    },
                    audio::serve(commands_rx, status_tx, monitor_tx)
                )
            });
            terminal.backend_mut().execute(DisableBracketedPaste)?;
            ui_result?;
            audio_result
        })();
        ratatui::restore();
        result
    })();

    if let Ok(endpoint) = endpoint {
        endpoint.abort();
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    lease
        .release_to_shell()
        .map_err(|error| io::Error::other(error.to_string()))?;
    result
}

async fn editor(
    terminal: &mut DefaultTerminal,
    commands: &Sender<Command>,
    mut status: Receiver<Status>,
    mut monitor: Receiver<[f32; MONITOR_FRAMES]>,
    session: &SharedSession,
    mcp_error: Option<&io::Error>,
    first_frame: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut first_frame = Some(first_frame);
    let drum_menu = drum::register();
    let mut patch = patch_editor(&session.lock().expect("Lico session").draft);
    let mut highlight = None;
    let mut review: Option<(String, u16)> = None;
    let mut examples = None;

    let transform = FftPlanner::new().plan_fft_forward(MONITOR_FRAMES);
    let mut fft_data = [Complex32::default(); MONITOR_FRAMES];
    let mut fft_scratch = vec![Complex32::default(); transform.get_inplace_scratch_len()];
    let window: [f32; MONITOR_FRAMES] = std::array::from_fn(|i| {
        let phase = std::f32::consts::TAU * i as f32 / (MONITOR_FRAMES - 1) as f32;
        0.5 * (1.0 - phase.cos())
    });
    let mut waveform = [(0., 0.); MONITOR_FRAMES];
    let mut spectrum = [0; MONITOR_FRAMES / 2];
    let mut playing = false;
    let mut state = "Opening audio".to_owned();
    let mut error = None;
    let mut sample_rate = 0;
    let mut bpm = DEFAULT_BPM;
    let docs: serde_json::Value = serde_json::from_str(include_str!("../assets/glicol-api.json"))
        .map_err(io::Error::other)?;
    let mut reference = mcp_error.map(|error| format!("Codex connection: {error}"));
    let mut insert_drum = false;

    loop {
        let menu_selected = drum_menu && drum::take_add_drum();
        if (std::mem::take(&mut insert_drum) || menu_selected)
            && review.is_none()
            && examples.is_none()
        {
            add_drum(&mut patch, session);
            highlight = None;
            error = None;
            reference = Some("Drum added. Alt+R to play.".into());
        }
        while let Ok(update) = status.try_recv() {
            match update {
                Status::Ready(rate) => {
                    sample_rate = rate;
                    state = "Ready".into();
                }
                Status::Tempo(value) => {
                    bpm = value;
                    session.lock().expect("Lico session").bpm = value;
                }
                Status::PatchParsed(code, spans) => {
                    if session.lock().expect("Lico session").draft == code {
                        highlight = Some((
                            Highlight::new(&code, &spans, patch.tab_length()),
                            Instant::now() + Duration::from_millis(300),
                        ));
                    }
                }
                Status::Playing(code) => {
                    session.lock().expect("Lico session").playing = Some(code);
                    playing = true;
                    state = "Playing".into();
                    error = None;
                }
                Status::Stopped => {
                    session.lock().expect("Lico session").playing = None;
                    playing = false;
                    state = "Stopped".into();
                    error = None;
                    waveform.fill((0., 0.));
                    spectrum.fill(0);
                }
                Status::PatchError(message) => error = Some(message),
                Status::AudioError(message) => {
                    session.lock().expect("Lico session").playing = None;
                    playing = false;
                    state = "Audio unavailable".into();
                    error = Some(message);
                    waveform.fill((0., 0.));
                    spectrum.fill(0);
                }
            }
        }
        if let Ok(samples) = monitor.try_recv() {
            if playing {
                for (i, sample) in samples.into_iter().enumerate() {
                    waveform[i] = (i as f64, f64::from(sample));
                    fft_data[i] = Complex32::new(sample * window[i], 0.0);
                }
                transform.process_with_scratch(&mut fft_data, &mut fft_scratch);
                for (level, bin) in spectrum.iter_mut().zip(fft_data) {
                    let amplitude = (4.0 * bin.norm() / MONITOR_FRAMES as f32).max(1.0e-4);
                    *level = (80.0 + 20.0 * amplitude.log10()).clamp(0.0, 80.0) as u64;
                }
            }
        }

        let pending = session.lock().expect("Lico session").proposal.is_some();
        terminal.draw(|frame| {
            let area = frame.area();
            if area.height < 25 {
                draw_patch(frame, &patch, area, highlight.as_ref());
            } else {
                let wide = area.width >= 80;
                let [header, edit, plots, status, footer] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(4),
                    if wide {
                        Constraint::Length(0)
                    } else {
                        Constraint::Percentage(30)
                    },
                    Constraint::Length(if error.is_some() || reference.is_some() {
                        7
                    } else {
                        3
                    }),
                    Constraint::Length(1),
                ])
                .areas(area);
                let (edit, plots) = if wide {
                    let [edit, plots] = Layout::horizontal([
                        Constraint::Percentage(65),
                        Constraint::Percentage(35),
                    ])
                    .areas(edit);
                    (
                        edit,
                        Layout::vertical([Constraint::Ratio(1, 2); 2]).areas(plots),
                    )
                } else {
                    (
                        edit,
                        Layout::horizontal([Constraint::Ratio(1, 2); 2]).areas(plots),
                    )
                };
                frame.render_widget(
                    Line::from(vec![
                        Span::styled(
                            " LICO ",
                            Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
                        ),
                        Span::raw(r"  Samples  \808bd  \808sd  \808ch  \blip"),
                        Span::raw(if pending {
                            "  • Alt+P review Codex edit"
                        } else if mcp_error.is_none() {
                            "  • Codex ready"
                        } else {
                            "  • Codex unavailable"
                        }),
                    ]),
                    header,
                );
                draw_patch(frame, &patch, edit, highlight.as_ref());
                draw_plots(frame, plots, &waveform, &spectrum, sample_rate);
                let label = error.as_deref().or(reference.as_deref()).unwrap_or(&state);
                let color = if error.is_some() {
                    Color::Red
                } else {
                    Color::Green
                };
                frame.render_widget(
                    Paragraph::new(label)
                        .style(Style::default().fg(color))
                        .wrap(Wrap { trim: false })
                        .block(
                            Block::bordered()
                                .title(format!(" {state} · {bpm} BPM · {sample_rate} Hz ")),
                        ),
                    status,
                );
                frame.render_widget(
                    Line::from(
                        " Alt+R Run  Alt+S Stop  Alt+E Examples  Alt+D Docs  Alt+[/] Tempo  Esc Quit ",
                    ),
                    footer,
                );
            }
            if let Some(selection) = &mut examples {
                draw_examples(frame, selection);
            }
            if let Some((diff, scroll)) = &review {
                let area = frame.area().inner(ratatui::layout::Margin {
                    horizontal: 2,
                    vertical: 1,
                });
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(diff.as_str())
                        .wrap(Wrap { trim: false })
                        .scroll((*scroll, 0))
                        .block(
                            Block::bordered()
                                .title(" Codex proposal · Alt+A accept · Alt+X reject · Esc back "),
                        ),
                    area,
                );
            }
        })?;

        if let Some(ready) = first_frame.take() {
            ready()?;
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
        if !event::poll(Duration::ZERO)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                if let Some(selection) = &mut examples {
                    match key.code {
                        KeyCode::Esc => examples = None,
                        KeyCode::Up => selection.select_previous(),
                        KeyCode::Down => selection.select_next(),
                        KeyCode::Home => selection.select_first(),
                        KeyCode::End => selection.select_last(),
                        KeyCode::Enter => {
                            if let Some(&(name, code)) =
                                selection.selected().and_then(|index| PATCHES.get(index))
                            {
                                let code = code.trim_end_matches('\n');
                                patch = patch_editor(code);
                                highlight = None;
                                session
                                    .lock()
                                    .expect("Lico session")
                                    .update_draft(code.to_owned());
                                reference = Some(format!("{name} loaded. Alt+R to play."));
                                error = None;
                            }
                            examples = None;
                        }
                        _ => {}
                    }
                    continue;
                }
                if let Some((_, scroll)) = &mut review {
                    match key.code {
                        KeyCode::Esc => review = None,
                        KeyCode::Up => *scroll = scroll.saturating_sub(1),
                        KeyCode::Down => *scroll = scroll.saturating_add(1),
                        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                        KeyCode::Char('a') if alt => {
                            let mut session = session.lock().expect("Lico session");
                            if let Some(proposal) = session.proposal.take() {
                                patch = patch_editor(&proposal.code);
                                highlight = None;
                                session.update_draft(patch.lines().join("\n"));
                                reference = Some(
                                    "Proposal accepted. Alt+R to play the edited patch.".into(),
                                );
                                error = None;
                            }
                            review = None;
                        }
                        KeyCode::Char('x') if alt => {
                            session.lock().expect("Lico session").proposal = None;
                            review = None;
                        }
                        _ => {}
                    }
                    continue;
                }
                let command = match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('q') if control => break,
                    KeyCode::Char('r') if alt => Some(Command::Run(patch.lines().join("\n"))),
                    KeyCode::Char('s') if alt => Some(Command::Stop),
                    KeyCode::Char('i') if alt => {
                        insert_drum = true;
                        None
                    }
                    KeyCode::Char('e') if alt => {
                        examples = Some(ListState::default().with_selected(Some(0)));
                        None
                    }
                    KeyCode::Char('p') if alt && pending => {
                        let session = session.lock().expect("Lico session");
                        if let Some(proposal) = &session.proposal {
                            review = Some((proposal_diff(&session.draft, proposal), 0));
                        }
                        None
                    }
                    KeyCode::Char('[') if alt => Some(Command::ChangeTempo(-5)),
                    KeyCode::Char(']') if alt => Some(Command::ChangeTempo(5)),
                    KeyCode::Char('d') if alt => {
                        error = None;
                        reference = Some(node_reference(&patch, &docs));
                        None
                    }
                    KeyCode::F(5) => Some(Command::Run(patch.lines().join("\n"))),
                    KeyCode::Enter if control => Some(Command::Run(patch.lines().join("\n"))),
                    KeyCode::F(6) => Some(Command::Stop),
                    KeyCode::Char('.') if control && key.modifiers.contains(KeyModifiers::ALT) => {
                        Some(Command::Stop)
                    }
                    _ => {
                        reference = None;
                        if patch.input(key) {
                            highlight = None;
                            session
                                .lock()
                                .expect("Lico session")
                                .update_draft(patch.lines().join("\n"));
                        }
                        None
                    }
                };
                if let Some(mut command) = command {
                    if let Command::Run(code) = &mut command
                        && let Some(converted) = drum::display_alias(code)
                    {
                        let cursor = patch.cursor();
                        patch = patch_editor(&converted);
                        patch.move_cursor(CursorMove::Jump(
                            cursor.0.min(u16::MAX as usize) as u16,
                            cursor.1.min(u16::MAX as usize) as u16,
                        ));
                        highlight = None;
                        session
                            .lock()
                            .expect("Lico session")
                            .update_draft(converted.clone());
                        *code = converted;
                    }
                    reference = None;
                    let _ = commands.send(command).await;
                }
            }
            Event::Paste(text) if review.is_none() && examples.is_none() => {
                reference = None;
                if patch.insert_str(text) {
                    highlight = None;
                    session
                        .lock()
                        .expect("Lico session")
                        .update_draft(patch.lines().join("\n"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn add_drum(patch: &mut TextArea<'static>, session: &SharedSession) {
    let addition = drum::track_to_append(&patch.lines().join("\n"));
    patch.cancel_selection();
    patch.move_cursor(CursorMove::Bottom);
    patch.move_cursor(CursorMove::End);
    patch.insert_str(addition);
    session
        .lock()
        .expect("Lico session")
        .update_draft(patch.lines().join("\n"));
}

fn patch_editor(code: &str) -> TextArea<'static> {
    let mut patch = TextArea::new(code.split('\n').map(str::to_owned).collect());
    patch.set_block(Block::bordered().title(" Patch ").border_style(Color::Cyan));
    patch.set_line_number_style(Style::default().fg(Color::DarkGray));
    patch.set_cursor_line_style(Style::default());
    patch
}

fn draw_patch(
    frame: &mut Frame,
    patch: &TextArea<'_>,
    area: Rect,
    highlight: Option<&(Highlight, Instant)>,
) {
    frame.render_widget(patch, area);
    let Some((highlight, outline_until)) = highlight else {
        return;
    };
    let text_area = patch.block().map_or(area, |block| block.inner(area));
    let (top, left) = patch.scroll_offset();
    let left = usize::from(left);
    let gutter = usize::from(patch.line_number_width());
    let width = usize::from(text_area.width);
    let buffer = frame.buffer_mut();
    for (y, line) in
        (text_area.top()..text_area.bottom()).zip(highlight.lines.iter().skip(usize::from(top)))
    {
        for span in line {
            let start = (gutter + span.start).saturating_sub(left).min(width);
            let end = (gutter + span.end).saturating_sub(left).min(width);
            for column in start..end {
                let cell = &mut buffer[(text_area.x + column as u16, y)];
                if cell.bg == Color::Reset && !cell.modifier.contains(Modifier::REVERSED) {
                    cell.fg = span.color;
                }
            }
        }
    }
    if !area.is_empty() && Instant::now() < *outline_until {
        let bottom = area.bottom() - 1;
        let right = area.right() - 1;
        let style = Style::default().fg(Color::LightGreen);
        for edge in [
            Rect::new(area.x, area.y, area.width, 1),
            Rect::new(area.x, bottom, area.width, 1),
            Rect::new(area.x, area.y + 1, 1, area.height.saturating_sub(2)),
            Rect::new(right, area.y + 1, 1, area.height.saturating_sub(2)),
        ] {
            buffer.set_style(edge, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use ratatui_textarea::CursorMove;

    #[test]
    fn highlights_follow_scrolling_and_keep_the_cursor_and_selection_visible() {
        let code = "out: sin 440 >> mul 0.25;";
        let (_, spans) = glicol_parser::get_ast_with_spans(code).unwrap();
        let mut patch = patch_editor(code);
        patch.set_selection_style(Style::default().bg(Color::Blue));
        let mut highlight = (
            Highlight::new(code, &spans, patch.tab_length()),
            Instant::now() + Duration::from_secs(1),
        );
        let mut terminal = Terminal::new(TestBackend::new(18, 4)).unwrap();

        patch.move_cursor(CursorMove::Jump(0, 5));
        terminal
            .draw(|frame| draw_patch(frame, &patch, frame.area(), Some(&highlight)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].fg, Color::LightGreen);
        assert_eq!(buffer[(9, 1)].symbol(), "s");
        assert!(buffer[(9, 1)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buffer[(10, 1)].fg, Color::LightBlue);

        patch.move_cursor(CursorMove::Jump(0, 22));
        patch.start_selection();
        patch.move_cursor(CursorMove::End);
        highlight.1 = Instant::now();
        terminal
            .draw(|frame| draw_patch(frame, &patch, frame.area(), Some(&highlight)))
            .unwrap();
        assert!(patch.scroll_offset().1 > 0);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].fg, Color::Cyan);
        let cells = (1..17).map(|x| &buffer[(x, 1)]).collect::<Vec<_>>();
        assert!(
            cells
                .iter()
                .any(|cell| cell.symbol() == "0" && cell.fg == Color::LightYellow)
        );
        assert!(
            cells
                .iter()
                .any(|cell| cell.symbol() == "5" && cell.bg == Color::Blue)
        );
    }
}

fn proposal_diff(draft: &str, proposal: &mcp::Proposal) -> String {
    use std::fmt::Write;
    let before: Vec<_> = draft.lines().collect();
    let after: Vec<_> = proposal.code.lines().collect();
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut diff = format!("{}\n\nChanges from line {}\n", proposal.summary, prefix + 1);
    for line in &before[prefix..before.len() - suffix] {
        let _ = writeln!(diff, "- {line}");
    }
    for line in &after[prefix..after.len() - suffix] {
        let _ = writeln!(diff, "+ {line}");
    }
    diff
}

fn node_reference(patch: &TextArea<'_>, docs: &serde_json::Value) -> String {
    let cursor = patch.cursor();
    let line = &patch.lines()[cursor.0];
    let byte = line
        .char_indices()
        .nth(cursor.1)
        .map_or(line.len(), |(index, _)| index);
    let is_separator = |ch: char| !(ch.is_ascii_alphanumeric() || ch == '_');
    let left = line[..byte].rsplit(is_separator).next().unwrap_or_default();
    let right = line[byte..].split(is_separator).next().unwrap_or_default();
    let node = format!("{left}{right}");
    if let Some(description) = docs[&node]["description"].as_str() {
        let example = docs[&node]["example"].as_str().unwrap_or_default();
        format!("{node} — {description}\n{example}")
    } else {
        "Move the cursor onto a node name and press Alt+D.".to_owned()
    }
}

fn draw_plots(
    frame: &mut Frame,
    [scope, frequencies]: [Rect; 2],
    waveform: &[(f64, f64)],
    spectrum: &[u64],
    sample_rate: u32,
) {
    let trace = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(waveform);
    frame.render_widget(
        Chart::new(vec![trace])
            .block(Block::bordered().title(" Waveform · L "))
            .x_axis(Axis::default().bounds([0.0, (MONITOR_FRAMES - 1) as f64]))
            .y_axis(Axis::default().bounds([-1.0, 1.0])),
        scope,
    );

    let block = Block::bordered().title(format!(" Spectrum · 0–{} Hz ", sample_rate / 2));
    let columns = usize::from(block.inner(frequencies).width).min(spectrum.len());
    let peaks: Vec<_> = (0..columns)
        .map(|i| {
            let start = i * spectrum.len() / columns;
            let end = (i + 1) * spectrum.len() / columns;
            spectrum[start..end].iter().copied().max().unwrap_or(0)
        })
        .collect();
    frame.render_widget(
        Sparkline::default()
            .block(block)
            .data(&peaks)
            .max(80)
            .style(Style::default().fg(Color::Magenta)),
        frequencies,
    );
}

fn draw_examples(frame: &mut Frame, selection: &mut ListState) {
    let area = frame.area();
    let width = area.width.min(43);
    let height = area.height.min(PATCHES.len() as u16 + 2);
    let picker = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, picker);
    frame.render_stateful_widget(
        List::new(PATCHES.iter().map(|&(name, _)| name))
            .block(
                Block::bordered()
                    .title(" Examples ")
                    .title_bottom(" Enter replace draft · Esc back "),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("› "),
        picker,
        selection,
    );
}
