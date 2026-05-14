// trueos-blueprint: features=["tokio-runtime"]

use trueos::ui2::{self, gfx};
use trueos::{
    bp_error, bp_info,
    platform::{format, vec, String, ToString, Vec},
    runtime, time,
};

const WINDOW_TITLE: &str = "Weather BP";
const WINDOW_X: i32 = 72;
const WINDOW_Y: i32 = 72;
const WINDOW_WIDTH: u32 = 560;
const WINDOW_HEIGHT: u32 = 530;
const TEX_ID: u32 = 4_730;
const WEATHER_CITY: &str = "Holzminden";
const WEATHER_API_KEY: &str = "9715912a7d8748d65bc3985b4a4274a0";
const DEMO_JSON: &str = include_str!("../crates/trueos-weather/src/demo.json");
const FETCH_TIMEOUT_MS: u64 = 45_000;
const REFRESH_SECS: u64 = 3_600;
const TRANSPORT_LOG: &str =
    "transport: shared trueos-weather fetch helper; models/config decode via trueos-weather";
const DAILY_ROW_COUNT: usize = 8;

const BG_RGBA: [u8; 4] = [0x18, 0x1C, 0x24, 0xFF];
const PANEL_RGBA: [u8; 4] = [0x21, 0x27, 0x31, 0xFF];
const ROW_RGBA: [u8; 4] = [0x1D, 0x23, 0x2C, 0xFF];
const TEXT_RGBA: [u8; 4] = [0xEE, 0xF3, 0xF9, 0xFF];
const DIM_RGBA: [u8; 4] = [0x9A, 0xA8, 0xB8, 0xFF];
const ACCENT_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const WARN_RGBA: [u8; 4] = [0xFF, 0xC6, 0x6D, 0xFF];
const SUN_RGBA: [u8; 4] = [0xFF, 0xD4, 0x66, 0xFF];
const MOON_RGBA: [u8; 4] = [0xC9, 0xD4, 0xEE, 0xFF];
const CLOUD_RGBA: [u8; 4] = [0xB8, 0xC5, 0xD4, 0xFF];
const RAIN_RGBA: [u8; 4] = [0x5D, 0xAE, 0xFF, 0xFF];
const SNOW_RGBA: [u8; 4] = [0xE8, 0xF7, 0xFF, 0xFF];
const FOG_RGBA: [u8; 4] = [0x93, 0xA2, 0xAF, 0xFF];
const THUNDER_RGBA: [u8; 4] = [0xFF, 0xDF, 0x5D, 0xFF];

const WEATHER_FONT_TIER: ui2::FontTier = ui2::FontTier::Third;
const PAD: usize = 14;
const ICON_TEXT_X: usize = PAD + 30;

#[derive(Clone, Copy, Debug)]
enum WeatherIcon {
    ClearDay,
    ClearNight,
    PartlyDay,
    PartlyNight,
    Cloud,
    RainDay,
    Rain,
    Thunder,
    Snow,
    Fog,
}

#[derive(Clone, Debug)]
struct GeoResult {
    name: String,
    country: String,
    lat: f64,
    lon: f64,
}

#[derive(Clone, Debug)]
struct WeatherRow {
    icon: WeatherIcon,
    day: &'static str,
    summary: String,
    temp_line: String,
}

#[derive(Clone, Debug)]
struct WeatherSnapshot {
    header: String,
    subheader: String,
    rows: Vec<WeatherRow>,
    note: String,
}

