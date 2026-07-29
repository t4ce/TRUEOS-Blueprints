use anyhow::Result;

use crate::weather::{WeatherIcon, WeatherSnapshot};

pub const FRAME_WIDTH: u32 = 1920;
pub const FRAME_HEIGHT: u32 = 1080;

const FRAME_ALPHA: u8 = 64;
const ICON_SIZE: u32 = 64;
const ANIMATION_FRAME_COUNT: usize = 10;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const KEY_ESCAPE: u8 = 0x29;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const KEY_SPACE: u8 = 0x2c;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const INPUT_POLL_MS: u64 = 16;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const ANIMATION_POLL_MS: u64 = 100;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const TEXT_SPRITE_ID: u32 = u32::MAX;

const TEXT: [u8; 4] = [232, 239, 246, 255];
const DIM: [u8; 4] = [145, 160, 176, 255];
const ACCENT: [u8; 4] = [114, 210, 174, 255];

macro_rules! animation_frames {
    ($name:literal) => {
        [
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-000.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-001.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-002.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-003.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-004.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-005.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-006.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-007.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-008.rgba"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../weather-icons/ui4-rgba/64/",
                $name,
                "-frame-009.rgba"
            ))
            .as_slice(),
        ]
    };
}

const CLEAR_DAY_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("clear-day");
const CLEAR_NIGHT_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("clear-night");
const PARTLY_DAY_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("cloudy-2-day");
const PARTLY_NIGHT_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("cloudy-2-night");
const CLOUD_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("cloudy");
const RAIN_DAY_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("rainy-1-day");
const RAIN_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("rainy-2");
const THUNDER_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("thunderstorms");
const SNOW_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("snowy-1");
const FOG_FRAMES: [&[u8]; ANIMATION_FRAME_COUNT] = animation_frames!("fog-day");

struct IconAnimation {
    family: usize,
    period_ms: u64,
    frames: &'static [&'static [u8]; ANIMATION_FRAME_COUNT],
}

fn icon_animation(icon: WeatherIcon) -> IconAnimation {
    match icon {
        WeatherIcon::ClearDay => IconAnimation {
            family: 0,
            period_ms: 9_000,
            frames: &CLEAR_DAY_FRAMES,
        },
        WeatherIcon::ClearNight => IconAnimation {
            family: 1,
            period_ms: 6_000,
            frames: &CLEAR_NIGHT_FRAMES,
        },
        WeatherIcon::PartlyDay => IconAnimation {
            family: 2,
            period_ms: 9_000,
            frames: &PARTLY_DAY_FRAMES,
        },
        WeatherIcon::PartlyNight => IconAnimation {
            family: 3,
            period_ms: 6_000,
            frames: &PARTLY_NIGHT_FRAMES,
        },
        WeatherIcon::Cloud => IconAnimation {
            family: 4,
            period_ms: 7_000,
            frames: &CLOUD_FRAMES,
        },
        WeatherIcon::RainDay => IconAnimation {
            family: 5,
            period_ms: 9_000,
            frames: &RAIN_DAY_FRAMES,
        },
        WeatherIcon::Rain => IconAnimation {
            family: 6,
            period_ms: 8_000,
            frames: &RAIN_FRAMES,
        },
        WeatherIcon::Thunder => IconAnimation {
            family: 7,
            period_ms: 7_000,
            frames: &THUNDER_FRAMES,
        },
        WeatherIcon::Snow => IconAnimation {
            family: 8,
            period_ms: 3_000,
            frames: &SNOW_FRAMES,
        },
        WeatherIcon::Fog => IconAnimation {
            family: 9,
            period_ms: 20_000,
            frames: &FOG_FRAMES,
        },
    }
}

fn animation_frame(icon: WeatherIcon, elapsed_ms: u64) -> usize {
    let animation = icon_animation(icon);
    ((elapsed_ms % animation.period_ms) * ANIMATION_FRAME_COUNT as u64 / animation.period_ms)
        as usize
}

