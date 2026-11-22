//! Publish the opaque 1:1 viewport with an explicit BCS0 copy.
use trueos::{
    ui4_scene::{Damage, Error, Frame, SpriteCorner, SpriteQuad, rgba},
    ui4_winit::{SpriteBackend, SpriteCommand},
};
const MAP_SPRITE: u32 = 1;

#[derive(Debug, Eq, PartialEq)]
pub enum Status {
    Idle,
    Busy,
    Published(u64),
}

#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Uploaded(u64),
    Publishing(u64, Damage),
    Published(u64),
}

#[derive(Default)]
pub struct Presenter {
    stage: Stage,
}

impl Presenter {
    pub fn is_publishing(&self) -> bool {
        matches!(self.stage, Stage::Publishing(..))
    }

    /// Attempt the latest opaque viewport. Advance the revision whenever its
    /// pixels or extent change, so an unpublished sprite can be replaced.
    pub fn step<'a>(
        &mut self,
        frame: &mut Frame,
        revision: u64,
        pixels: impl FnOnce() -> &'a image::RgbaImage,
    ) -> Result<Status, Error> {
        match self.stage {
            // A successful draw owns a write lease until publication succeeds.
            Stage::Publishing(submitted, damage) => return self.publish(frame, submitted, damage),
            Stage::Published(published) if published == revision => return Ok(Status::Idle),
            _ => {}
        }

        let (width, height) = (frame.width(), frame.height());
        if !matches!(self.stage, Stage::Uploaded(uploaded) if uploaded == revision) {
            let pixels = pixels();
            if pixels.dimensions() != (width, height) {
                return Err(stage_error(frame, revision, "pixels", Error::Invalid));
            }
            match frame.upload_sprite_rgba8(MAP_SPRITE, width, height, pixels.as_raw()) {
                Ok(()) => self.stage = Stage::Uploaded(revision),
                Err(Error::Busy) => return Ok(Status::Busy),
                Err(error) => return Err(stage_error(frame, revision, "upload", error)),
            }
        }

        match frame.begin_gpu_frame() {
            Ok(()) => {}
            Err(Error::Busy) => return Ok(Status::Busy),
            Err(error) => return Err(stage_error(frame, revision, "begin", error)),
        }
        let copy = SpriteCommand {
            quad: viewport_quad(width, height),
            backend: SpriteBackend::Bcs0,
        };
        match frame.draw_sprite_commands(&[copy]) {
            Ok(()) => self.publish(frame, revision, Damage::full(width, height)),
            Err(Error::Busy) => {
                // Admission Busy cancels the write lease. The retained sprite
                // can be drawn on a new lease or replaced by newer pixels.
                Ok(Status::Busy)
            }
            Err(error) => Err(stage_error(frame, revision, "draw", error)),
        }
    }

    fn publish(
        &mut self,
        frame: &mut Frame,
        revision: u64,
        damage: Damage,
    ) -> Result<Status, Error> {
        match frame.publish(damage) {
            Ok(()) => {
                self.stage = Stage::Published(revision);
                Ok(Status::Published(revision))
            }
            Err(Error::Busy) => {
                self.stage = Stage::Publishing(revision, damage);
                Ok(Status::Busy)
            }
            Err(error) => Err(stage_error(frame, revision, "publish", error)),
        }
    }
}

fn viewport_quad(width: u32, height: u32) -> SpriteQuad {
    SpriteQuad {
        sprite_id: MAP_SPRITE,
        c0: SpriteCorner {
            x: 0.0,
            y: 0.0,
            u: 0.0,
            v: 0.0,
        },
        c1: SpriteCorner {
            x: width as f32,
            y: 0.0,
            u: 1.0,
            v: 0.0,
        },
        c2: SpriteCorner {
            x: width as f32,
            y: height as f32,
            u: 1.0,
            v: 1.0,
        },
        c3: SpriteCorner {
            x: 0.0,
            y: height as f32,
            u: 0.0,
            v: 1.0,
        },
        color_rgba: rgba(255, 255, 255, 255),
        source_over: false,
    }
}

fn stage_error(frame: &Frame, revision: u64, stage: &str, error: Error) -> Error {
    trueos::logl::log_record(
        trueos::logl::level::ERROR,
        "osm",
        format_args!(
            "presentation stage={stage} window={} extent={}x{} revision={revision} error={error:?}",
            frame.window_id(),
            frame.width(),
            frame.height(),
        ),
    );
    error
}
