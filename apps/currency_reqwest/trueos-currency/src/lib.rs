extern crate alloc;

use core::future::Future;

use trueos::logl::{self, level};
use trueos::ui2::{self, gfx};
use trueos::{platform, platform::format};
use trueos::{
    platform::{String, ToString, Vec, vec},
    runtime, time,
};

pub const FXFEED_URL: &str = "https://api.fxfeed.io/v2/latest?base=USD&currencies=EUR,GBP,JPY&api_key=fxf_SwF1T46MmH8uCkOO7tOc";

const CURRENCY_CODES: [&str; 3] = ["EUR", "GBP", "JPY"];
const IDLE_SLEEP_MS: u64 = 200;

const UI2_CURRENCY_VIEW_W: u32 = 460;
const UI2_CURRENCY_VIEW_H: u32 = 240;
const UI2_CURRENCY_WINDOW_ALPHA: u8 = 0xFF;

const UI2_CURRENCY_BG_RGBA: [u8; 4] = [0x12, 0x16, 0x1D, 0xFF];
const UI2_CURRENCY_HEADER_BG_RGBA: [u8; 4] = [0x20, 0x27, 0x32, 0xFF];
const UI2_CURRENCY_ROW_BG_RGBA: [u8; 4] = [0x1A, 0x21, 0x29, 0xFF];
const UI2_CURRENCY_ROW_ALT_RGBA: [u8; 4] = [0x1D, 0x25, 0x30, 0xFF];
const UI2_CURRENCY_TEXT_RGBA: [u8; 4] = [0xF0, 0xF5, 0xFA, 0xFF];
const UI2_CURRENCY_DIM_RGBA: [u8; 4] = [0x99, 0xA7, 0xB8, 0xFF];
const UI2_CURRENCY_ACCENT_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const UI2_CURRENCY_VALUE_RGBA: [u8; 4] = [0xFF, 0xD1, 0x7A, 0xFF];

const UI2_CURRENCY_FONT_TIER: ui2::FontTier = ui2::FontTier::OneX;
const UI2_CURRENCY_PAD_X: usize = 14;
const UI2_CURRENCY_PAD_Y: usize = 10;
const UI2_CURRENCY_ROW_GAP_Y: usize = 6;
const UI2_CURRENCY_HEADER_H: usize = 58;
const UI2_CURRENCY_ROW_H: usize = 42;

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
    code: String,
    per_usd: String,
    per_100_usd: String,
    usd_per_unit: String,
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
    logl::log(
        level::INFO,
        format_args!("currency_bp: stage runtime.current_thread_net.builder_new"),
    );

    let mut runtime_builder = runtime::current_thread_net();
    logl::log(
        level::INFO,
        format_args!("currency_bp: success runtime.current_thread_net.builder_new"),
    );

    logl::log(level::INFO, format_args!("currency_bp: stage runtime.current_thread_net.build"));

    let runtime = match runtime_builder.build() {
        Ok(rt) => {
            logl::log(
                level::INFO,
                format_args!("currency_bp: success runtime.current_thread_net.build"),
            );
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
                logl::log(
                    level::INFO,
                    format_args!("currency_bp: success fetch.await bytes={}", raw.len()),
                );
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

fn parse_feed(raw: &str) -> Option<(String, String, serde_json::Value)> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let success = root.get("success")?.as_bool()?;
    if !success {
        return None;
    }
    let base = root.get("base")?.as_str()?.to_string();
    let date = root.get("date")?.as_str()?.to_string();
    Some((base, date, root))
}

fn build_loading_snapshot(transport_label: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "USD Exchange Rates".to_string(),
        subheader: currency_subheader(),
        rows: Vec::new(),
        footer: format!("Loading live market snapshot via {}...", transport_label),
    }
}

fn build_error_snapshot(transport_label: &str, message: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "USD Exchange Rates".to_string(),
        subheader: currency_subheader(),
        rows: Vec::new(),
        footer: format!("{} ({})", message, transport_label),
    }
}

fn currency_subheader() -> String {
    format!("Tracked pairs: USD to {}", CURRENCY_CODES.join(", "))
}

fn build_currency_snapshot(raw: &str) -> Option<CurrencySnapshot> {
    let (base, date, root) = parse_feed(raw)?;
    let rates = root.get("rates")?;
    let mut rows = Vec::new();
    for code in CURRENCY_CODES {
        let rate = rates.get(code)?.as_f64()?;
        let per_usd = format_rate(rate, code);
        let per_100_usd = format_rate(rate * 100.0, code);
        let usd_per_unit = if rate > 0.0 {
            format!("${:.4}", 1.0 / rate)
        } else {
            String::from("-")
        };
        rows.push(CurrencyRow {
            code: code.to_string(),
            per_usd,
            per_100_usd,
            usd_per_unit,
        });
    }

    Some(CurrencySnapshot {
        header: format!("{} Exchange Rates", base),
        subheader: String::from("Live FXFeed market snapshot"),
        rows,
        footer: format!("Updated {}  |  1 unit inverse shown in USD", date),
    })
}

fn format_rate(rate: f64, code: &str) -> String {
    if code == "JPY" {
        format!("{:.2}", rate)
    } else {
        format!("{:.4}", rate)
    }
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
        UI2_CURRENCY_HEADER_H,
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
    y = UI2_CURRENCY_HEADER_H.saturating_add(UI2_CURRENCY_ROW_GAP_Y);

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

    for (idx, row) in snapshot.rows.iter().enumerate() {
        let row_bg = if idx % 2 == 0 {
            UI2_CURRENCY_ROW_BG_RGBA
        } else {
            UI2_CURRENCY_ROW_ALT_RGBA
        };
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X.saturating_sub(4),
            y.saturating_sub(2),
            dst_width.saturating_sub(UI2_CURRENCY_PAD_X.saturating_sub(4) * 2),
            UI2_CURRENCY_ROW_H,
            row_bg,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X,
            y,
            row.code.as_str(),
            UI2_CURRENCY_ACCENT_RGBA,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X + 58,
            y,
            format!("1 USD = {} {}", row.per_usd, row.code).as_str(),
            UI2_CURRENCY_TEXT_RGBA,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X + 250,
            y,
            format!("100 USD = {}", row.per_100_usd).as_str(),
            UI2_CURRENCY_VALUE_RGBA,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X + 58,
            y.saturating_add(line_step),
            format!("1 {} = {}", row.code, row.usd_per_unit).as_str(),
            UI2_CURRENCY_DIM_RGBA,
        );
        y = y
            .saturating_add(UI2_CURRENCY_ROW_H)
            .saturating_add(UI2_CURRENCY_ROW_GAP_Y);
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
