use std::collections::VecDeque;
use trueos::ui4_scene::{output_dimensions, rgba};
use trueos::ui4_winit::{Damage, Error, HullTextRun, SceneTarget};
use trueos::{logl, netfs, vshell};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::trueos::{ActiveEventLoopExtTrueOS, WindowExtTrueOS};
use winit::window::{Window, WindowAttributes, WindowId};

mod css;
mod layout;
mod scripts;
mod shellio;
mod snapshot;
mod viewport;
#[path = "../xxx/mod.rs"]
mod xxx;

const PAGE_URL: &str = "https://example.com/";

#[derive(Default)]
struct Hull {
    window: Option<Box<dyn Window>>,
    target: Option<SceneTarget>,
    pending_publish: bool,
    quads_dirty: bool,
    redraw_reason: &'static str,
    frame_counter: shellio::FrameCounter,
    quads: Option<xxx::Quads>,
    text_runs: Vec<layout::TextRun>,
    rectangles: Vec<xxx::Rect>,
    layout_extent: (u32, u32),
    viewport: viewport::Viewport,
    shift_scroll: bool,
    download: Option<u32>,
    html: Option<Vec<u8>>,
    document: Option<rp5::Document>,
    css: css::Css,
    scripts: scripts::Scripts,
    save_attempted: bool,
    page_url: String,
    shell_input: shellio::ShellInput,
    queued_urls: VecDeque<String>,
}

impl Hull {
    fn create_window(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (width, height) = output_dimensions().map_err(|error| format!("{error:?}"))?;
        let attributes = WindowAttributes::default()
            .with_surface_size(PhysicalSize::new(1280, 720))
            .with_position(PhysicalPosition::new(
                ((i64::from(width) - 1280) / 2) as i32,
                ((i64::from(height) - 720) / 2) as i32,
            ));
        let window = event_loop.create_streaming_window(attributes)?;
        let size = window.surface_size();
        self.target = Some(
            SceneTarget::for_window(window.trueos_window_id(), size.width, size.height)
                .map_err(|error| format!("{error:?}"))?,
        );
        self.viewport.resize(size.width, size.height);
        self.quads_dirty = true;
        self.redraw_reason = "initial";
        window.request_redraw();
        self.window = Some(window);
        vshell::line("hull: enter an HTTPS URL to download, parse and display it");
        vshell::line(
            "hull: scroll with wheel, arrows or Page Up/Down; Home/End; Shift+wheel pans sideways",
        );
        self.start_page_with_policy(PAGE_URL.into(), false);
        Ok(())
    }

    fn start_page(&mut self, url: String) {
        self.start_page_with_policy(url, true);
    }

    fn start_page_with_policy(&mut self, url: String, happy_eyeballs: bool) {
        let download = if happy_eyeballs {
            netfs::fetch_bytes_happy_eyeballs(url.as_bytes())
        } else {
            netfs::fetch_bytes(url.as_bytes())
        };
        match download {
            Ok(operation) => {
                self.html = None;
                self.document = None;
                self.css = css::Css::default();
                self.scripts = scripts::Scripts::default();
                self.save_attempted = false;
                self.page_url = url;
                self.download = Some(operation);
                vshell::linef(format_args!("hull: downloading {}", self.page_url));
            }
            Err(code) => {
                vshell::linef(format_args!("hull: download start failed: {code}"));
            }
        }
    }

    fn poll_shell_input(&mut self) {
        // The VM minishell owns echo, line editing and its prompt.
        for _ in 0..512 {
            let Some(byte) = vshell::attached_read_byte() else {
                break;
            };
            let Some(line) = self.shell_input.push(byte) else {
                continue;
            };
            match line.and_then(|line| shellio::page_url(&line)) {
                Ok(url) if self.queued_urls.len() < 16 => self.queued_urls.push_back(url),
                Ok(_) => {
                    vshell::line(
                        "hull: URL queue is full; try again after the current pages finish",
                    );
                }
                Err(error) => {
                    vshell::linef(format_args!("hull: {error}"));
                }
            }
        }
    }

    fn page_pending(&self) -> bool {
        self.download.is_some()
            || self.css.is_pending()
            || (self.document.is_some() && !self.save_attempted)
    }

