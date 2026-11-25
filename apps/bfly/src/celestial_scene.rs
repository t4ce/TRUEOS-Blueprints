//! Independently paced UI4 background producer.

use bfly::backdrop::Backdrop;
use bfly::celestial::{Sprite, SpriteRect, sprite_for};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use trueos::clock;
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    BackgroundLayer, Damage, Error as Ui4Error, Frame, SHADERTOY_BFLY_CELESTIAL, ShadertoyParamsV1,
};
use trueos::ui4_winit::SceneTarget;
use trueos::{vsys, worker};

use super::{Error, RETRY_MS};

pub(super) const BACKGROUND_HZ: u32 = 8;
const SAMPLE_MS: u64 = 1_000 / BACKGROUND_HZ as u64;
const SHADER: &[u8] = include_bytes!("../shaders/trueos/bfly-celestial.stpkg");

struct Control {
    extent: AtomicU64,
    resizing: AtomicBool,
}

impl Control {
    fn try_guard(&self) -> Option<ResizeGuard<'_>> {
        self.resizing
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| ResizeGuard(&self.resizing))
    }

    fn stopping(&self) -> bool {
        self.extent.load(Ordering::Acquire) == 0 || worker::cancellation_requested()
    }
}

struct ResizeGuard<'a>(&'a AtomicBool);

impl Drop for ResizeGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) struct CelestialScene(Arc<Control>);

impl CelestialScene {
    pub(super) fn start(frame: &Frame) -> Result<Self, Error> {
        let (width, height) = (frame.width(), frame.height());
        let mut layer = frame
            .background()
            .map_err(|code| Error::Ui4("open background layer", code))?;
        layer
            .register_shadertoy(SHADERTOY_BFLY_CELESTIAL, SHADER)
            .map_err(|code| Error::Ui4("register celestial shader", code))?;
        let current = frame.display_bottom_color().ok();
        let initial = current.unwrap_or([255; 3]);
        let source = if current.is_some() {
            "display"
        } else {
            "fallback"
        };
        logl::log(
            level::INFO,
            format_args!("bfly display backdrop source={source} start={initial:?}"),
        );
        let display = SceneTarget::for_window(frame.window_id(), width, height)
            .map_err(|code| Error::Ui4("open backdrop control", code))?;
        let control = Arc::new(Control {
            extent: AtomicU64::new(pack_extent(width, height)),
            resizing: AtomicBool::new(false),
        });
        let worker_control = Arc::clone(&control);
        let job = worker::spawn(move || {
            if let Err(error) = render(layer, display, initial, &worker_control)
                && !matches!(error, Error::Ui4(_, Ui4Error::NotFound))
                && !worker_control.stopping()
            {
                logl::log(level::ERROR, format_args!("bfly {error}"));
                let _ = trueos::vshell::shutdown_current_blueprint("bfly background failed");
            }
        })
        .map_err(Error::BackgroundWorker)?;
        drop(job);
        Ok(Self(control))
    }

    pub(super) fn try_resize(
        &self,
        frame: &mut Frame,
        width: u32,
        height: u32,
    ) -> Result<bool, Error> {
        let Some(_guard) = self.0.try_guard() else {
            return Ok(false);
        };
        match frame.resize(width, height) {
            Ok(()) => {
                self.0
                    .extent
                    .store(pack_extent(width, height), Ordering::Release);
                Ok(true)
            }
            Err(Ui4Error::Busy) => Ok(false),
            Err(code) => Err(Error::Ui4("resize window", code)),
        }
    }
}

impl Drop for CelestialScene {
    fn drop(&mut self) {
        self.0.extent.store(0, Ordering::Release);
    }
}

fn pack_extent(width: u32, height: u32) -> u64 {
    (u64::from(width) << 32) | u64::from(height)
}

fn render(
    mut layer: BackgroundLayer,
    mut display: SceneTarget,
    mut rgb: [u8; 3],
    control: &Control,
) -> Result<(), Error> {
    let mut backdrop = Some(Backdrop::new(rgb, clock::monotonic_nanos()));
    let mut published: Option<(u64, Option<Sprite>)> = None;
    while !control.stopping() {
        if let Some(drift) = &mut backdrop {
            let next = drift.rgb_at(clock::monotonic_nanos());
            if next != rgb {
                match display.set_display_bottom_color(next) {
                    Ok(()) => rgb = next,
                    Err(Ui4Error::Busy) => {}
                    Err(code) => {
                        logl::log(
                            level::WARN,
                            format_args!("bfly display backdrop stopped ({code:?})"),
                        );
                        backdrop = None;
                    }
                }
            }
        }
        let desired = (control.extent.load(Ordering::Acquire), sprite_for(rgb));
        if desired.0 == 0 {
            break;
        }
        if published != Some(desired)
            && let Some(_guard) = control.try_guard()
        {
            // Synchronize only resize staging. Both producers set their own pace.
            let extent = control.extent.load(Ordering::Acquire);
            if extent == 0 {
                break;
            }
            let sprite = desired.1;
            let resized = published.is_none_or(|(previous, _)| previous != extent);
            let (width, height) = ((extent >> 32) as u32, extent as u32);
            let rect = SpriteRect::top_right(width, height);
            match layer.begin_visual_gpu_frame() {
                Ok(()) => {
                    layer
                        .render_shadertoy_unpublished(&sprite_params(rect, sprite))
                        .map_err(|code| Error::Ui4("draw background sprite", code))?;
                    let damage = if resized {
                        Damage::full(width, height)
                    } else {
                        Damage {
                            x: rect.x,
                            y: rect.y,
                            width: rect.size,
                            height: rect.size,
                        }
                    };
                    loop {
                        match layer.publish_compute(damage) {
                            Ok(()) => break,
                            Err(Ui4Error::Busy) if !control.stopping() => vsys::sleep_ms(RETRY_MS),
                            Err(code) => return Err(Error::Ui4("publish background", code)),
                        }
                    }
                    if resized
                        || published
                            .is_some_and(|(_, old)| old.map(|s| s.body) != sprite.map(|s| s.body))
                    {
                        logl::log(
                            level::INFO,
                            format_args!(
                                "bfly background extent={width}x{height} sprite={:?} size={} rect={},{}",
                                sprite.map(|s| s.body),
                                rect.size,
                                rect.x,
                                rect.y,
                            ),
                        );
                    }
                    published = Some((extent, sprite));
                }
                Err(Ui4Error::Busy) => {}
                Err(code) => return Err(Error::Ui4("begin background", code)),
            }
        }
        vsys::sleep_ms(SAMPLE_MS);
    }
    Ok(())
}

fn sprite_params(rect: SpriteRect, sprite: Option<Sprite>) -> ShadertoyParamsV1 {
    ShadertoyParamsV1 {
        shader_id: SHADERTOY_BFLY_CELESTIAL,
        flags: 0,
        frame: 0,
        time_seconds: sprite.map_or(0.0, |state| state.body as u8 as f32),
        delta_seconds: 0.0,
        frame_rate: BACKGROUND_HZ as f32,
        sample_rate: 0.0,
        mouse_x: rect.x as f32,
        mouse_y: rect.y as f32,
        click_x: rect.size as f32,
        click_y: sprite.map_or(0.0, |state| f32::from(state.opacity) / 255.0),
        date_year: 0.0,
        date_month: 0.0,
        date_day: 0.0,
        date_seconds: 0.0,
    }
}
