//! Independently paced UI4 background producer.

use bfly::backdrop::Backdrop;
use bfly::celestial::{Body, Sprite, Wake, appearance_for};
use bfly::celestial_pixels::frame_rgba8;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use trueos::clock;
use trueos::logl::{self, level};
use trueos::ui4_scene::{BackgroundLayer, Damage, Error as Ui4Error, Frame};
use trueos::ui4_winit::{SceneTarget, SpriteBackend, SpriteCommand, SpriteCorner, SpriteQuad};
use trueos::{vsys, worker};

use super::{Error, RETRY_MS};

pub(super) const BACKGROUND_HZ: u32 = 30;
const SECOND_NS: u64 = 1_000_000_000;
const SAMPLE_NS: u64 = SECOND_NS / BACKGROUND_HZ as u64;
const STAGE_FADE_NS: u64 = 400_000_000;
const BACKGROUND_SPRITE: u32 = 1;
const WAKE_KIND_MASK: u64 = 0b11;

struct Control {
    extent: AtomicU64,
    resizing: AtomicBool,
    wake: AtomicU64,
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

    fn publish_wake(&self, wake: Wake, started_ns: u64) {
        if self.wake.load(Ordering::Relaxed) & WAKE_KIND_MASK != wake as u64 {
            self.wake
                .store(pack_wake(wake, started_ns), Ordering::Release);
        }
    }
}