fn main() {
    bp_info!("weather_bp: start");
    let runtime = match runtime::current_thread().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("weather_bp: runtime build failed: {}", err);
            return;
        }
    };

    let Some(window) = ui2::SurfaceWindow::create(
        WINDOW_TITLE,
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TEX_ID,
    ) else {
        bp_error!("weather_bp: ui2 surface window create failed");
        return;
    };
    let _ = window
        .id()
        .set_decorations(ui2::WindowDecorationMode::Client);
    let _ = window.id().set_vertical_scrollbar_visible(false);
    let _ = window.id().set_horizontal_scrollbar_visible(false);

    let loading = WeatherSnapshot {
        header: String::from("Weather BP"),
        subheader: String::from("Fetching live OpenWeather forecast"),
        rows: Vec::new(),
        note: String::new(),
    };
    present_snapshot(&window, &loading);

    runtime.block_on(async {
        if let Err(err) = run_weather_loop(&window).await {
            bp_error!("weather_bp: {}", err);
            let snapshot = WeatherSnapshot {
                header: String::from("Weather BP"),
                subheader: format!("failed: {}", err),
                rows: Vec::new(),
                note: String::from("weather app stopped"),
            };
            present_snapshot(&window, &snapshot);
        }
    });
}

async fn run_weather_loop(window: &ui2::SurfaceWindow) -> Result<(), &'static str> {
    bp_info!("weather_bp: {}", TRANSPORT_LOG);

    loop {
        let snapshot = load_weather_snapshot().await;
        present_snapshot(window, &snapshot);
        time::sleep(time::Duration::from_secs(REFRESH_SECS)).await;
    }
}

async fn load_weather_snapshot() -> WeatherSnapshot {
    let mut note = String::new();
    let geo_url = format!(
        "{}?q={}&limit=1&appid={}",
        trueos_weather::config::GEO_URL,
        WEATHER_CITY,
        WEATHER_API_KEY
    );

    let geo = match trueos_weather::transport::fetch_text(geo_url.as_str(), FETCH_TIMEOUT_MS) {
        Ok(raw) => parse_geo_response(raw.as_str()),
        Err(err) => {
            bp_info!("weather_bp: geo fetch failed: {}; using bundled coordinates", err);
            note = format!("geo lookup failed: {}; using saved coordinates", err);
            None
        }
    };

    let geo = geo.unwrap_or_else(|| GeoResult {
        name: String::from(WEATHER_CITY),
        country: String::from("DE"),
        lat: 51.8288,
        lon: 9.4467,
    });

    let weather_url = format!(
        "{}?lat={}&lon={}&exclude=current,minutely,hourly,alerts&appid={}",
        trueos_weather::config::ONECALL_URL,
        geo.lat,
        geo.lon,
        WEATHER_API_KEY
    );

    let (raw_weather, source_note) =
        match trueos_weather::transport::fetch_text(weather_url.as_str(), FETCH_TIMEOUT_MS) {
            Ok(raw) => (raw, String::from("live OpenWeather forecast")),
            Err(err) => {
                bp_info!("weather_bp: weather fetch failed: {}; using bundled demo", err);
                note = if note.is_empty() {
                    format!("weather fetch failed: {}; using bundled demo", err)
                } else {
                    format!("weather fetch failed: {}; {}", err, note)
                };
                (String::from(bundled_demo_json()), String::from("bundled demo weather fallback"))
            }
        };

    let response = trueos_weather::oc3::decode_onecall_raw_safe(raw_weather.as_str()).ok();
    match response {
        Some(response) => {
            build_weather_snapshot(&geo, &response, source_note.as_str(), note.as_str())
        }
        None => WeatherSnapshot {
            header: format!("{} {} weather unavailable", geo.country, geo.name),
            subheader: source_note,
            rows: Vec::new(),
            note: if note.is_empty() {
                String::from("decode failed")
            } else {
                format!("decode failed; {}", note)
            },
        },
    }
}

fn bundled_demo_json() -> &'static str {
    DEMO_JSON
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('{'))
        .unwrap_or(DEMO_JSON)
}

fn parse_geo_response(raw: &str) -> Option<GeoResult> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let arr = root.as_array()?;
    let first = arr.first()?;
    Some(GeoResult {
        name: first.get("name")?.as_str()?.to_string(),
        country: first.get("country")?.as_str()?.to_string(),
        lat: first.get("lat")?.as_f64()?,
        lon: first.get("lon")?.as_f64()?,
    })
}

