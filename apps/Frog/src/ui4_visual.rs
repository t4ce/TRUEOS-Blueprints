use anyhow::Result;

pub const FRAME_WIDTH: u32 = 1920;
pub const FRAME_HEIGHT: u32 = 1080;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const FRAME_ALPHA: u8 = 64;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const KEY_ESCAPE: u8 = 0x29;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const KEY_SPACE: u8 = 0x2c;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const INPUT_POLL_MS: u64 = 16;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub struct FrogVisual {
    frame: trueos::ui4_scene::Frame,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl FrogVisual {
    pub fn open() -> Result<Self> {
        use anyhow::anyhow;
        use trueos::ui4_scene::{Frame, output_dimensions};

        let (x, y) = output_dimensions()
            .map(|(width, height)| {
                (
                    width.saturating_sub(FRAME_WIDTH) as i32 / 2,
                    height.saturating_sub(FRAME_HEIGHT) as i32 / 2,
                )
            })
            .unwrap_or((0, 0));
        let frame = Frame::open_immutable(x, y, FRAME_WIDTH, FRAME_HEIGHT)
            .map_err(|error| anyhow!("open Frog UI4 frame: {error:?}"))?;
        let mut visual = Self { frame };
        visual
            .present_blank()
            .map_err(|error| anyhow!("present Frog UI4 frame: {error:?}"))?;
        Ok(visual)
    }

    pub fn wait_for_escape(mut self) -> Result<()> {
        use anyhow::anyhow;

        let mut space_was_down = false;
        loop {
            let keyboard = self
                .frame
                .keyboard_state()
                .map_err(|error| anyhow!("read Frog UI4 keyboard state: {error:?}"))?;
            let escape_down = keyboard
                .as_ref()
                .is_some_and(|state| state.is_down(KEY_ESCAPE));
            if escape_down {
                self.frame
                    .close(trueos::ui4_scene::CloseRequest::default())
                    .map_err(|error| anyhow!("close Frog UI4 frame: {error:?}"))?;
                return Ok(());
            }

            let space_down = keyboard
                .as_ref()
                .is_some_and(|state| state.is_down(KEY_SPACE));
            if space_down && !space_was_down {
                self.maximize()
                    .map_err(|error| anyhow!("maximize Frog UI4 frame: {error:?}"))?;
            }
            space_was_down = space_down;

            trueos::vsys::poll_once();
            trueos::vsys::sleep_ms(INPUT_POLL_MS);
        }
    }

    fn maximize(&mut self) -> Result<(), trueos::ui4_scene::Error> {
        let (width, height) = trueos::ui4_scene::output_dimensions()?;
        if self.frame.width() != width || self.frame.height() != height {
            self.frame.resize(width, height)?;
        }
        self.frame.set_position(0, 0)?;
        self.present_blank()
    }

    fn present_blank(&mut self) -> Result<(), trueos::ui4_scene::Error> {
        use trueos::ui4_scene::{Damage, Error, rgba};

        loop {
            match self.frame.begin(rgba(0, 0, 0, FRAME_ALPHA)) {
                Ok(()) => break,
                Err(Error::Busy) => {
                    trueos::vsys::poll_once();
                    trueos::vsys::sleep_ms(1);
                }
                Err(error) => return Err(error),
            }
        }
        self.frame
            .publish(Damage::full(self.frame.width(), self.frame.height()))
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub struct FrogVisual;

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl FrogVisual {
    pub fn open() -> Result<Self> {
        Ok(Self)
    }

    pub fn wait_for_escape(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_frame_is_full_hd() {
        assert_eq!((FRAME_WIDTH, FRAME_HEIGHT), (1920, 1080));
    }

    #[test]
    fn black_frame_uses_quarter_alpha() {
        assert_eq!(64, (u8::MAX as u16 + 1) / 4);
    }
}
