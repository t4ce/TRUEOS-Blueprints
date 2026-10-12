use trueos::logl;
use trueos::ui4_scene::{output_dimensions, rgba};
use trueos::ui4_winit::{Damage, Error, SceneTarget};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::trueos::WindowExtTrueOS;
use winit::window::{Window, WindowAttributes, WindowId};

#[derive(Default)]
struct Hull {
    window: Option<Box<dyn Window>>,
    target: Option<SceneTarget>,
    pending_publish: bool,
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
        Ok(())
    }
}

impl ApplicationHandler for Hull {
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
