//! Exercise the actual presenter against UI4's lease cancellation contract.
#![allow(dead_code)]
extern crate self as trueos;

pub mod logl {
    pub mod level {
        pub const IMPORTANT: u8 = 1;
        pub const ERROR: u8 = 2;
    }
    pub fn log(_: u8, _: std::fmt::Arguments<'_>) {}
}
pub mod ui4_scene {
    #[derive(Debug, PartialEq)]
    pub enum Error {
        Busy,
        Invalid,
        InvalidState,
    }
    pub struct SpriteCorner {
        pub x: f32,
        pub y: f32,
        pub u: f32,
        pub v: f32,
    }
    pub struct SpriteQuad {
        pub sprite_id: u32,
        pub c0: SpriteCorner,
        pub c1: SpriteCorner,
        pub c2: SpriteCorner,
        pub c3: SpriteCorner,
        pub color_rgba: u32,
        pub source_over: bool,
    }
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
        u32::from_le_bytes([r, g, b, a])
    }
    #[derive(Clone, Copy)]
    pub struct Damage;
    impl Damage {
        pub fn full(_: u32, _: u32) -> Self {
            Self
        }
    }
    #[derive(Default)]
    pub struct Frame {
        pub begins: usize,
        pub draws: usize,
        pub publishes: usize,
        pub uploads: usize,
        pub upload_busy: bool,
        pub begin_busy: bool,
        pub draw_busy: usize,
        pub publish_busy: usize,
        pub fatal_draw: bool,
        pub(crate) lease: bool,
        pub(crate) drawn: bool,
        pub(crate) sprite_red: u8,
        pub published_red: Vec<u8>,
    }
    impl Frame {
        pub fn width(&self) -> u32 {
            2
        }
        pub fn height(&self) -> u32 {
            2
        }
        pub fn upload_sprite_rgba8(
            &mut self,
            _: u32,
            _: u32,
            _: u32,
            pixels: &[u8],
        ) -> Result<(), Error> {
            assert!(!self.lease);
            self.uploads += 1;
            if std::mem::take(&mut self.upload_busy) {
                return Err(Error::Busy);
            }
            self.sprite_red = pixels[0];
            Ok(())
        }
        pub fn begin_sprite_frame(&mut self, _: u32) -> Result<(), Error> {
            if self.lease {
                return Err(Error::InvalidState);
            }
            self.begins += 1;
            if std::mem::take(&mut self.begin_busy) {
                return Err(Error::Busy);
            }
            self.lease = true;
            self.drawn = false;
            Ok(())
        }
        pub fn draw_sprite_quads(&mut self, quads: &[SpriteQuad]) -> Result<(), Error> {
            if !self.lease {
                return Err(Error::InvalidState);
            }
            self.draws += 1;
            if self.fatal_draw {
                return Err(Error::InvalidState);
            }
            if self.draw_busy > 0 {
                self.draw_busy -= 1;
                self.lease = false;
                return Err(Error::Busy);
            }
            assert_eq!(quads.len(), 1);
            let quad = &quads[0];
            assert_eq!(quad.sprite_id, 1);
            assert!(!quad.source_over);
            assert_eq!((quad.c0.x, quad.c0.y), (0.0, 0.0));
            assert_eq!((quad.c2.x, quad.c2.y), (2.0, 2.0));
            self.drawn = true;
            Ok(())
        }
        pub fn publish(&mut self, _: Damage) -> Result<(), Error> {
            if !self.lease || !self.drawn {
                return Err(Error::InvalidState);
            }
            self.publishes += 1;
            if self.publish_busy > 0 {
                self.publish_busy -= 1;
                return Err(Error::Busy);
            }
            self.lease = false;
            self.published_red.push(self.sprite_red);
            Ok(())
        }
    }
}
#[path = "../presenter.rs"]
mod presenter;
use presenter::{Presenter, Status};

fn pixels(red: u8) -> image::RgbaImage {
    image::RgbaImage::from_pixel(2, 2, image::Rgba([red, 255, 255, 255]))
}

#[test]
fn cancelled_draw_restarts_frame_but_busy_publish_keeps_lease() {
    let mut presenter = Presenter::default();
    let mut frame = ui4_scene::Frame {
        draw_busy: 2,
        publish_busy: 2,
        ..Default::default()
    };
    let pixels = pixels(255);
    for publishing in [false, false, true, true] {
        assert_eq!(presenter.step(&mut frame, 1, || &pixels), Ok(Status::Busy));
        assert_eq!(presenter.is_publishing(), publishing);
    }
    assert_eq!(
        presenter.step(&mut frame, 1, || &pixels),
        Ok(Status::Published(1))
    );
    assert!(!presenter.is_publishing());
    assert_eq!(presenter.step(&mut frame, 1, || &pixels), Ok(Status::Idle));
    assert_eq!(
        (frame.uploads, frame.begins, frame.draws, frame.publishes),
        (1, 3, 3, 3)
    );
    assert_eq!(frame.published_red, [255]);
}

#[test]
fn new_pixels_replace_unsubmitted_viewport_at_every_busy_stage() {
    let pixels_10 = pixels(10);
    let pixels_20 = pixels(20);
    for mut frame in [
        ui4_scene::Frame {
            upload_busy: true,
            ..Default::default()
        },
        ui4_scene::Frame {
            begin_busy: true,
            ..Default::default()
        },
        ui4_scene::Frame {
            draw_busy: 1,
            ..Default::default()
        },
    ] {
        let mut presenter = Presenter::default();
        assert_eq!(
            presenter.step(&mut frame, 1, || &pixels_10),
            Ok(Status::Busy)
        );
        assert!(!presenter.is_publishing());
        assert_eq!(
            presenter.step(&mut frame, 2, || &pixels_20),
            Ok(Status::Published(2))
        );
        assert_eq!(frame.uploads, 2);
        assert_eq!(frame.published_red, [20]);
    }
}

#[test]
fn completed_draw_is_published_before_uploading_new_pixels() {
    let pixels_10 = pixels(10);
    let pixels_20 = pixels(20);
    let mut presenter = Presenter::default();
    let mut frame = ui4_scene::Frame {
        publish_busy: 1,
        ..Default::default()
    };
    assert_eq!(
        presenter.step(&mut frame, 1, || &pixels_10),
        Ok(Status::Busy)
    );
    assert!(presenter.is_publishing());
    assert_eq!(
        presenter.step(&mut frame, 2, || &pixels_20),
        Ok(Status::Published(1))
    );
    assert!(!presenter.is_publishing());
    assert_eq!(frame.uploads, 1);
    assert_eq!(
        presenter.step(&mut frame, 2, || &pixels_20),
        Ok(Status::Published(2))
    );
    assert_eq!(frame.published_red, [10, 20]);
}

#[test]
fn invalid_state_is_reported_immediately() {
    let pixels_255 = pixels(255);
    let mut presenter = Presenter::default();
    let mut frame = ui4_scene::Frame {
        fatal_draw: true,
        ..Default::default()
    };
    assert_eq!(
        presenter.step(&mut frame, 1, || &pixels_255),
        Err(ui4_scene::Error::InvalidState)
    );
    assert_eq!((frame.begins, frame.draws, frame.publishes), (1, 1, 0));
}
