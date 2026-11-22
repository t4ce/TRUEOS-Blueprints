//! One full opaque 1:1 quad meets UI4's retained BCS0 copy eligibility.
use trueos::ui4_scene::{Damage, Error, Frame, SpriteCorner, SpriteQuad, rgba};
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
                return Err(Error::Invalid);
            }
            match frame.upload_sprite_rgba8(MAP_SPRITE, width, height, pixels.as_raw()) {
                Ok(()) => self.stage = Stage::Uploaded(revision),
                Err(Error::Busy) => return Ok(Status::Busy),
                Err(error) => return Err(stage_error("upload", error)),
            }
        }

        match frame.begin_sprite_frame(rgba(255, 255, 255, 255)) {
            Ok(()) => {}
            Err(Error::Busy) => return Ok(Status::Busy),
            Err(error) => return Err(stage_error("begin", error)),
        }
        match frame.draw_sprite_quads(&[viewport_quad(width, height)]) {
            Ok(()) => self.publish(frame, revision, Damage::full(width, height)),
            Err(Error::Busy) => {
                // Admission Busy cancels the write lease. The retained sprite
                // can be drawn on a new lease or replaced by newer pixels.
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!("osm: sprite admission busy action=restart-frame"),
                );
                Ok(Status::Busy)
            }
            Err(error) => Err(stage_error("draw", error)),
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
            Err(error) => Err(stage_error("publish", error)),
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

fn stage_error(stage: &str, error: Error) -> Error {
    trueos::logl::log(
        trueos::logl::level::ERROR,
        format_args!("osm: presentation stage={stage} error={error:?}"),
    );
    error
}