fn pack_wake(wake: Wake, started_ns: u64) -> u64 {
    // Keep the preset and its monotonic handoff time in one atomic sample.
    (started_ns & !WAKE_KIND_MASK) | wake as u64
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
        let layer = frame
            .background()
            .map_err(|code| Error::Ui4("open background layer", code))?;
        let target = SceneTarget::for_window(layer.render_target(), width, height)
            .map_err(|code| Error::Ui4("open background copy target", code))?;
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
        let initial_wake = appearance_for(initial.map(f32::from)).wake;
        let control = Arc::new(Control {
            extent: AtomicU64::new(pack_extent(width, height)),
            resizing: AtomicBool::new(false),
            wake: AtomicU64::new(pack_wake(initial_wake, clock::monotonic_nanos())),
        });
        let worker_control = Arc::clone(&control);
        let job = worker::spawn(move || {
            if let Err(error) = render(layer, target, display, initial, &worker_control)
                && !matches!(error, Error::Ui4(_, Ui4Error::NotFound))
                && !worker_control.stopping()
            {
                logl::log(level::ERROR, format_args!("bfly {error}"));
            }
        })
        .map_err(Error::BackgroundWorker)?;
        drop(job);
        Ok(Self(control))
    }

    pub(super) fn wake(&self) -> (Wake, u64) {
        let sample = self.0.wake.load(Ordering::Acquire);
        (
            Wake::from_code((sample & WAKE_KIND_MASK) as u8),
            sample & !WAKE_KIND_MASK,
        )
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Stage {
    body: Body,
    white_gradient: bool,
}

impl Stage {
    fn sprite(self) -> Sprite {
        let opacity = match self.body {
            Body::Moon if self.white_gradient => 0.50,
            Body::Moon => 0.70,
            Body::Earth => 0.92,
            Body::Sun => 1.0,
        };
        Sprite {
            body: self.body,
            opacity,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct BackgroundFrame {
    extent: u64,
    stage: Stage,
}

fn copy_sprite(width: u32, height: u32) -> SpriteCommand {
    let corner = |x: u32, y: u32, u: f32, v: f32| SpriteCorner {
        x: x as f32,
        y: y as f32,
        u,
        v,
    };
    SpriteCommand {
        quad: SpriteQuad {
            sprite_id: BACKGROUND_SPRITE,
            c0: corner(0, 0, 0.0, 0.0),
            c1: corner(width, 0, 1.0, 0.0),
            c2: corner(width, height, 1.0, 1.0),
            c3: corner(0, height, 0.0, 1.0),
            color_rgba: u32::MAX,
            source_over: false,
        },
        backend: SpriteBackend::Bcs0Overwrite,
    }
}

fn wait_background<T>(
    control: &Control,
    name: &'static str,
    mut operation: impl FnMut() -> Result<T, Ui4Error>,
) -> Result<T, Error> {
    loop {
        if control.stopping() {
            return Err(Error::Ui4(name, Ui4Error::NotFound));
        }
        match operation() {
            Ok(value) => return Ok(value),
            Err(Ui4Error::Busy) => vsys::sleep_ms(RETRY_MS),
            Err(error) => return Err(Error::Ui4(name, error)),
        }
    }
}

fn copy_frame(
    layer: &mut BackgroundLayer,
    target: &mut SceneTarget,
    frame: BackgroundFrame,
    control: &Control,
) -> Result<(), Error> {
    let width = (frame.extent >> 32) as u32;
    let height = frame.extent as u32;
    let stage = frame.stage;
    let pixels = frame_rgba8([width, height], stage.white_gradient, Some(stage.sprite()));
    wait_background(control, "retain background pixels", || {
        target.upload_sprite_rgba8(BACKGROUND_SPRITE, width, height, &pixels)
    })?;

    let command = copy_sprite(width, height);
    loop {
        wait_background(control, "begin background copy", || {
            layer.begin_visual_gpu_frame()
        })?;
        match target.draw_sprite_commands(core::slice::from_ref(&command)) {
            Ok(()) => break,
            // A deferred BCS0 copy discards its unwritten lease.
            Err(Ui4Error::Busy) => vsys::sleep_ms(RETRY_MS),
            Err(error) => return Err(Error::Ui4("BCS0 background copy", error)),
        }
    }
    wait_background(control, "publish background copy", || {
        layer.publish(Damage::full(width, height))
    })?;
    let body = stage.body;
    let tint = if stage.white_gradient {
        "white"
    } else {
        "black"
    };
    logl::log(
        level::INFO,
        format_args!("bfly background BCS0 stage={body:?} extent={width}x{height} gradient={tint}"),
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct Fade {
    started_ns: u64,
    from: u8,
}

impl Fade {
    fn opacity_at(self, now: u64, to: f32) -> u8 {
        let t =
            now.saturating_sub(self.started_ns).min(STAGE_FADE_NS) as f32 / STAGE_FADE_NS as f32;
        let weight = t * t * (3.0 - 2.0 * t);
        (f32::from(self.from) * (1.0 - weight) + to * weight + 0.5) as u8
    }
}

fn set_opacity(
    layer: &mut BackgroundLayer,
    control: &Control,
    current: &mut u8,
    target: u8,
) -> Result<(), Error> {
    if *current != target {
        wait_background(control, "set background plane opacity", || {
            layer.set_opacity(target)
        })?;
        *current = target;
    }
    Ok(())
}

fn render(
    mut layer: BackgroundLayer,
    mut target: SceneTarget,
    mut display: SceneTarget,
    mut rgb: [u8; 3],
    control: &Control,
) -> Result<(), Error> {
    let started = clock::monotonic_nanos();
    let mut backdrop = Some(Backdrop::new(rgb, started));
    let mut precise_rgb = rgb.map(f32::from);
    let mut published = None;
    let mut next_sample = started;
    let mut fade_in = Fade {
        started_ns: started,
        from: 0,
    };
    let mut fade_out = None;
    let mut opacity = u8::MAX;
    set_opacity(&mut layer, control, &mut opacity, 0)?;

    while !control.stopping() {
        let now = clock::monotonic_nanos();
        if let Some(drift) = &mut backdrop {
            let next = drift.sample_at(now);
            if next.rgb == rgb {
                precise_rgb = next.precise_rgb;
            } else {
                match display.set_display_bottom_color(next.rgb) {
                    Ok(()) => {
                        rgb = next.rgb;
                        precise_rgb = next.precise_rgb;
                    }
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

        let appearance = appearance_for(precise_rgb);
        let extent = control.extent.load(Ordering::Acquire);
        if extent == 0 {
            break;
        }
        let desired = appearance.sprite.map(|sprite| {
            let stage = Stage {
                body: sprite.body,
                white_gradient: appearance.white_gradient,
            };
            (BackgroundFrame { extent, stage }, sprite.opacity)
        });
        if let Some((frame, strength)) = desired.filter(|&(frame, _)| Some(frame) == published) {
            if fade_out.take().is_some() {
                fade_in = Fade {
                    started_ns: now,
                    from: opacity,
                };
            }
            let peak = frame.stage.sprite().opacity;
            let target_alpha = 255.0 * (strength / peak).clamp(0.0, 1.0);
            let alpha = fade_in.opacity_at(now, target_alpha);
            set_opacity(&mut layer, control, &mut opacity, alpha)?;
        } else {
            let fade = fade_out.get_or_insert(Fade {
                started_ns: now,
                from: opacity,
            });
            let resized = published.is_some_and(|last: BackgroundFrame| last.extent != extent);
            let alpha = if resized {
                0
            } else {
                fade.opacity_at(now, 0.0)
            };
            set_opacity(&mut layer, control, &mut opacity, alpha)?;
            if alpha == 0 {
                if let Some((mut frame, _)) = desired
                    && let Some(_guard) = control.try_guard()
                {
                    frame.extent = control.extent.load(Ordering::Acquire);
                    if frame.extent == 0 {
                        break;
                    }
                    copy_frame(&mut layer, &mut target, frame, control)?;
                    published = Some(frame);
                    let handoff = clock::monotonic_nanos();
                    control.publish_wake(appearance.wake, handoff);
                    fade_in = Fade {
                        started_ns: handoff,
                        from: 0,
                    };
                    fade_out = None;
                }
            }
        }

        let now = clock::monotonic_nanos();
        next_sample = next_sample.saturating_add(SAMPLE_NS);
        if next_sample <= now {
            next_sample = now.saturating_add(SAMPLE_NS);
        }
        vsys::sleep_ms((next_sample - now).div_ceil(1_000_000));
    }
    Ok(())
}
