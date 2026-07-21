use std::string::String;

use crate::weather::{WeatherIcon, WeatherSnapshot};

pub const FRAME_X: i32 = 480;
pub const FRAME_Y: i32 = 72;
pub const FRAME_WIDTH: u32 = 960;
pub const FRAME_HEIGHT: u32 = 640;

const BG: [u8; 4] = [9, 15, 22, 255];
// Frame-00 icon assets use the same opaque matte, so their native 1:1 edges
// disappear into icon-bearing panels without runtime alpha/JPEG work.
const PANEL: [u8; 4] = [20, 34, 56, 255];
const PANEL_2: [u8; 4] = [25, 42, 58, 255];
const BORDER: [u8; 4] = [43, 66, 84, 255];
const TEXT: [u8; 4] = [232, 239, 246, 255];
const DIM: [u8; 4] = [137, 151, 165, 255];
const ACCENT: [u8; 4] = [114, 210, 174, 255];
const BLUE: [u8; 4] = [111, 169, 242, 255];
const WARN: [u8; 4] = [255, 193, 109, 255];

#[derive(Clone, Copy)]
pub struct IconImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct TextItem {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub pixels: f32,
    pub color: [u8; 4],
}

pub struct VisualScene {
    pub pixels: Vec<u8>,
    pub text: Vec<TextItem>,
}

macro_rules! static_icon_frame {
    ($size:literal, $name:literal) => {
        include_bytes!(concat!(
            "../weather-icons/ui4-jpeg/",
            stringify!($size),
            "/",
            $name,
            "-frame-000.rgba"
        ))
        .as_slice()
    };
}