fn build_weather_snapshot(
    geo: &GeoResult,
    response: &trueos_weather::OpenWeatherResponse,
    source_note: &str,
    note: &str,
) -> WeatherSnapshot {
    let mut rows = Vec::new();
    if let Some(daily) = response.daily.as_ref() {
        for day in daily.iter().take(DAILY_ROW_COUNT) {
            let weather = day.weather.first();
            let condition = weather.map(|w| w.description.as_str()).unwrap_or("weather");
            let k2c = |k: f64| libm::round(k - 273.15) as i32;
            rows.push(WeatherRow {
                icon: weather_icon_for(weather),
                day: weekday_abbrev(day.dt),
                summary: String::from(condition),
                temp_line: format!(
                    "day {:>3}C feel {:>3}C  night {:>3}C  rain {:>3}%",
                    k2c(day.temp.day),
                    k2c(day.feels_like.day),
                    k2c(day.temp.night),
                    libm::round(day.pop * 100.0) as i32
                ),
            });
        }
    }

    WeatherSnapshot {
        header: format!("{} {}  {:.4} {:.4}", geo.country, geo.name, geo.lat, geo.lon),
        subheader: String::from(source_note),
        rows,
        note: String::from(note),
    }
}

fn weekday_abbrev(unix: u64) -> &'static str {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    DAYS[((unix / 86_400) % 7) as usize]
}

fn weather_icon_for(weather: Option<&trueos_weather::WetterInfo>) -> WeatherIcon {
    if let Some(info) = weather {
        match info.icon.as_str() {
            "01d" => return WeatherIcon::ClearDay,
            "01n" => return WeatherIcon::ClearNight,
            "02d" => return WeatherIcon::PartlyDay,
            "02n" => return WeatherIcon::PartlyNight,
            "03d" | "03n" | "04d" | "04n" => return WeatherIcon::Cloud,
            "09d" | "09n" => return WeatherIcon::Rain,
            "10d" => return WeatherIcon::RainDay,
            "10n" => return WeatherIcon::Rain,
            "11d" | "11n" => return WeatherIcon::Thunder,
            "13d" | "13n" => return WeatherIcon::Snow,
            "50d" | "50n" => return WeatherIcon::Fog,
            _ => {}
        }

        match info.id {
            200..=299 => WeatherIcon::Thunder,
            300..=599 => WeatherIcon::Rain,
            600..=699 => WeatherIcon::Snow,
            700..=799 => WeatherIcon::Fog,
            800 => WeatherIcon::ClearDay,
            801 => WeatherIcon::PartlyDay,
            802..=899 => WeatherIcon::Cloud,
            _ => WeatherIcon::Cloud,
        }
    } else {
        WeatherIcon::Cloud
    }
}

fn present_snapshot(window: &ui2::SurfaceWindow, snapshot: &WeatherSnapshot) {
    let pixels = compose_weather(snapshot, WINDOW_WIDTH as usize, WINDOW_HEIGHT as usize);
    if gfx::upload_texture_rgba_image_now(
        window.tex_id(),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        pixels.as_slice(),
    ) {
        let _ = window.id().request_repaint();
    }
}

