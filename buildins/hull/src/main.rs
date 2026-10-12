use trueos::ui4_scene::{output_dimensions, rgba};
use trueos::ui4_winit::{Damage, Error, SceneTarget};
use trueos::{logl, netfs};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::trueos::WindowExtTrueOS;
use winit::window::{Window, WindowAttributes, WindowId};

mod css;
mod snapshot;

#[derive(Default)]
struct Hull {
    window: Option<Box<dyn Window>>,
    target: Option<SceneTarget>,
    pending_publish: bool,
    download: Option<u32>,
    html: Option<Vec<u8>>,
    document: Option<rp5::Document>,
    css: css::Css,
    save_attempted: bool,
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
        let window = event_loop.create_window(attributes)?;
        let size = window.surface_size();
        self.target = Some(
            SceneTarget::for_window(window.trueos_window_id(), size.width, size.height)
                .map_err(|error| format!("{error:?}"))?,
        );
        window.request_redraw();
        self.window = Some(window);
        match netfs::fetch_bytes(b"https://example.com/") {
            Ok(operation) => self.download = Some(operation),
            Err(code) => logl::log(
                logl::level::ERROR,
                format_args!("hull: download start failed: {code}"),
            ),
        }
        Ok(())
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
                        format_args!(
                            "hull: downloaded https://example.com/ bytes={}",
                            bytes.len()
                        ),
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
                    self.css.parse_document(&document, "https://example.com/");
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
        self.poll_download();
        self.css.poll_downloads();
        if !self.save_attempted && self.document.is_some() && !self.css.is_pending() {
            self.save_attempted = true;
            if let Err(error) = snapshot::uneccecaryStoreToFilesystemForDebug(
                self.document.as_ref().unwrap(),
                &self.css,
            ) {
                logl::log(
                    logl::level::ERROR,
                    format_args!("hull: snapshot save failed: {error}"),
                );
            }
        }
        if self.download.is_some() || self.css.is_pending() {
            trueos::vsys::sleep_ms(50);
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
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
            WindowEvent::SurfaceResized(size) => {
                window.request_redraw();
                target.set_extent(size.width, size.height)
            }
            WindowEvent::RedrawRequested => (|| {
                if !self.pending_publish {
                    target.begin(rgba(35, 39, 39, 255))?;
                    self.pending_publish = true;
                }
                window.pre_present_notify();
                let size = window.surface_size();
                target.publish(Damage::full(size.width, size.height))?;
                self.pending_publish = false;
                Ok(())
            })(),
            _ => Ok(()),
        };
        match result {
            Ok(()) => {}
            Err(Error::Busy) => window.request_redraw(),
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