fn sprite_id(icon: WeatherIcon, frame: usize) -> u32 {
    1 + (icon_animation(icon).family * ANIMATION_FRAME_COUNT + frame) as u32
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct SceneLayout {
    general: Rect,
    forecasts: Vec<Rect>,
}

fn scene_layout(width: u32, height: u32, forecast_count: usize) -> SceneLayout {
    let margin = (width as f32 * 0.021).clamp(24.0, 48.0);
    let gap = 8.0;
    let general_height = (height as f32 * 0.15).clamp(136.0, 196.0);
    let content_width = (width as f32 - margin * 2.0).max(1.0);
    let general = Rect {
        x: margin,
        y: margin,
        width: content_width,
        height: general_height,
    };

    let mut forecasts = Vec::with_capacity(forecast_count);
    if forecast_count != 0 {
        let top = general.y + general.height + gap;
        let available = (height as f32 - margin - top - gap * (forecast_count - 1) as f32).max(1.0);
        let row_height = available / forecast_count as f32;
        for index in 0..forecast_count {
            forecasts.push(Rect {
                x: margin,
                y: top + index as f32 * (row_height + gap),
                width: content_width,
                height: row_height,
            });
        }
    }

    SceneLayout { general, forecasts }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
struct TextItem {
    text: String,
    x: f32,
    y: f32,
    pixels: f32,
    color: [u8; 4],
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub struct FrogVisual {
    frame: trueos::ui4_scene::Frame,
    snapshot: Option<WeatherSnapshot>,
    uploaded: [bool; 10],
    animation_started: std::time::Instant,
    last_animation_poll: std::time::Instant,
    last_signature: u64,
    text_ready: bool,
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
        let now = std::time::Instant::now();
        let mut visual = Self {
            frame,
            snapshot: None,
            uploaded: [false; 10],
            animation_started: now,
            last_animation_poll: now,
            last_signature: u64::MAX,
            text_ready: false,
        };
        visual
            .present_blank()
            .map_err(|error| anyhow!("present Frog UI4 frame: {error:?}"))?;
        Ok(visual)
    }

    pub fn show_snapshot(&mut self, snapshot: WeatherSnapshot) -> Result<()> {
        use anyhow::anyhow;

        let mut icons = Vec::with_capacity(snapshot.days.len() + 1);
        if let Some(current) = snapshot.current.as_ref() {
            icons.push(current.icon);
        }
        icons.extend(snapshot.days.iter().map(|day| day.icon));
        for icon in icons {
            self.ensure_animation_uploaded(icon)
                .map_err(|error| anyhow!("upload Frog weather animation: {error:?}"))?;
        }

        self.snapshot = Some(snapshot);
        self.animation_started = std::time::Instant::now();
        self.last_signature = u64::MAX;
        self.text_ready = false;
        self.render_scene(true)
            .map_err(|error| anyhow!("present Frog weather scene: {error:?}"))
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

            if self.last_animation_poll.elapsed()
                >= std::time::Duration::from_millis(ANIMATION_POLL_MS)
            {
                self.last_animation_poll = std::time::Instant::now();
                let signature = self.animation_signature();
                if signature != self.last_signature {
                    self.render_scene(false)
                        .map_err(|error| anyhow!("animate Frog weather scene: {error:?}"))?;
                }
            }

            trueos::vsys::poll_once();
            trueos::vsys::sleep_ms(INPUT_POLL_MS);
        }
    }

    fn ensure_animation_uploaded(
        &mut self,
        icon: WeatherIcon,
    ) -> Result<(), trueos::ui4_scene::Error> {
        let animation = icon_animation(icon);
        if self.uploaded[animation.family] {
            return Ok(());
        }
        for (index, rgba) in animation.frames.iter().enumerate() {
            self.frame
                .upload_sprite_rgba8(sprite_id(icon, index), ICON_SIZE, ICON_SIZE, rgba)?;
        }
        self.uploaded[animation.family] = true;
        Ok(())
    }

    fn maximize(&mut self) -> Result<(), trueos::ui4_scene::Error> {
        let (width, height) = trueos::ui4_scene::output_dimensions()?;
        if self.frame.width() != width || self.frame.height() != height {
            self.frame.resize(width, height)?;
        }
        self.frame.set_position(0, 0)?;
        self.text_ready = false;
        if self.snapshot.is_some() {
            self.render_scene(true)
        } else {
            self.present_blank()
        }
    }

    fn render_scene(&mut self, rebuild_text: bool) -> Result<(), trueos::ui4_scene::Error> {
        use trueos::ui4_scene::{Damage, Error, rgba};
        use trueos::ui4_solara_text::{Font, SceneTextRow};

        let elapsed_ms = self.animation_started.elapsed().as_millis() as u64;
        let width = self.frame.width();
        let height = self.frame.height();
        let snapshot = self
            .snapshot
            .as_ref()
            .expect("weather scene requires a snapshot");
        let layout = scene_layout(width, height, snapshot.days.len());
        let text = rebuild_text.then(|| build_text(snapshot, &layout));
        let mut quads = Vec::with_capacity(layout.forecasts.len() * 2 + 5);
        quads.push(sprite_quad(
            0,
            Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            rgba(0, 0, 0, FRAME_ALPHA),
            true,
        ));
        quads.extend(build_quads(snapshot, &layout, elapsed_ms));
        quads.push(sprite_quad(
            TEXT_SPRITE_ID,
            Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            rgba(255, 255, 255, 255),
            true,
        ));

        loop {
            match self.frame.begin_sprite_frame(rgba(0, 0, 0, 0)) {
                Ok(()) => break,
                Err(Error::Busy) => {
                    trueos::vsys::poll_once();
                    trueos::vsys::sleep_ms(1);
                }
                Err(error) => return Err(error),
            }
        }

        if let Some(text) = text.as_ref() {
            for color in [TEXT, DIM, ACCENT] {
                let rows: Vec<_> = text
                    .iter()
                    .filter(|item| item.color == color)
                    .map(|item| SceneTextRow {
                        text: item.text.as_str(),
                        x: item.x,
                        y: item.y,
                        font_pixels: item.pixels,
                    })
                    .collect();
                if !rows.is_empty() {
                    loop {
                        match self.frame.retain_text_backbuffer(
                            Font::Inconsolata,
                            (width, height),
                            rgba(color[0], color[1], color[2], color[3]),
                            rows.as_slice(),
                        ) {
                            Ok(()) => break,
                            Err(Error::Busy) => {
                                trueos::vsys::poll_once();
                                trueos::vsys::sleep_ms(1);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
            self.text_ready = true;
        }

        if !self.text_ready {
            return Err(trueos::ui4_scene::Error::InvalidState);
        }
        self.frame.draw_sprite_quads(quads.as_slice())?;
        self.frame.publish(Damage::full(width, height))?;
        self.last_signature = self.animation_signature();
        Ok(())
    }

    fn animation_signature(&self) -> u64 {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return 0;
        };
        let elapsed_ms = self.animation_started.elapsed().as_millis() as u64;
        let mut signature = snapshot
            .current
            .as_ref()
            .map(|current| animation_frame(current.icon, elapsed_ms) as u64)
            .unwrap_or(0);
        for day in &snapshot.days {
            signature = signature.rotate_left(4)
                ^ animation_frame(day.icon, elapsed_ms) as u64
                ^ (icon_animation(day.icon).family as u64) << 4;
        }
        signature
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

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn build_quads(
    snapshot: &WeatherSnapshot,
    layout: &SceneLayout,
    elapsed_ms: u64,
) -> Vec<trueos::ui4_scene::SpriteQuad> {
    use trueos::ui4_scene::rgba;

    let mut quads = Vec::with_capacity(layout.forecasts.len() * 2 + 3);
    quads.push(sprite_quad(0, layout.general, rgba(9, 15, 22, 176), true));
    if let Some(current) = snapshot.current.as_ref() {
        let icon_size = layout.general.height.min(112.0) - 24.0;
        let icon_rect = Rect {
            x: layout.general.x + 20.0,
            y: layout.general.y + (layout.general.height - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        };
        quads.push(sprite_quad(
            sprite_id(current.icon, animation_frame(current.icon, elapsed_ms)),
            icon_rect,
            rgba(255, 255, 255, 255),
            true,
        ));
    }

    for (day, row) in snapshot.days.iter().zip(&layout.forecasts) {
        quads.push(sprite_quad(0, *row, rgba(9, 15, 22, 150), true));
        let icon_size = row.height.min(76.0) - 12.0;
        quads.push(sprite_quad(
            sprite_id(day.icon, animation_frame(day.icon, elapsed_ms)),
            Rect {
                x: row.x + 18.0,
                y: row.y + (row.height - icon_size) * 0.5,
                width: icon_size,
                height: icon_size,
            },
            rgba(255, 255, 255, 255),
            true,
        ));
    }
    quads
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn sprite_quad(
    sprite_id: u32,
    rect: Rect,
    color_rgba: u32,
    source_over: bool,
) -> trueos::ui4_scene::SpriteQuad {
    use trueos::ui4_scene::{SpriteCorner, SpriteQuad};

    SpriteQuad {
        sprite_id,
        c0: SpriteCorner {
            x: rect.x,
            y: rect.y,
            u: 0.0,
            v: 0.0,
        },
        c1: SpriteCorner {
            x: rect.x + rect.width,
            y: rect.y,
            u: 1.0,
            v: 0.0,
        },
        c2: SpriteCorner {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
            u: 1.0,
            v: 1.0,
        },
        c3: SpriteCorner {
            x: rect.x,
            y: rect.y + rect.height,
            u: 0.0,
            v: 1.0,
        },
        color_rgba,
        source_over,
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn build_text(snapshot: &WeatherSnapshot, layout: &SceneLayout) -> Vec<TextItem> {
    let mut text = Vec::with_capacity(snapshot.days.len() * 2 + 3);
    let general_text_x = layout.general.x + layout.general.height.min(112.0) + 32.0;
    text.push(TextItem {
        text: format!(
            "{} · {}    {:.4}, {:.4}",
            snapshot.location.name,
            snapshot.location.country,
            snapshot.location.lat,
            snapshot.location.lon
        ),
        x: general_text_x,
        y: layout.general.y + 18.0,
        pixels: 30.0,
        color: ACCENT,
    });
    text.push(TextItem {
        text: snapshot.source.clone(),
        x: general_text_x,
        y: layout.general.y + 56.0,
        pixels: 17.0,
        color: DIM,
    });
    if let Some(current) = snapshot.current.as_ref() {
        text.push(TextItem {
            text: format!(
                "{}C · feels {}C · {} · humidity {}% · wind {} km/h",
                current.temp_c,
                current.feels_c,
                current.summary,
                current.humidity,
                current.wind_kmh
            ),
            x: general_text_x,
            y: layout.general.y + 88.0,
            pixels: 23.0,
            color: TEXT,
        });
    }

    for (day, row) in snapshot.days.iter().zip(&layout.forecasts) {
        let text_x = row.x + 100.0;
        text.push(TextItem {
            text: format!("{}  {}", day.weekday, ellipsize(day.summary.as_str(), 88)),
            x: text_x,
            y: row.y + 12.0,
            pixels: 21.0,
            color: TEXT,
        });
        text.push(TextItem {
            text: format!(
                "day {}C · feels {}C · {}..{}C · night {}C · rain {}% · humidity {}% · wind {} km/h {} · UV {}",
                day.temp_day_c,
                day.feels_day_c,
                day.temp_min_c,
                day.temp_max_c,
                day.temp_night_c,
                day.rain_percent,
                day.humidity,
                day.wind_kmh,
                day.wind_dir,
                day.uvi
            ),
            x: text_x,
            y: row.y + 46.0,
            pixels: 17.0,
            color: DIM,
        });
    }
    text
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn ellipsize(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let mut output: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub struct FrogVisual;

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl FrogVisual {
    pub fn open() -> Result<Self> {
        Ok(Self)
    }

    pub fn show_snapshot(&mut self, _snapshot: WeatherSnapshot) -> Result<()> {
        Ok(())
    }

    pub fn wait_for_escape(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hd_layout_has_one_general_and_eight_forecast_rows() {
        let layout = scene_layout(FRAME_WIDTH, FRAME_HEIGHT, 8);
        assert_eq!(layout.forecasts.len(), 8);
        assert!(layout.general.height > layout.forecasts[0].height);
        assert!(
            layout
                .forecasts
                .windows(2)
                .all(|rows| rows[0].y < rows[1].y)
        );
        let last = layout.forecasts.last().unwrap();
        assert!(last.y + last.height <= FRAME_HEIGHT as f32);
    }

    #[test]
    fn every_weather_icon_has_ten_embedded_rgba_frames() {
        for icon in WeatherIcon::ALL {
            let animation = icon_animation(icon);
            assert!(animation.period_ms > 0);
            assert!(
                animation
                    .frames
                    .iter()
                    .all(|frame| frame.len() == ICON_SIZE as usize * ICON_SIZE as usize * 4)
            );
        }
    }

    #[test]
    fn black_frame_uses_quarter_alpha() {
        assert_eq!(FRAME_ALPHA as u16, (u8::MAX as u16 + 1) / 4);
    }
}
