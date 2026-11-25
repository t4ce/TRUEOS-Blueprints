use crate::audio::{self, Command, DEFAULT_BPM, MONITOR_FRAMES, MonitorFrame, Status};
use crate::drum;
use crate::examples::{DEFAULT_PATCH, PATCHES};
use crate::highlight::Highlight;
use crate::mcp::{self, Session, SharedSession};
use crate::midi;
use crate::playhead;
use crate::visualizer::Visualizer;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::{
    execute, queue,
    style::Print,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
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
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot, watch,
};

enum Picker {
    Examples(ListState),
    Midi(ListState),
}

struct PatchUndo {
    editor: TextArea<'static>,
    applied: String,
    playing: Option<String>,
}

pub fn run(midi_input: midi::Input) -> Result<(), Box<dyn Error>> {
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
    let (monitor_tx, monitor_rx) = watch::channel(MonitorFrame::default());

    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut terminal = ratatui::try_init()?;
        let result = (|| -> Result<(), Box<dyn Error>> {
            // Request SGR button reports so pointer motion does not fill the input queue.
            execute!(
                terminal.backend_mut(),
                EnableBracketedPaste,
                Print("\x1b[?1006h\x1b[?1000h")
            )?;
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
                    audio::serve(commands_rx, status_tx, monitor_tx, midi_input)
                )
            });
            ui_result?;
            audio_result
        })();
        let reset = execute!(
            terminal.backend_mut(),
            EndSynchronizedUpdate,
            Print("\x1b[?1000l\x1b[?1006l"),
            DisableBracketedPaste
        );
        ratatui::restore();
        result?;
        reset?;
        Ok(())
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
    mut monitor: watch::Receiver<MonitorFrame>,
    session: &SharedSession,
    mcp_error: Option<&io::Error>,
    first_frame: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut first_frame = Some(first_frame);
    let drum_menu = drum::register();
    let mut patch = patch_editor(&session.lock().expect("Lico session").draft);
    let mut highlight = None;
    let mut active_run = 0;
    let mut sequence_slots = Vec::new();
    let mut outlines = Vec::new();
    let mut published_outlines = Vec::new();
    let mut published_area = Rect::default();
    let mut undo_patch: Option<PatchUndo> = None;
    let mut picker = None;
    let mut midi_source = midi::Source::Off;
    let mut midi_error = None;
    let mut visualizer = Visualizer::new(monitor.clone());

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
        if let Some(message) = visualizer.take_status() {
            reference = Some(message);
        }
        let menu_selected = drum_menu && drum::take_add_drum();
        if (std::mem::take(&mut insert_drum) || menu_selected) && picker.is_none() {
            add_drum(&mut patch, session);
            highlight = None;
            error = None;
            reference = Some("Drum added. Alt+R to play.".into());
        }
        while let Ok(update) = status.try_recv() {
            match update {
                Status::Ready {
                    rate,
                    midi_source: source,
                } => {
                    sample_rate = rate;
                    midi_source = source;
                    state = "Ready".into();
                }
                Status::Tempo(value) => {
                    bpm = value;
                    session.lock().expect("Lico session").bpm = value;
                }
                Status::PatchParsed { run, code, source } => {
                    active_run = run;
                    sequence_slots.clear();
                    if session.lock().expect("Lico session").draft == code {
                        highlight = Some((
                            Highlight::new(&code, &source.spans, patch.tab_length()),
                            Instant::now() + Duration::from_millis(300),
                        ));
                        if cfg!(any(target_os = "trueos", target_os = "zkvm")) {
                            sequence_slots = Highlight::sequence_slots(
                                &code,
                                &source.sequences,
                                patch.tab_length(),
                            );
                        }
                    }
                }
                Status::Playing(code) => {
                    session.lock().expect("Lico session").playing = Some(code);
                    playing = true;
                    state = "Playing".into();
                    error = None;
                }
                Status::Midi(message) => reference = Some(message),
                Status::MidiInput { source, error } => {
                    midi_source = source;
                    reference = Some(
                        error
                            .clone()
                            .unwrap_or_else(|| format!("MIDI input · {}", source.label())),
                    );
                    midi_error = error;
                    if midi_error.is_none() && matches!(picker, Some(Picker::Midi(_))) {
                        picker = None;
                    }
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
        let incoming = session.lock().expect("Lico session").take_patch();
        if let Some(request) = incoming {
            let code = drum::display_alias(&request.code).unwrap_or(request.code);
            let result = match restart_patch(commands, &code).await {
                Ok(previous_playing) => {
                    undo_patch = Some(PatchUndo {
                        editor: std::mem::replace(&mut patch, patch_editor(&code)),
                        applied: code.clone(),
                        playing: previous_playing,
                    });
                    highlight = None;
                    error = None;
                    reference = Some(format!("Codex applied · {}", request.summary));
                    let mut session = session.lock().expect("Lico session");
                    session.playing = Some(code.clone());
                    Ok(session.update_draft(code))
                }
                Err(message) => {
                    error = Some(message.clone());
                    Err(message)
                }
            };
            let _ = request.reply.send(result);
        }
        if monitor.has_changed().unwrap_or(false) {
            let frame = monitor.borrow_and_update();
            if playing && frame.run == active_run {
                for (i, sample) in frame.waveform.into_iter().enumerate() {
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

        let mut editor_area = Rect::default();
        if playing {
            queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        }
        let drawn = terminal.draw(|frame| {
            let area = frame.area();
            if area.height < 25 {
                editor_area = area;
                draw_patch(frame, &patch, editor_area, highlight.as_ref());
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
                    // Reserve room for diagnostics so they never move the plot frames.
                    Constraint::Length(7),
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
                        Span::raw(if mcp_error.is_none() {
                            "  • Codex ready"
                        } else {
                            "  • Codex unavailable"
                        }),
                    ]),
                    header,
                );
                editor_area = edit;
                draw_patch(frame, &patch, editor_area, highlight.as_ref());
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
                        " Ctrl+U Undo  Alt+R Run  Alt+S Stop  Alt+E Examples  Alt+V Visualizer  Alt+N MIDI  Alt+D Docs  Alt+[/] Tempo  Ctrl+Q Quit ",
                    ),
                    footer,
                );
            }
            match &mut picker {
                Some(Picker::Examples(selection)) => draw_examples(frame, selection),
                Some(Picker::Midi(selection)) => {
                    draw_midi_picker(frame, selection, midi_source, midi_error.as_deref());
                }
                None => {}
            }
        }).map(|frame| frame.area);

        if let Ok(area) = drawn.as_ref()
            && cfg!(any(target_os = "trueos", target_os = "zkvm"))
        {
            outlines.clear();
            if playing && highlight.is_some() && picker.is_none() {
                let frame = monitor.borrow();
                if frame.run == active_run {
                    playhead::visible_outlines(
                        &mut outlines,
                        &patch,
                        editor_area,
                        &sequence_slots,
                        &frame.sequences,
                    );
                }
            }
            if (outlines != published_outlines || (!outlines.is_empty() && *area != published_area))
                && playhead::publish(&outlines)
            {
                published_area = *area;
                std::mem::swap(&mut outlines, &mut published_outlines);
            }
        }

        // Close the fence even if drawing failed. Shell3 can otherwise see
        // plot cells and cursor moves from a partially written terminal frame.
        let commit = if playing {
            execute!(terminal.backend_mut(), EndSynchronizedUpdate)
        } else {
            Ok(())
        };
        drawn?;
        commit?;

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
                if let Some(open) = &mut picker {
                    let selection = match open {
                        Picker::Examples(selection) | Picker::Midi(selection) => selection,
                    };
                    match key.code {
                        KeyCode::Esc => picker = None,
                        KeyCode::Up => selection.select_previous(),
                        KeyCode::Down => selection.select_next(),
                        KeyCode::Home => selection.select_first(),
                        KeyCode::End => selection.select_last(),
                        KeyCode::Enter => match open {
                            Picker::Examples(selection) => {
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
                                picker = None;
                            }
                            Picker::Midi(selection) => {
                                if let Some(&source) = selection
                                    .selected()
                                    .and_then(|index| midi::Source::CHOICES.get(index))
                                {
                                    midi_error = None;
                                    let _ = commands.send(Command::SelectMidiInput(source)).await;
                                }
                            }
                        },
                        _ => {}
                    }
                    continue;
                }
                if control
                    && !alt
                    && key.code == KeyCode::Char('u')
                    && let Some(undo) = undo_patch
                        .take_if(|undo| session.lock().expect("Lico session").draft == undo.applied)
                {
                    let result = match undo.playing.as_deref() {
                        Some(code) => restart_patch(commands, code).await.map(|_| ()),
                        None => commands
                            .send(Command::Stop)
                            .await
                            .map_err(|_| "Lico audio closed".into()),
                    };
                    match result {
                        Ok(()) => {
                            patch = undo.editor;
                            highlight = None;
                            error = None;
                            reference = Some("Codex patch undone".into());
                            let mut session = session.lock().expect("Lico session");
                            session.playing = undo.playing;
                            session.update_draft(patch.lines().join("\n"));
                        }
                        Err(message) => {
                            undo_patch = Some(undo);
                            error = Some(message);
                        }
                    }
                    continue;
                }
                let command = match key.code {
                    KeyCode::Esc if visualizer.close_if_open() => {
                        reference = Some("Closing visualizer…".into());
                        None
                    }
                    KeyCode::Esc => break,
                    KeyCode::Char('q') if control => break,
                    KeyCode::Char('r') if alt => Some(Command::Run(patch.lines().join("\n"))),
                    KeyCode::Char('s') if alt => Some(Command::Stop),
                    KeyCode::Char('m') if alt => Some(Command::TestMidi),
                    KeyCode::Char('v') if alt => {
                        reference = Some(visualizer.toggle());
                        None
                    }
                    KeyCode::Char('i') if alt => {
                        insert_drum = true;
                        None
                    }
                    KeyCode::Char('e') if alt => {
                        picker = Some(Picker::Examples(
                            ListState::default().with_selected(Some(0)),
                        ));
                        None
                    }
                    KeyCode::Char('n') if alt => {
                        let index = midi::Source::CHOICES
                            .iter()
                            .position(|&source| source == midi_source);
                        picker = Some(Picker::Midi(ListState::default().with_selected(index)));
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
            Event::Paste(text) if picker.is_none() => {
                reference = None;
                if patch.insert_str(text) {
                    highlight = None;
                    session
                        .lock()
                        .expect("Lico session")
                        .update_draft(patch.lines().join("\n"));
                }
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                ..
            }) if picker.is_none() => {
                if place_patch_cursor(&mut patch, editor_area, Position::new(column, row)) {
                    reference = None;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn restart_patch(commands: &Sender<Command>, code: &str) -> Result<Option<String>, String> {
    let (finished, result) = oneshot::channel();
    commands
        .send(Command::Restart {
            code: code.to_owned(),
            finished,
        })
        .await
        .map_err(|_| "Lico audio closed")?;
    result.await.map_err(|_| "Lico audio closed")?
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

fn place_patch_cursor(patch: &mut TextArea<'_>, area: Rect, click: Position) -> bool {
    let text_area = patch.block().map_or(area, |block| block.inner(area));
    if !text_area.contains(click) {
        return false;
    }

    let (top, left) = patch.scroll_offset();
    let row = usize::from(click.y - text_area.y) + usize::from(top);
    let col = (usize::from(click.x - text_area.x) + usize::from(left))
        .saturating_sub(usize::from(patch.line_number_width()));
    let cursor = patch.screen_to_data(row, col);
    patch.cancel_selection();
    patch.move_cursor(CursorMove::Jump(
        cursor.0.min(u16::MAX as usize) as u16,
        cursor.1.min(u16::MAX as usize) as u16,
    ));
    true
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

    #[test]
    fn click_uses_editor_cells_and_clears_the_selection() {
        let mut patch = patch_editor("top\n\t音abc");
        patch.start_selection();
        patch.move_cursor(CursorMove::End);
        let area = Rect::new(3, 1, 18, 4);
        let mut terminal = Terminal::new(TestBackend::new(24, 6)).unwrap();
        terminal
            .draw(|frame| draw_patch(frame, &patch, area, None))
            .unwrap();

        let click = Position::new(13, 3);
        assert_eq!(terminal.backend().buffer()[click].symbol(), "a");
        assert!(!place_patch_cursor(&mut patch, area, Position::new(3, 2)));
        assert_eq!(patch.selection_range(), Some(((0, 0), (0, 3))));
        assert!(place_patch_cursor(&mut patch, area, click));
        assert_eq!(patch.cursor(), (1, 2));
        assert_eq!(patch.selection_range(), None);
        assert_eq!(patch.lines(), ["top", "\t音abc"]);
    }

    #[test]
    fn click_accounts_for_both_scroll_offsets() {
        let mut patch = patch_editor(&["0123456789abcdef"; 4].join("\n"));
        patch.move_cursor(CursorMove::Bottom);
        patch.move_cursor(CursorMove::End);
        let area = Rect::new(2, 1, 10, 5);
        let mut terminal = Terminal::new(TestBackend::new(16, 7)).unwrap();
        terminal
            .draw(|frame| draw_patch(frame, &patch, area, None))
            .unwrap();

        assert_eq!(patch.scroll_offset(), (1, 12));
        let click = Position::new(3, 2);
        assert_eq!(terminal.backend().buffer()[click].symbol(), "9");
        assert!(place_patch_cursor(&mut patch, area, click));
        assert_eq!(patch.cursor(), (1, 9));
    }

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
    let scope_frame = Block::bordered().title(" Waveform · L ");
    let scope_inner = scope_frame.inner(scope);
    frame.render_widget(scope_frame, scope);
    let trace = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(waveform);
    frame.render_widget(
        Chart::new(vec![trace])
            .x_axis(Axis::default().bounds([0.0, (MONITOR_FRAMES - 1) as f64]))
            .y_axis(Axis::default().bounds([-1.0, 1.0])),
        scope_inner,
    );

    let spectrum_frame = Block::bordered().title(format!(" Spectrum · 0–{} Hz ", sample_rate / 2));
    let spectrum_inner = spectrum_frame.inner(frequencies);
    frame.render_widget(spectrum_frame, frequencies);
    let columns = usize::from(spectrum_inner.width).min(spectrum.len());
    let peaks: Vec<_> = (0..columns)
        .map(|i| {
            let start = i * spectrum.len() / columns;
            let end = (i + 1) * spectrum.len() / columns;
            spectrum[start..end].iter().copied().max().unwrap_or(0)
        })
        .collect();
    frame.render_widget(
        Sparkline::default()
            .data(&peaks)
            .max(80)
            .style(Style::default().fg(Color::Magenta)),
        spectrum_inner,
    );
}

fn draw_examples(frame: &mut Frame, selection: &mut ListState) {
    let picker = picker_area(frame.area(), 43, PATCHES.len() as u16 + 2);
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

fn draw_midi_picker(
    frame: &mut Frame,
    selection: &mut ListState,
    active: midi::Source,
    error: Option<&str>,
) {
    let message_height = if error.is_some() { 3 } else { 0 };
    let picker = picker_area(
        frame.area(),
        50,
        midi::Source::CHOICES.len() as u16 + 2 + message_height,
    );
    let block = Block::bordered()
        .title(" MIDI input ")
        .title_bottom(" Enter select · Esc back ");
    let [options, message] = Layout::vertical([
        Constraint::Min(midi::Source::CHOICES.len() as u16),
        Constraint::Length(message_height),
    ])
    .areas(block.inner(picker));
    frame.render_widget(Clear, picker);
    frame.render_widget(block, picker);
    let labels = midi::Source::CHOICES.iter().map(|&source| {
        let label = source.label();
        if source == active {
            format!("{label} · Active")
        } else {
            label.into()
        }
    });
    frame.render_stateful_widget(
        List::new(labels)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
            .highlight_symbol("› "),
        options,
        selection,
    );
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(error)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::Red)),
            message,
        );
    }
}

fn picker_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = area.width.min(width);
    let height = area.height.min(height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