fn compose_weather(snapshot: &WeatherSnapshot, width: usize, height: usize) -> Vec<u8> {
    let mut buf = vec![0u8; width * height * 4];
    let line_h = WEATHER_FONT_TIER.line_height_px().max(1) as usize;
    let row_gap = line_h / 2;
    fill_rect(&mut buf, width, height, 0, 0, width, height, BG_RGBA);
    fill_rect(&mut buf, width, height, 0, 0, width, 58, PANEL_RGBA);

    draw_text(
        &mut buf,
        width,
        height,
        PAD,
        PAD,
        WEATHER_FONT_TIER,
        snapshot.header.as_str(),
        ACCENT_RGBA,
    );
    draw_text(
        &mut buf,
        width,
        height,
        PAD,
        PAD + line_h,
        WEATHER_FONT_TIER,
        snapshot.subheader.as_str(),
        DIM_RGBA,
    );

    let mut y = 72usize;
    for (idx, row) in snapshot.rows.iter().enumerate() {
        let row_bg = if idx % 2 == 0 { ROW_RGBA } else { BG_RGBA };
        if idx % 2 == 0 {
            fill_rect(
                &mut buf,
                width,
                height,
                0,
                y.saturating_sub(6),
                width,
                line_h * 2 + row_gap,
                ROW_RGBA,
            );
        }
        draw_weather_icon(&mut buf, width, height, PAD, y.saturating_sub(1), row.icon, row_bg);
        draw_text(
            &mut buf,
            width,
            height,
            ICON_TEXT_X,
            y,
            WEATHER_FONT_TIER,
            format!("{}  {}", row.day, row.summary).as_str(),
            TEXT_RGBA,
        );
        draw_text(
            &mut buf,
            width,
            height,
            ICON_TEXT_X,
            y + line_h,
            WEATHER_FONT_TIER,
            row.temp_line.as_str(),
            DIM_RGBA,
        );
        y = y.saturating_add(line_h * 2 + row_gap);
        if y >= height.saturating_sub(line_h * 2) {
            break;
        }
    }

    if !snapshot.note.is_empty() {
        let note_rgba = if snapshot.note.contains("failed") {
            WARN_RGBA
        } else {
            DIM_RGBA
        };
        draw_text(
            &mut buf,
            width,
            height,
            PAD,
            height.saturating_sub(PAD + line_h),
            WEATHER_FONT_TIER,
            snapshot.note.as_str(),
            note_rgba,
        );
    }
    buf
}

fn fill_rect(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let ex = x.saturating_add(w).min(dst_w);
    let ey = y.saturating_add(h).min(dst_h);
    for row in y.min(dst_h)..ey {
        for col in x.min(dst_w)..ex {
            let i = (row * dst_w + col) * 4;
            dst[i] = rgba[0];
            dst[i + 1] = rgba[1];
            dst[i + 2] = rgba[2];
            dst[i + 3] = rgba[3];
        }
    }
}

fn put_px(dst: &mut [u8], dst_w: usize, dst_h: usize, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if x >= dst_w || y >= dst_h {
        return;
    }
    let i = (y * dst_w + x) * 4;
    dst[i] = rgba[0];
    dst[i + 1] = rgba[1];
    dst[i + 2] = rgba[2];
    dst[i + 3] = rgba[3];
}

fn draw_disc(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    cx: i32,
    cy: i32,
    radius: i32,
    rgba: [u8; 4],
) {
    let rr = radius * radius;
    for y in (cy - radius)..=(cy + radius) {
        for x in (cx - radius)..=(cx + radius) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= rr {
                put_px(dst, dst_w, dst_h, x, y, rgba);
            }
        }
    }
}

fn draw_sun(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize) {
    let x = x as i32;
    let y = y as i32;
    draw_disc(dst, dst_w, dst_h, x + 10, y + 9, 5, SUN_RGBA);
    for (rx, ry) in [
        (10, 1),
        (10, 17),
        (2, 9),
        (18, 9),
        (4, 3),
        (16, 3),
        (4, 15),
        (16, 15),
    ] {
        put_px(dst, dst_w, dst_h, x + rx, y + ry, SUN_RGBA);
    }
}

fn draw_moon(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize, bg_rgba: [u8; 4]) {
    let x = x as i32;
    let y = y as i32;
    draw_disc(dst, dst_w, dst_h, x + 10, y + 9, 6, MOON_RGBA);
    draw_disc(dst, dst_w, dst_h, x + 13, y + 7, 6, bg_rgba);
}

fn draw_cloud(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize) {
    let x = x as i32;
    let y = y as i32;
    draw_disc(dst, dst_w, dst_h, x + 7, y + 11, 4, CLOUD_RGBA);
    draw_disc(dst, dst_w, dst_h, x + 12, y + 9, 5, CLOUD_RGBA);
    draw_disc(dst, dst_w, dst_h, x + 16, y + 12, 3, CLOUD_RGBA);
    fill_rect(dst, dst_w, dst_h, x as usize + 4, y as usize + 11, 15, 5, CLOUD_RGBA);
}