    fn poll_download(&mut self) {
        let Some(operation) = self.download else {
            return;
        };
        match netfs::fetch_bytes_result_len(operation) {
            Err(-8) => return, // Kernel operation is still pending.
            Err(code) => logl::log(
                logl::level::ERROR,
                format_args!("hull: download failed: {code}"),
            ),
            Ok(_) => match netfs::fetch_bytes_read(operation) {
                Ok(bytes) => {
                    logl::log(
                        logl::level::INFO,
                        format_args!("hull: downloaded {} bytes={}", self.page_url, bytes.len()),
                    );
                    let document = rp5::parse(&String::from_utf8_lossy(&bytes));
                    logl::log(
                        logl::level::INFO,
                        format_args!(
                            "hull: rp5 parsed nodes={} errors={}",
                            document.nodes.len(),
                            document.errors.len(),
                        ),
                    );
                    self.css.parse_document(&document, &self.page_url);
                    self.scripts.extract(&document, &self.page_url);
                    self.document = Some(document);
                    self.html = Some(bytes);
                }
                Err(code) => logl::log(
                    logl::level::ERROR,
                    format_args!("hull: download read failed: {code}"),
                ),
            },
        }
        let _ = netfs::fetch_bytes_discard(operation);
        self.download = None;
    }
}

impl Drop for Hull {
    fn drop(&mut self) {
        if let Some(operation) = self.download.take() {
            let _ = netfs::fetch_bytes_discard(operation);
        }
    }
}

