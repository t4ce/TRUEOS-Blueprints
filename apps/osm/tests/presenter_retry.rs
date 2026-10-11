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
pub mod vsys {
    pub fn poll_once() {}
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
        pub draw_busy: usize,
        pub publish_busy: usize,
        pub fatal_draw: bool,
        pub(crate) lease: bool,
        pub(crate) drawn: bool,
        pub source_over: bool,
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
            _: &[u8],
        ) -> Result<(), Error> {
            self.uploads += 1;
            Ok(())
        }
        pub fn begin_sprite_frame(&mut self, _: u32) -> Result<(), Error> {
            if self.lease {
                return Err(Error::InvalidState);
            }
            self.begins += 1;
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
            self.source_over = quads[0].source_over;
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
            Ok(())
        }
    }
}
#[path = "../presenter.rs"]
mod presenter;

#[test]
fn cancelled_draw_restarts_frame_but_busy_publish_keeps_lease() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    for logo in [false, true] {
        let mut frame = ui4_scene::Frame {
            draw_busy: 2,
            publish_busy: 2,
            ..Default::default()
        };
        let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([255; 4]));
        let result = if logo {
            rt.block_on(presenter::present_logo(&mut frame, &pixels))
        } else {
            rt.block_on(presenter::present(&mut frame, &pixels))
        };
        assert_eq!(result, Ok(()));
        assert_eq!(
            (frame.uploads, frame.begins, frame.draws, frame.publishes),
            (1, 3, 3, 3)
        );
        assert_eq!(frame.source_over, logo);
    }
}

#[test]
fn invalid_state_is_reported_without_retrying_forever() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let mut frame = ui4_scene::Frame {
        fatal_draw: true,
        ..Default::default()
    };
    let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([255; 4]));
    assert_eq!(
        rt.block_on(presenter::present(&mut frame, &pixels)),
        Err(ui4_scene::Error::InvalidState)
    );
    assert_eq!((frame.begins, frame.draws, frame.publishes), (1, 1, 0));
}
