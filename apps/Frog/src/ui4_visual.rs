use std::string::String;

use crate::weather::{WeatherIcon, WeatherSnapshot};

pub const FRAME_X: i32 = 480;
pub const FRAME_Y: i32 = 72;
pub const FRAME_WIDTH: u32 = 960;
pub const FRAME_HEIGHT: u32 = 640;

const BG: [u8; 4] = [9, 15, 22, 255];
const PANEL: [u8; 4] = [20, 34, 48, 255];
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
        panel(
            &mut canvas,
            x,
            y,
            224,
            134,
            if index % 2 == 0 { PANEL } else { PANEL_2 },
        );
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
            format!("{}°  {}°/{}°", day.temp_day_c, day.temp_min_c, day.temp_max_c),
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
            format!("wind {} {} km/h · UVI {}", day.wind_dir, day.wind_kmh, day.uvi),
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
        let right = x
            .saturating_add(width)
            .max(0)
            .min(FRAME_WIDTH as i32) as usize;
        let bottom = y
            .saturating_add(height)
            .max(0)
            .min(FRAME_HEIGHT as i32) as usize;
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