fn static_icon(icon: WeatherIcon, size: u32) -> Option<IconImage<'static>> {
    let rgba = match (size, icon) {
        (64, WeatherIcon::ClearDay) => static_icon_frame!(64, "clear-day"),
        (64, WeatherIcon::ClearNight) => static_icon_frame!(64, "clear-night"),
        (64, WeatherIcon::PartlyDay) => static_icon_frame!(64, "cloudy-2-day"),
        (64, WeatherIcon::PartlyNight) => static_icon_frame!(64, "cloudy-2-night"),
        (64, WeatherIcon::Cloud) => static_icon_frame!(64, "cloudy"),
        (64, WeatherIcon::RainDay) => static_icon_frame!(64, "rainy-1-day"),
        (64, WeatherIcon::Rain) => static_icon_frame!(64, "rainy-2"),
        (64, WeatherIcon::Thunder) => static_icon_frame!(64, "thunderstorms"),
        (64, WeatherIcon::Snow) => static_icon_frame!(64, "snowy-1"),
        (64, WeatherIcon::Fog) => static_icon_frame!(64, "fog"),
        (128, WeatherIcon::ClearDay) => static_icon_frame!(128, "clear-day"),
        (128, WeatherIcon::ClearNight) => static_icon_frame!(128, "clear-night"),
        (128, WeatherIcon::PartlyDay) => static_icon_frame!(128, "cloudy-2-day"),
        (128, WeatherIcon::PartlyNight) => static_icon_frame!(128, "cloudy-2-night"),
        (128, WeatherIcon::Cloud) => static_icon_frame!(128, "cloudy"),
        (128, WeatherIcon::RainDay) => static_icon_frame!(128, "rainy-1-day"),
        (128, WeatherIcon::Rain) => static_icon_frame!(128, "rainy-2"),
        (128, WeatherIcon::Thunder) => static_icon_frame!(128, "thunderstorms"),
        (128, WeatherIcon::Snow) => static_icon_frame!(128, "snowy-1"),
        (128, WeatherIcon::Fog) => static_icon_frame!(128, "fog"),
        _ => return None,
    };
    Some(IconImage {
        width: size,
        height: size,
        rgba,
    })
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub struct FrogVisual {
    frame: Option<trueos::ui4_scene::Frame>,
    x: i32,
    y: i32,
    active_pan: Option<trueos::ui4_solara_text::CursorSource>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl FrogVisual {
    pub const fn new() -> Self {
        Self {
            frame: None,
            x: FRAME_X,
            y: FRAME_Y,
            active_pan: None,
        }
    }

    fn ensure_frame(&mut self) -> Result<&mut trueos::ui4_scene::Frame, trueos::ui4_scene::Error> {
        if self.frame.is_none() {
            self.frame = Some(trueos::ui4_scene::Frame::open_immutable(
                self.x,
                self.y,
                FRAME_WIDTH,
                FRAME_HEIGHT,
            )?);
        }
        Ok(self.frame.as_mut().expect("Frog UI4 frame was just opened"))
    }

    fn present(&mut self, scene: &VisualScene) -> Result<(), trueos::ui4_scene::Error> {
        use trueos::ui4_scene::{Damage, Error, rgba};
        use trueos::ui4_solara_text::{Font, SceneTextRow};

        let frame = self.ensure_frame()?;
        loop {
            match frame.begin(rgba(BG[0], BG[1], BG[2], BG[3])) {
                Ok(()) => break,
                Err(Error::Busy) => {
                    trueos::vsys::poll_once();
                    trueos::vsys::sleep_ms(1);
                }
                Err(error) => return Err(error),
            }
        }
        frame.write_opaque_rgba8(scene.pixels.as_slice())?;

        for color in [TEXT, DIM, ACCENT, BLUE, WARN] {
            let rows: Vec<_> = scene
                .text
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
                frame.draw_text_scene(
                    Font::Default,
                    (FRAME_WIDTH, FRAME_HEIGHT),
                    rgba(color[0], color[1], color[2], color[3]),
                    rows.as_slice(),
                )?;
            }
        }
        frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT))
    }

    fn poll_pan(&mut self) {
        use trueos::ui4_solara_text::PanPhase;

        loop {
            let event = match self.frame.as_mut().map(|frame| frame.take_pan_event()) {
                None | Some(Ok(None)) => break,
                Some(Ok(Some(event))) => event,
                Some(Err(error)) => {
                    trueos::logl::log(
                        trueos::logl::level::WARN,
                        format_args!("Frog: UI4 pan read failed: {error:?}"),
                    );
                    break;
                }
            };
            match event.phase {
                PanPhase::Begin => self.active_pan = Some(event.source),
                PanPhase::Update if self.active_pan == Some(event.source) => {
                    // This app's middle-button pan moves the native-pixel
                    // snapshot window without repainting its immutable frame.
                    let x = self
                        .x
                        .saturating_add(event.dx)
                        .clamp(64 - FRAME_WIDTH as i32, 2_560 - 64);
                    let y = self
                        .y
                        .saturating_add(event.dy)
                        .clamp(48 - FRAME_HEIGHT as i32, 1_440 - 48);
                    if (x != self.x || y != self.y)
                        && self
                            .frame
                            .as_mut()
                            .is_some_and(|frame| frame.set_position(x, y).is_ok())
                    {
                        self.x = x;
                        self.y = y;
                    }
                }
                PanPhase::End if self.active_pan == Some(event.source) => {
                    self.active_pan = None;
                    trueos::logl::log(
                        trueos::logl::level::INFO,
                        format_args!(
                            "Frog: UI4 pan complete position={},{} native={}x{} repaint=0",
                            self.x, self.y, FRAME_WIDTH, FRAME_HEIGHT
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl crate::ui::WeatherVisual for FrogVisual {
    fn publish_snapshot(&mut self, snapshot: &WeatherSnapshot) {
        let scene = render_snapshot(snapshot, static_icon);
        if let Err(error) = self.present(&scene) {
            trueos::logl::log(
                trueos::logl::level::ERROR,
                format_args!("Frog: UI4 immutable publish failed: {error:?}"),
            );
            // A paint failure after begin can retain a write lease. Closing
            // this generation is safer than attempting to overwrite it.
            self.frame = None;
            self.active_pan = None;
        }
    }

    fn poll(&mut self) {
        self.poll_pan();
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub struct FrogVisual;

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl FrogVisual {
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl crate::ui::WeatherVisual for FrogVisual {
    fn publish_snapshot(&mut self, snapshot: &WeatherSnapshot) {
        let _ = render_snapshot(snapshot, static_icon);
    }
}

/// Build a fixed-size native-pixel Frog scene. `icons` must return the first
/// (static) animation frame at the requested size; no scaling is performed.
pub fn render_snapshot<'a, F>(snapshot: &WeatherSnapshot, mut icons: F) -> VisualScene
where
    F: FnMut(WeatherIcon, u32) -> Option<IconImage<'a>>,
{
    let mut canvas = Canvas::new();
    let mut text = Vec::with_capacity(72);

    panel(&mut canvas, 20, 18, 920, 44, PANEL);
    label(&mut text, "FROG WEATHER", 38.0, 29.0, 20.0, ACCENT);
    label(
        &mut text,
        format!(
            "{} · {}    {:.4}, {:.4}",
            snapshot.location.name,
            snapshot.location.country,
            snapshot.location.lat,
            snapshot.location.lon
        ),
        224.0,
        31.0,
        15.0,
        TEXT,
    );

    panel(&mut canvas, 20, 74, 920, 164, PANEL);
    canvas.rect(20, 74, 6, 164, ACCENT);
    if let Some(current) = snapshot.current.as_ref() {
        icon_or_placeholder(
            &mut canvas,
            &mut text,
            icons(current.icon, 128),
            current.icon,
            42,
            92,
            128,
        );
        label(
            &mut text,
            format!("{}°C", current.temp_c),
            194.0,
            91.0,
            45.0,
            TEXT,
        );
        label(
            &mut text,
            ellipsize(current.summary.as_str(), 44),
            196.0,
            145.0,
            21.0,
            ACCENT,
        );
        label(
            &mut text,
            format!(
                "feels {}°C    humidity {}%    wind {} km/h",
                current.feels_c, current.humidity, current.wind_kmh
            ),
            196.0,
            183.0,
            16.0,
            DIM,
        );
    } else {
        label(
            &mut text,
            "Current weather unavailable",
            48.0,
            118.0,
            24.0,
            WARN,
        );
    }
    label(
        &mut text,
        ellipsize(snapshot.source.as_str(), 84),
        196.0,
        211.0,
        13.0,
        DIM,
    );

    label(&mut text, "8 DAY FORECAST", 24.0, 252.0, 16.0, DIM);
    for (index, day) in snapshot.days.iter().take(8).enumerate() {
        let column = index % 4;
        let row = index / 4;
        let x = 20 + column as i32 * 232;
        let y = 276 + row as i32 * 146;
        panel(&mut canvas, x, y, 224, 134, PANEL);
        icon_or_placeholder(
            &mut canvas,
            &mut text,
            icons(day.icon, 64),
            day.icon,
            x + 10,
            y + 35,
            64,
        );
        label(
            &mut text,
            day.weekday,
            (x + 12) as f32,
            (y + 11) as f32,
            17.0,
            ACCENT,
        );
        label(
            &mut text,
            ellipsize(day.summary.as_str(), 17),
            (x + 84) as f32,
            (y + 12) as f32,
            13.0,
            TEXT,
        );
        label(
            &mut text,
            format!(
                "{}°  {}°/{}°",
                day.temp_day_c, day.temp_min_c, day.temp_max_c
            ),
            (x + 84) as f32,
            (y + 39) as f32,
            16.0,
            TEXT,
        );
        label(
            &mut text,
            format!("feels {}° · night {}°", day.feels_day_c, day.temp_night_c),
            (x + 84) as f32,
            (y + 63) as f32,
            12.0,
            DIM,
        );
        label(
            &mut text,
            format!("rain {}% · humidity {}%", day.rain_percent, day.humidity),
            (x + 84) as f32,
            (y + 84) as f32,
            12.0,
            BLUE,
        );
        label(
            &mut text,
            format!(
                "wind {} {} km/h · UVI {}",
                day.wind_dir, day.wind_kmh, day.uvi
            ),
            (x + 12) as f32,
            (y + 111) as f32,
            12.0,
            DIM,
        );
        let rain_width = ((day.rain_percent.clamp(0, 100) as u32 * 64) / 100) as i32;
        canvas.rect(x + 10, y + 104, 64, 3, BORDER);
        canvas.rect(x + 10, y + 104, rain_width, 3, BLUE);
    }

    panel(&mut canvas, 20, 577, 920, 43, PANEL);
    let footer = if snapshot.note.is_empty() {
        snapshot.updated_line.as_str()
    } else {
        snapshot.note.as_str()
    };
    label(
        &mut text,
        ellipsize(footer, 116),
        36.0,
        590.0,
        13.0,
        if snapshot.note.is_empty() { DIM } else { WARN },
    );

    VisualScene {
        pixels: canvas.pixels,
        text,
    }
}

fn icon_or_placeholder(
    canvas: &mut Canvas,
    text: &mut Vec<TextItem>,
    icon: Option<IconImage<'_>>,
    kind: WeatherIcon,
    x: i32,
    y: i32,
    expected_size: u32,
) {
    match icon {
        Some(icon)
            if icon.width == expected_size
                && icon.height == expected_size
                && icon.rgba.len() == expected_size as usize * expected_size as usize * 4 =>
        {
            canvas.blit_rgba(x, y, icon);
        }
        _ => {
            canvas.rect(x, y, expected_size as i32, expected_size as i32, PANEL_2);
            label(
                text,
                kind.glyph(),
                (x + expected_size as i32 / 3) as f32,
                (y + expected_size as i32 / 3) as f32,
                expected_size as f32 * 0.42,
                WARN,
            );
        }
    }
}

fn panel(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, fill: [u8; 4]) {
    canvas.rect(x, y, width, height, BORDER);
    canvas.rect(x + 1, y + 1, width - 2, height - 2, fill);
}

fn label(
    text: &mut Vec<TextItem>,
    value: impl Into<String>,
    x: f32,
    y: f32,
    pixels: f32,
    color: [u8; 4],
) {
    text.push(TextItem {
        text: value.into(),
        x,
        y,
        pixels,
        color,
    });
}

fn ellipsize(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let mut output: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        let mut canvas = Self {
            pixels: vec![0; FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 4],
        };
        canvas.rect(0, 0, FRAME_WIDTH as i32, FRAME_HEIGHT as i32, BG);
        canvas
    }

    fn rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
        if width <= 0 || height <= 0 {
            return;
        }
        let left = x.max(0).min(FRAME_WIDTH as i32) as usize;
        let top = y.max(0).min(FRAME_HEIGHT as i32) as usize;
        let right = x.saturating_add(width).max(0).min(FRAME_WIDTH as i32) as usize;
        let bottom = y.saturating_add(height).max(0).min(FRAME_HEIGHT as i32) as usize;
        for row in top..bottom {
            let start = (row * FRAME_WIDTH as usize + left) * 4;
            let end = (row * FRAME_WIDTH as usize + right) * 4;
            for pixel in self.pixels[start..end].chunks_exact_mut(4) {
                pixel.copy_from_slice(&color);
            }
        }
    }

    fn blit_rgba(&mut self, x: i32, y: i32, image: IconImage<'_>) {
        for source_y in 0..image.height as i32 {
            let destination_y = y + source_y;
            if !(0..FRAME_HEIGHT as i32).contains(&destination_y) {
                continue;
            }
            for source_x in 0..image.width as i32 {
                let destination_x = x + source_x;
                if !(0..FRAME_WIDTH as i32).contains(&destination_x) {
                    continue;
                }
                let source = (source_y as usize * image.width as usize + source_x as usize) * 4;
                let destination =
                    (destination_y as usize * FRAME_WIDTH as usize + destination_x as usize) * 4;
                let alpha = image.rgba[source + 3] as u16;
                for channel in 0..3 {
                    let foreground = image.rgba[source + channel] as u16;
                    let background = self.pixels[destination + channel] as u16;
                    self.pixels[destination + channel] =
                        ((foreground * alpha + background * (255 - alpha) + 127) / 255) as u8;
                }
                self.pixels[destination + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_scene_is_full_opaque_native_frame() {
        let scene = render_snapshot(&crate::weather::demo_snapshot(), |_icon, _size| None);
        assert_eq!(
            scene.pixels.len(),
            FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 4
        );
        assert!(scene.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(!scene.text.is_empty());
    }

    #[test]
    fn icon_is_blitted_without_scaling() {
        let source = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 1, 2, 3, 255];
        let mut canvas = Canvas::new();
        canvas.blit_rgba(
            7,
            9,
            IconImage {
                width: 2,
                height: 2,
                rgba: &source,
            },
        );
        let first = (9 * FRAME_WIDTH as usize + 7) * 4;
        let second_row = (10 * FRAME_WIDTH as usize + 7) * 4;
        assert_eq!(&canvas.pixels[first..first + 8], &source[..8]);
        assert_eq!(&canvas.pixels[second_row..second_row + 8], &source[8..]);
    }
}
