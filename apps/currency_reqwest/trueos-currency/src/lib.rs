extern crate alloc;

use core::future::Future;

use trueos::logl::{self, level};
use trueos::ui2::{self, gfx};
use trueos::{
    platform::{String, ToString, Vec, vec},
    runtime, time,
};
use trueos::{platform, platform::format};

pub const FXFEED_URL: &str =
    "https://api.fxfeed.io/v2/latest?base=USD&currencies=EUR,GBP,JPY&api_key=fxf_SwF1T46MmH8uCkOO7tOc";

const CURRENCY_CODES: [&str; 3] = ["EUR", "GBP", "JPY"];
const IDLE_SLEEP_MS: u64 = 200;

const UI2_CURRENCY_VIEW_W: u32 = 360;
const UI2_CURRENCY_VIEW_H: u32 = 180;
const UI2_CURRENCY_WINDOW_ALPHA: u8 = 0xFF;

const UI2_CURRENCY_BG_RGBA: [u8; 4] = [0x18, 0x1B, 0x22, 0xFF];
const UI2_CURRENCY_HEADER_BG_RGBA: [u8; 4] = [0x21, 0x26, 0x31, 0xFF];
const UI2_CURRENCY_ROW_BG_RGBA: [u8; 4] = [0x1C, 0x21, 0x2A, 0xFF];
const UI2_CURRENCY_TEXT_RGBA: [u8; 4] = [0xEC, 0xF2, 0xF8, 0xFF];
const UI2_CURRENCY_DIM_RGBA: [u8; 4] = [0x94, 0xA2, 0xB3, 0xFF];
const UI2_CURRENCY_ACCENT_RGBA: [u8; 4] = [0x79, 0xCF, 0xB0, 0xFF];

const UI2_CURRENCY_FONT_TIER: ui2::FontTier = ui2::FontTier::OneX;
const UI2_CURRENCY_PAD_X: usize = 10;
const UI2_CURRENCY_PAD_Y: usize = 8;
const UI2_CURRENCY_ROW_GAP_Y: usize = 4;

#[derive(Copy, Clone)]
pub struct CurrencyAppConfig {
    pub transport_label: &'static str,
    pub window_title: &'static str,
    pub tex_id: u32,
    pub window_x: i32,
    pub window_y: i32,
    pub window_z: i32,
}

#[derive(Clone, Debug)]
struct CurrencyRow {
    pair: String,
    value: String,
}

#[derive(Clone, Debug)]
struct CurrencySnapshot {
    header: String,
    subheader: String,
    rows: Vec<CurrencyRow>,
    footer: String,
}

pub fn run_currency_app<F, Fut>(config: CurrencyAppConfig, fetcher: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    logl::log(level::INFO, format_args!("currency_bp: start transport={}", config.transport_label));
    logl::log(level::INFO, format_args!("currency_bp: stage runtime.current_thread_net.builder_new"));

    let mut runtime_builder = runtime::current_thread_net();
    logl::log(level::INFO, format_args!("currency_bp: success runtime.current_thread_net.builder_new"));

    logl::log(level::INFO, format_args!("currency_bp: stage runtime.current_thread_net.build"));

    let runtime = match runtime_builder.build() {
        Ok(rt) => {
            logl::log(level::INFO, format_args!("currency_bp: success runtime.current_thread_net.build"));
            rt
        }
        Err(err) => {
            logl::log(level::ERROR, format_args!("currency_bp: runtime build failed: {}", err));
            return;
        }
    };

    logl::log(level::INFO, format_args!("currency_bp: stage ui2.surface_window.create"));

    let Some(surface) = ui2::SurfaceWindow::create_with_options(
        config.window_title,
        ui2::Rect {
            x: config.window_x,
            y: config.window_y,
            width: UI2_CURRENCY_VIEW_W,
            height: UI2_CURRENCY_VIEW_H,
        },
        ui2::CreateOptions {
            z: config.window_z,
            alpha: UI2_CURRENCY_WINDOW_ALPHA,
        },
        config.tex_id,
        true,
    ) else {
        logl::log(level::ERROR, format_args!("currency_bp: ui2 surface window create failed"));
        return;
    };
    logl::log(level::INFO, format_args!("currency_bp: success ui2.surface_window.create"));
    let _ = surface
        .id()
        .set_decorations(ui2::WindowDecorationMode::System);
    let _ = surface.id().set_vertical_scrollbar_visible(false);
    let _ = surface.id().set_horizontal_scrollbar_visible(false);

    logl::log(level::INFO, format_args!("currency_bp: stage ui2.present.loading_snapshot"));
    present_snapshot(&surface, &build_loading_snapshot(config.transport_label));
    logl::log(level::INFO, format_args!("currency_bp: success ui2.present.loading_snapshot"));

    logl::log(level::INFO, format_args!("currency_bp: stage runtime.block_on.fetch_cycle"));

    runtime.block_on(async move {
        logl::log(level::INFO, format_args!("currency_bp: stage fetch.await"));
        let snapshot = match fetcher().await {
            Ok(raw) => {
                logl::log(level::INFO, format_args!("currency_bp: success fetch.await bytes={}", raw.len()));
                build_currency_snapshot(raw.as_str()).unwrap_or_else(|| {
                    logl::log(level::ERROR, format_args!("currency_bp: parse failed after fetch"));
                    build_error_snapshot(config.transport_label, "FXFEED PARSE FAILED")
                })
            }
            Err(err) => {
                logl::log(level::ERROR, format_args!("currency_bp: request failed: {}", err));
                build_error_snapshot(config.transport_label, "FXFEED REQUEST FAILED")
            }
        };
        logl::log(level::INFO, format_args!("currency_bp: stage ui2.present.result_snapshot"));
        present_snapshot(&surface, &snapshot);
        logl::log(level::INFO, format_args!("currency_bp: success ui2.present.result_snapshot"));

        logl::log(level::INFO, format_args!("currency_bp: stage idle.loop"));
        loop {
            platform::poll_once();
            time::sleep(time::Duration::from_millis(IDLE_SLEEP_MS)).await;
        }
    });
}

