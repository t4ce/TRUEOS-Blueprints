use crate::audio::MonitorFrame;
use tokio::sync::watch;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod native {
    use super::{MonitorFrame, watch};
    use crate::audio::SAMPLE_RATE;
    use crate::visualizer_data::Analyzer;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
    use std::thread;
    use std::time::{Duration, Instant};
    use trueos::input::{
        KEYBOARD_KEY_ESCAPE, KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY,
    };
    use trueos::ui4_scene::{self, Error, Frame, SHADERTOY_LICO, ShadertoyParamsV1};
    use trueos::ui4_solara_text::FrameEscapeKeyAction;

    const SHADER: &[u8] = include_bytes!("../assets/visualizer.stpkg");
    const FRAME_HZ: u32 = 30;

    pub struct Visualizer {
        monitor: watch::Receiver<MonitorFrame>,
        worker: Option<Worker>,
    }

    struct Worker {
        stop: Arc<AtomicBool>,
        updates: Receiver<Update>,
    }

    enum Update {
        Opened,
        Finished(Result<(), (&'static str, Error)>),
    }

    impl Visualizer {
        pub fn new(monitor: watch::Receiver<MonitorFrame>) -> Self {
            Self {
                monitor,
                worker: None,
            }
        }

        pub fn toggle(&mut self) -> String {
            let _ = self.take_status();
            if self.close_if_open() {
                return "Closing visualizer…".into();
            }

            let monitor = self.monitor.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_worker = Arc::clone(&stop);
            let (send, updates) = mpsc::channel();
            match thread::Builder::new()
                .name("lico-visualizer".into())
                .spawn(move || {
                    let result = render_window(monitor, &stop_worker, &send);
                    let _ = send.send(Update::Finished(result));
                }) {
                Ok(_thread) => {
                    self.worker = Some(Worker { stop, updates });
                    "Opening visualizer…".into()
                }
                Err(error) => format!("Visualizer worker could not start ({error})"),
            }
        }

        pub fn close_if_open(&self) -> bool {
            let Some(worker) = &self.worker else {
                return false;
            };
            worker.stop.store(true, Ordering::Release);
            true
        }

        pub fn take_status(&mut self) -> Option<String> {
            let worker = self.worker.as_ref()?;
            let mut opened = None;
            loop {
                match worker.updates.try_recv() {
                    Ok(Update::Opened) => opened = Some("Visualizer opened on TRUEOS".into()),
                    Ok(Update::Finished(result)) => {
                        self.worker = None;
                        return Some(match result {
                            Ok(()) => "Visualizer closed".into(),
                            Err((stage, error)) => {
                                format!("Visualizer {stage} failed ({error:?})")
                            }
                        });
                    }
                    Err(TryRecvError::Empty) => return opened,
                    Err(TryRecvError::Disconnected) => {
                        self.worker = None;
                        return Some("Visualizer worker exited unexpectedly".into());
                    }
                }
            }
        }
    }

    impl Drop for Visualizer {
        fn drop(&mut self) {
            // The visual worker drops its frame after its current render.
            self.close_if_open();
        }
    }

    fn render_window(
        monitor: watch::Receiver<MonitorFrame>,
        stop: &AtomicBool,
        updates: &Sender<Update>,
    ) -> Result<(), (&'static str, Error)> {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let (output_width, output_height) =
            ui4_scene::output_dimensions().map_err(|error| ("display query", error))?;
        let (width, height) =
            ui4_scene::fit_frame_extent(640.min(output_width), 360.min(output_height))
                .map_err(|error| ("size query", error))?;
        let x = output_width.saturating_sub(width + 24) as i32;
        let y = output_height.saturating_sub(height).min(64) as i32;
        let mut frame = Frame::open_visual(x, y, width, height, FRAME_HZ)
            .map_err(|error| ("window open", error))?;
        frame
            .set_escape_key_action(FrameEscapeKeyAction::DeliverToApplication)
            .map_err(|error| ("Escape setup", error))?;
        frame
            .set_title("Lico · Visualizer")
            .map_err(|error| ("title", error))?;
        frame
            .register_shadertoy(SHADERTOY_LICO, SHADER)
            .map_err(|error| ("shader registration", error))?;

        let mut analyzer = Analyzer::new();
        let started = Instant::now();
        let mut previous = started;
        let mut frame_number = 0u32;
        let mut pending_size = None;
        while !stop.load(Ordering::Acquire) {
            loop {
                match frame.take_keyboard_event() {
                    Ok(Some(event))
                        if event.kind == KEYBOARD_OUTPUT_KIND_KEY
                            && event.key_code == KEYBOARD_KEY_ESCAPE
                            && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0 =>
                    {
                        return Ok(());
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(Error::NotFound | Error::InvalidState) => return Ok(()),
                    Err(error) => return Err(("keyboard event", error)),
                }
            }
            match frame.take_resize_event() {
                Ok(Some(size)) => pending_size = Some((size.width, size.height)),
                Ok(None) => {}
                Err(Error::NotFound | Error::InvalidState) => return Ok(()),
                Err(error) => return Err(("resize event", error)),
            }
            if let Some((width, height)) = pending_size {
                match frame.resize(width, height) {
                    Ok(()) => pending_size = None,
                    Err(Error::Busy) => {
                        thread::sleep(Duration::from_millis(4));
                        continue;
                    }
                    Err(Error::NotFound | Error::InvalidState) => return Ok(()),
                    Err(error) => return Err(("resize", error)),
                }
            }

            // Release the watch guard before FFT or kernel calls so the
            // producer can publish the next audio batch independently.
            let snapshot = monitor.borrow().clone();
            let analysis = analyzer.sample(&snapshot);
            let now = Instant::now();
            let params = ShadertoyParamsV1 {
                shader_id: SHADERTOY_LICO,
                flags: 0,
                frame: frame_number,
                time_seconds: now.duration_since(started).as_secs_f32(),
                delta_seconds: now.duration_since(previous).as_secs_f32(),
                frame_rate: FRAME_HZ as f32,
                sample_rate: SAMPLE_RATE as f32,
                mouse_x: 0.0,
                mouse_y: 0.0,
                click_x: 0.0,
                click_y: 0.0,
                date_year: 0.0,
                date_month: 0.0,
                date_day: 0.0,
                date_seconds: 0.0,
            };
            match frame.render_shadertoy_audio_v2(params, analysis) {
                Ok(()) => {
                    if frame_number == 0 {
                        let _ = updates.send(Update::Opened);
                    }
                    previous = now;
                    frame_number = frame_number.wrapping_add(1);
                }
                Err(Error::Busy) => thread::sleep(Duration::from_millis(4)),
                Err(Error::NotFound | Error::InvalidState) => return Ok(()),
                Err(error) => return Err(("shader render", error)),
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub use native::Visualizer;

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub struct Visualizer;

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl Visualizer {
    pub fn new(_: watch::Receiver<MonitorFrame>) -> Self {
        Self
    }

    pub fn toggle(&mut self) -> String {
        "Visualizer opens on a TRUEOS UI4 display".into()
    }

    pub fn close_if_open(&self) -> bool {
        false
    }

    pub fn take_status(&mut self) -> Option<String> {
        None
    }
}
