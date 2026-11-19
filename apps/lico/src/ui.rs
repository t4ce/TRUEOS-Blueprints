use crate::audio::{self, Command, DEFAULT_BPM, DEFAULT_PATCH, MONITOR_FRAMES, Status};
use crate::mcp::{self, Session, SharedSession};
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Clear, Dataset, GraphType, Paragraph, Sparkline, Wrap};
use ratatui::{DefaultTerminal, Frame};
use ratatui_textarea::TextArea;
use rustfft::{FftPlanner, num_complex::Complex32};
use std::error::Error;
use std::io;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};

pub fn run(pairing: bool) -> Result<(), Box<dyn Error>> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("the editor requires a terminal".into());
    }
    let session = Arc::new(Mutex::new(Session::new(DEFAULT_PATCH.to_owned())));
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_time();
    let token = if pairing {
        Some(mcp::pairing_token()?)
    } else {
        match std::env::var("LICO_MCP_TOKEN") {
            Ok(token) => Some(token),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(error.into()),
        }
    };
    let connect = if pairing {
        token.as_deref().map(mcp::connect_command)
    } else {
        None
    };
    if token.is_some() {
        builder.enable_io();
    }
    let runtime = builder.build()?;
    let endpoint = token
        .map(|token| runtime.block_on(mcp::start(token, session.clone())))
        .transpose()?;
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
                            endpoint.is_some(),
                            connect.as_deref(),
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

    if let Some(endpoint) = endpoint {
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
    mcp_enabled: bool,
    connect: Option<&str>,
    first_frame: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut first_frame = Some(first_frame);
    let mut patch = patch_editor(DEFAULT_PATCH);
    let mut review: Option<(String, u16)> = None;
    let mut pairing_panel = connect.is_some();

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
    let mut reference = None;

    loop {
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

        let pending = mcp_enabled && session.lock().expect("Lico session").proposal.is_some();
        terminal.draw(|frame| {
            let [header, edit, plots, status, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Percentage(30),
                Constraint::Length(if error.is_some() || reference.is_some() {
                    7
                } else {
                    3
                }),
                Constraint::Length(1),
            ])
            .areas(frame.area());
            frame.render_widget(
                Line::from(vec![
                    Span::styled(
                        " LICO ",
                        Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
                    ),
                    Span::raw(r"  Samples  \808bd  \808sd  \808ch  \blip"),
                    Span::raw(if pending {
                        "  • Alt+P review Codex edit"
                    } else if connect.is_some() {
                        "  • Alt+M connect Codex"
                    } else if mcp_enabled {
                        "  • MCP ready"
                    } else {
                        ""
                    }),
                ]),
                header,
            );
            frame.render_widget(&patch, edit);
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
                Line::from(" Alt+R Run   Alt+S Stop   Alt+D Docs   Alt+[ / ] Tempo   Esc Quit "),
                footer,
            );
            if let Some(command) = connect.filter(|_| pairing_panel) {
                let area = frame.area().inner(ratatui::layout::Margin {
                    horizontal: 2,
                    vertical: 1,
                });
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(format!(
                        "Run this in another Shell3 slot. Add resume --last to continue your conversation.\n\n{command}\n\nThe key is valid while this Lico session is open."
                    ))
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(" Connect Codex · Esc back ")),
                    area,
                );
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
                if pairing_panel {
                    if key.code == KeyCode::Esc || (alt && key.code == KeyCode::Char('m')) {
                        pairing_panel = false;
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
                    KeyCode::Char('m') if alt && connect.is_some() => {
                        pairing_panel = true;
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
                            session
                                .lock()
                                .expect("Lico session")
                                .update_draft(patch.lines().join("\n"));
                        }
                        None
                    }
                };
                if let Some(command) = command {
                    reference = None;
                    let _ = commands.send(command).await;
                }
            }
            Event::Paste(text) if review.is_none() && !pairing_panel => {
                reference = None;
                if patch.insert_str(text) {
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

fn patch_editor(code: &str) -> TextArea<'static> {
    let mut patch = TextArea::new(code.split('\n').map(str::to_owned).collect());
    patch.set_block(Block::bordered().title(" Patch ").border_style(Color::Cyan));
    patch.set_line_number_style(Style::default().fg(Color::DarkGray));
    patch.set_cursor_line_style(Style::default());
    patch
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
    area: ratatui::layout::Rect,
    waveform: &[(f64, f64)],
    spectrum: &[u64],
    sample_rate: u32,
) {
    let [scope, frequencies] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
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