fn draw_rain(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize, day: bool) {
    if day {
        draw_sun(dst, dst_w, dst_h, x.saturating_sub(1), y);
    }
    draw_cloud(dst, dst_w, dst_h, x, y + 1);
    for dx in [6usize, 11, 16] {
        fill_rect(dst, dst_w, dst_h, x + dx, y + 17, 2, 5, RAIN_RGBA);
    }
}

fn draw_thunder(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize) {
    draw_cloud(dst, dst_w, dst_h, x, y);
    fill_rect(dst, dst_w, dst_h, x + 10, y + 15, 4, 2, THUNDER_RGBA);
    fill_rect(dst, dst_w, dst_h, x + 9, y + 17, 3, 2, THUNDER_RGBA);
    fill_rect(dst, dst_w, dst_h, x + 8, y + 19, 2, 3, THUNDER_RGBA);
    fill_rect(dst, dst_w, dst_h, x + 12, y + 17, 2, 2, THUNDER_RGBA);
}

fn draw_snow(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize) {
    draw_cloud(dst, dst_w, dst_h, x, y);
    for (dx, dy) in [(7usize, 18usize), (12, 20), (17, 18)] {
        fill_rect(dst, dst_w, dst_h, x + dx, y + dy, 2, 2, SNOW_RGBA);
        put_px(dst, dst_w, dst_h, (x + dx + 2) as i32, (y + dy) as i32, SNOW_RGBA);
        put_px(dst, dst_w, dst_h, (x + dx) as i32, (y + dy + 2) as i32, SNOW_RGBA);
    }
}

fn draw_fog(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize) {
    draw_cloud(dst, dst_w, dst_h, x, y.saturating_sub(1));
    for dy in [16usize, 19, 22] {
        fill_rect(dst, dst_w, dst_h, x + 4, y + dy, 15, 2, FOG_RGBA);
    }
}

fn draw_weather_icon(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    icon: WeatherIcon,
    bg_rgba: [u8; 4],
) {
    match icon {
        WeatherIcon::ClearDay => draw_sun(dst, dst_w, dst_h, x, y),
        WeatherIcon::ClearNight => draw_moon(dst, dst_w, dst_h, x, y, bg_rgba),
        WeatherIcon::PartlyDay => {
            draw_sun(dst, dst_w, dst_h, x.saturating_sub(2), y.saturating_sub(1));
            draw_cloud(dst, dst_w, dst_h, x + 2, y + 3);
        }
        WeatherIcon::PartlyNight => {
            draw_moon(dst, dst_w, dst_h, x.saturating_sub(1), y, bg_rgba);
            draw_cloud(dst, dst_w, dst_h, x + 3, y + 4);
        }
        WeatherIcon::Cloud => draw_cloud(dst, dst_w, dst_h, x, y + 2),
        WeatherIcon::RainDay => draw_rain(dst, dst_w, dst_h, x, y, true),
        WeatherIcon::Rain => draw_rain(dst, dst_w, dst_h, x, y, false),
        WeatherIcon::Thunder => draw_thunder(dst, dst_w, dst_h, x, y + 1),
        WeatherIcon::Snow => draw_snow(dst, dst_w, dst_h, x, y + 1),
        WeatherIcon::Fog => draw_fog(dst, dst_w, dst_h, x, y),
    }
}

fn draw_text(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    tier: ui2::FontTier,
    text: &str,
    rgba: [u8; 4],
) {
    let max_width = dst_w.saturating_sub(PAD).saturating_sub(x);
    let _ = ui2::blit_text_rgba(
        dst,
        dst_w as u32,
        dst_h as u32,
        tier,
        x as u32,
        y as u32,
        max_width as u32,
        text,
        rgba,
    );
}