impl ApplicationHandler for Hull {
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.poll_shell_input();
        self.poll_download();
        self.css.poll_downloads();
        self.scripts.poll_downloads();
        // Scripts are inspection-only; their downloads cannot block CSS layout.
        if !self.save_attempted && self.document.is_some() && !self.css.is_pending() {
            self.save_attempted = true;
            let result = snapshot::save_svg(
                self.document.as_ref().unwrap(),
                &self.css,
                &self.page_url,
                self.window
                    .as_ref()
                    .map(|window| window.surface_size())
                    .map(|size| (size.width, size.height))
                    .unwrap_or((1280, 720)),
            )
            .and_then(|layout| {
                if self.quads.is_none() {
                    self.quads = Some(xxx::Quads::new()?);
                }
                self.layout_extent = layout.line_extent;
                self.text_runs = layout.text_runs;
                self.rectangles = layout.line_rects;
                // CSS overflow can extend beyond the SVG's declared viewBox.
                // Keep that overflow reachable without allocating a page-sized frame.
                let mut right = self.layout_extent.0 as f32;
                let mut bottom = self.layout_extent.1 as f32;
                for &[x, y, width, height] in &self.rectangles {
                    if [x, y, width, height].iter().all(|value| value.is_finite()) {
                        right = right.max(x + width);
                        bottom = bottom.max(y + height);
                    }
                }
                for run in &self.text_runs {
                    right = right.max(run.x + run.advance_px);
                    bottom = bottom.max(run.baseline_y + run.size_px * 0.25 + 1.0);
                }
                self.viewport
                    .set_document(right.ceil() as u32, bottom.ceil() as u32);
                self.quads_dirty = true;
                self.redraw_reason = "layout";
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                Ok(())
            });
            if let Err(error) = result {
                logl::log(
                    logl::level::ERROR,
                    format_args!("hull: layout display failed: {error}"),
                );
            }
        }
        if !self.page_pending() {
            if let Some(url) = self.queued_urls.pop_front() {
                self.start_page(url);
            }
        }
        // ShellIO input does not wake Winit's window-event wait. Keep polling
        // cooperatively even after the current page has finished.
        trueos::vsys::sleep_ms(20);
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(error) = self.create_window(event_loop) {
                logl::log(logl::level::ERROR, format_args!("hull: {error}"));
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref().filter(|window| window.id() == id) else {
            return;
        };
        let Some(target) = self.target.as_mut() else {
            return;
        };
        let result = match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                event_loop.exit();
                Ok(())
            }
            WindowEvent::SurfaceResized(size) => (|| {
                if size.width == 0 || size.height == 0 {
                    return Ok(());
                }
                self.pending_publish = false;
                self.quads_dirty = true;
                self.redraw_reason = "resize";
                self.viewport.resize(size.width, size.height);
                self.frame_counter.resizing();
                window.request_redraw();
                target.set_extent(size.width, size.height)
            })(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift_scroll = modifiers.state().shift_key();
                Ok(())
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (mut dx, mut dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (-f64::from(x) * 48.0, -f64::from(y) * 48.0)
                    }
                    MouseScrollDelta::PixelDelta(position) => (-position.x, -position.y),
                    _ => (0.0, 0.0),
                };
                if self.shift_scroll && dx == 0.0 {
                    dx = dy;
                    dy = 0.0;
                }
                if self.viewport.scroll_by(dx, dy) {
                    self.quads_dirty = true;
                    self.redraw_reason = "scroll";
                    window.request_redraw();
                }
                Ok(())
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let changed = match event.logical_key {
                    Key::Named(NamedKey::ArrowUp) => self.viewport.scroll_by(0.0, -48.0),
                    Key::Named(NamedKey::ArrowDown) => self.viewport.scroll_by(0.0, 48.0),
                    Key::Named(NamedKey::ArrowLeft) => self.viewport.scroll_by(-48.0, 0.0),
                    Key::Named(NamedKey::ArrowRight) => self.viewport.scroll_by(48.0, 0.0),
                    Key::Named(NamedKey::PageUp) => self.viewport.page(-1),
                    Key::Named(NamedKey::PageDown) => self.viewport.page(1),
                    Key::Named(NamedKey::Home) => self.viewport.home(),
                    Key::Named(NamedKey::End) => self.viewport.end(),
                    _ => false,
                };
                if changed {
                    self.quads_dirty = true;
                    self.redraw_reason = "scroll";
                    window.request_redraw();
                }
                Ok(())
            }
            WindowEvent::RedrawRequested => (|| {
                // Moving the retained UI4 frame is compositor work. A generic
                // redraw notification does not invalidate our content.
                if !self.quads_dirty && !self.pending_publish {
                    return Ok(());
                }
                if !self.pending_publish && self.quads_dirty {
                    self.frame_counter.attempt();
                    let size = window.surface_size();
                    let visible_rectangles: Vec<_> = self
                        .rectangles
                        .iter()
                        .filter_map(|&rect| self.viewport.clip_rect(rect))
                        .collect();
                    if let Some(quads) = &mut self.quads {
                        quads
                            .set_layout(size.width, size.height, &visible_rectangles)
                            .map_err(|code| {
                                if code == trueos::vgpu::ERR_BUSY {
                                    Error::Busy
                                } else {
                                    Error::Unknown(code)
                                }
                            })?;
                        quads.draw(target)?;
                    } else {
                        target.begin(rgba(35, 39, 39, 255))?;
                    }
                    if !self.text_runs.is_empty() {
                        // Document pixels remain document pixels. Cull and trim
                        // offscreen runs before transport or font preparation.
                        let runs: Vec<_> = self
                            .text_runs
                            .iter()
                            .filter_map(|run| {
                                if run.color[3] == 0 {
                                    return None;
                                }
                                let visible = self.viewport.text_slice(
                                    &run.text,
                                    run.x,
                                    run.baseline_y,
                                    run.size_px,
                                    run.advance_px,
                                )?;
                                Some(HullTextRun {
                                    text: visible.text,
                                    x: visible.x,
                                    baseline_y: visible.baseline_y,
                                    size_px: visible.size_px,
                                    advance_px: visible.advance_px,
                                    rgba: u32::from_le_bytes(run.color),
                                })
                            })
                            .collect();
                        target.draw_hull_text_runs(&runs)?;
                        let (x, y) = self.viewport.offset();
                        let (doc_width, doc_height) = self.viewport.document_extent();
                        vshell::linef(format_args!(
                            "hull: viewport={}x{} document={}x{} scroll={},{} rects={}/{} text_runs={}/{} glyphs={} scale=1",
                            size.width,
                            size.height,
                            doc_width,
                            doc_height,
                            x,
                            y,
                            visible_rectangles.len(),
                            self.rectangles.len(),
                            runs.len(),
                            self.text_runs.len(),
                            runs.iter()
                                .map(|run| run.text.chars().count())
                                .sum::<usize>(),
                        ));
                    }
                    self.frame_counter.produced(
                        self.redraw_reason,
                        size.width,
                        size.height,
                        self.quads.is_some(),
                    );
                    self.quads_dirty = false;
                    self.pending_publish = true;
                }
                window.pre_present_notify();
                let size = window.surface_size();
                self.frame_counter.publishing();
                target.publish(Damage::full(size.width, size.height))?;
                self.frame_counter.published();
                self.pending_publish = false;
                if self.quads_dirty {
                    window.request_redraw();
                }
                Ok(())
            })(),
            _ => Ok(()),
        };
        match result {
            Ok(()) => {}
            Err(Error::Busy) => {
                self.frame_counter.busy();
                window.request_redraw();
            }
            Err(error) => {
                logl::log(logl::level::ERROR, format_args!("hull: {error:?}"));
                event_loop.exit();
            }
        }
    }
}

fn main() {
    let result = EventLoop::new().and_then(|event_loop| event_loop.run_app(Hull::default()));
    if let Err(error) = result {
        logl::log(logl::level::ERROR, format_args!("hull: {error}"));
    }
}