fn parse_rate(raw: &str, code: &str) -> Option<f64> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let success = root.get("success")?.as_bool()?;
    if !success {
        return None;
    }
    root.get("rates")?.get(code)?.as_f64()
}

fn parse_string_field(raw: &str, key: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    root.get(key)?.as_str().map(ToString::to_string)
}

fn build_loading_snapshot(transport_label: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: currency_subheader(),
        rows: Vec::new(),
        footer: format!("Loading FXFeed via {}...", transport_label),
    }
}

fn build_error_snapshot(transport_label: &str, message: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: currency_subheader(),
        rows: Vec::new(),
        footer: format!("{} ({})", message, transport_label),
    }
}

fn currency_subheader() -> String {
    format!("USD base  |  {}", CURRENCY_CODES.join(" "))
}

fn build_currency_snapshot(raw: &str) -> Option<CurrencySnapshot> {
    let base = parse_string_field(raw, "base")?;
    let date = parse_string_field(raw, "date")?;
    let mut rows = Vec::new();
    for code in CURRENCY_CODES {
        let rate = parse_rate(raw, code)?;
        let value = if code == "JPY" {
            format!("{:.4}", rate)
        } else {
            format!("{:.6}", rate)
        };
        rows.push(CurrencyRow {
            pair: code.to_string(),
            value,
        });
    }

    Some(CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: format!("1 {} =", base),
        rows,
        footer: format!("Updated {}", date),
    })
}

fn currency_line_height() -> usize {
    UI2_CURRENCY_FONT_TIER.line_height_px().max(1) as usize
}

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn render_text_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let max_width_px = dst_width.saturating_sub(x) as u32;
    let _ = ui2::blit_text_rgba(
        dst,
        dst_width as u32,
        dst_height as u32,
        UI2_CURRENCY_FONT_TIER,
        x as u32,
        y as u32,
        max_width_px,
        text,
        rgba,
    );
}

fn compose_currency_rgba(snapshot: &CurrencySnapshot) -> Vec<u8> {
    let dst_width = UI2_CURRENCY_VIEW_W as usize;
    let dst_height = UI2_CURRENCY_VIEW_H as usize;
    let mut rgba = vec![0u8; dst_width.saturating_mul(dst_height).saturating_mul(4)];

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        dst_height,
        UI2_CURRENCY_BG_RGBA,
    );

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        currency_line_step()
            .saturating_mul(2)
            .saturating_add(UI2_CURRENCY_PAD_Y),
        UI2_CURRENCY_HEADER_BG_RGBA,
    );

    let line_step = currency_line_step();
    let mut y = UI2_CURRENCY_PAD_Y;
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.header.as_str(),
        UI2_CURRENCY_ACCENT_RGBA,
    );
    y = y.saturating_add(line_step);
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.subheader.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );
    y = y.saturating_add(line_step).saturating_add(2);

    if snapshot.rows.is_empty() {
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X,
            y,
            snapshot.footer.as_str(),
            UI2_CURRENCY_TEXT_RGBA,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X,
            dst_height.saturating_sub(UI2_CURRENCY_PAD_Y + currency_line_height()),
            snapshot.footer.as_str(),
            UI2_CURRENCY_DIM_RGBA,
        );
        return rgba;
    }

    for row in snapshot.rows.iter() {
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X.saturating_sub(4),
            y.saturating_sub(2),
            dst_width.saturating_sub(UI2_CURRENCY_PAD_X.saturating_sub(4) * 2),
            line_step.saturating_add(4),
            UI2_CURRENCY_ROW_BG_RGBA,
        );
        let line = format!("{}  {}", row.pair, row.value);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X,
            y,
            line.as_str(),
            UI2_CURRENCY_TEXT_RGBA,
        );
        y = y.saturating_add(line_step);
    }

    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        UI2_CURRENCY_PAD_X,
        dst_height.saturating_sub(UI2_CURRENCY_PAD_Y + currency_line_height()),
        snapshot.footer.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );

    rgba
}

fn currency_line_step() -> usize {
    currency_line_height().saturating_add(UI2_CURRENCY_ROW_GAP_Y)
}

fn present_snapshot(surface: &ui2::SurfaceWindow, snapshot: &CurrencySnapshot) {
    let rgba = compose_currency_rgba(snapshot);
    if gfx::upload_texture_rgba_image_now(
        surface.tex_id(),
        UI2_CURRENCY_VIEW_W,
        UI2_CURRENCY_VIEW_H,
        rgba.as_slice(),
    ) {
        let _ = surface.id().request_repaint();
    }
}