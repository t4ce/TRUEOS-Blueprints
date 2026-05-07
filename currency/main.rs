// trueos-blueprint: features=["tokio-runtime"]

use std::format;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use trueos::{net_fetch, ui2, vgfx_hosted, vsys};
use trueos_blueprint::{bp_error, bp_info, runtime, time};

const UI2_CURRENCY_TEX_ID: u32 = 4_723;
const UI2_CURRENCY_WINDOW_TITLE: &str = "Currency";
const UI2_CURRENCY_VIEW_W: u32 = 360;
const UI2_CURRENCY_VIEW_H: u32 = 180;
const UI2_CURRENCY_WINDOW_X: i32 = 210;
const UI2_CURRENCY_WINDOW_Y: i32 = 90;
const UI2_CURRENCY_WINDOW_Z: i32 = 39;
const UI2_CURRENCY_WINDOW_ALPHA: u8 = 0xFF;

const UI2_CURRENCY_BG_RGBA: [u8; 4] = [0x18, 0x1B, 0x22, 0xFF];
const UI2_CURRENCY_HEADER_BG_RGBA: [u8; 4] = [0x21, 0x26, 0x31, 0xFF];
const UI2_CURRENCY_ROW_BG_RGBA: [u8; 4] = [0x1C, 0x21, 0x2A, 0xFF];
const UI2_CURRENCY_TEXT_RGBA: [u8; 4] = [0xEC, 0xF2, 0xF8, 0xFF];
const UI2_CURRENCY_DIM_RGBA: [u8; 4] = [0x94, 0xA2, 0xB3, 0xFF];
const UI2_CURRENCY_ACCENT_RGBA: [u8; 4] = [0x79, 0xCF, 0xB0, 0xFF];

const UI2_CURRENCY_PAD_X: usize = 10;
const UI2_CURRENCY_PAD_Y: usize = 8;
const UI2_CURRENCY_ROW_GAP_Y: usize = 4;
const FONT_W: usize = 5;
const FONT_H: usize = 7;
const FONT_SCALE: usize = 2;
const GLYPH_ADVANCE: usize = (FONT_W + 1) * FONT_SCALE;
const LINE_H: usize = (FONT_H * FONT_SCALE) + UI2_CURRENCY_ROW_GAP_Y;

const FXFEED_URL: &str = "https://api.fxfeed.io/v2/latest?base=USD&currencies=EUR,GBP,JPY&api_key=fxf_SwF1T46MmH8uCkOO7tOc";
const FETCH_TIMEOUT_MS: u64 = 45_000;
const IDLE_SLEEP_MS: u64 = 200;

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

fn main() {
    bp_info!("currency_bp: start transport=host-fetch-cabi hyper");
    let runtime = match runtime::current_thread().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("currency_bp: runtime build failed: {}", err);
            return;
        }
    };

    let Some(surface) = ui2::SurfaceWindow::create_with_options(
        UI2_CURRENCY_WINDOW_TITLE,
        ui2::Rect {
            x: UI2_CURRENCY_WINDOW_X,
            y: UI2_CURRENCY_WINDOW_Y,
            width: UI2_CURRENCY_VIEW_W,
            height: UI2_CURRENCY_VIEW_H,
        },
        ui2::CreateOptions {
            z: UI2_CURRENCY_WINDOW_Z,
            alpha: UI2_CURRENCY_WINDOW_ALPHA,
        },
        UI2_CURRENCY_TEX_ID,
        true,
    ) else {
        bp_error!("currency_bp: ui2 surface window create failed");
        return;
    };
    let _ = surface
        .id()
        .set_vertical_scrollbar_side(ui2::VerticalScrollbarSide::Right);

    present_snapshot(&surface, &build_loading_snapshot());

    runtime.block_on(async {
        let snapshot = match fetch_text(FXFEED_URL).await {
            Ok(raw) => build_currency_snapshot(raw.as_str())
                .unwrap_or_else(|| build_error_snapshot("FXFEED PARSE FAILED")),
            Err(err) => {
                bp_error!("currency_bp: request failed: {}", err);
                build_error_snapshot("FXFEED REQUEST FAILED")
            }
        };
        present_snapshot(&surface, &snapshot);

        loop {
            vsys::poll_once();
            time::sleep(time::Duration::from_millis(IDLE_SLEEP_MS)).await;
        }
    });
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let op_id = net_fetch::fetch_bytes_start(url).map_err(|err| format!("{:?}", err))?;
    let deadline = time::Instant::now() + time::Duration::from_millis(FETCH_TIMEOUT_MS);

    loop {
        match net_fetch::fetch_bytes_result_len(op_id) {
            Ok(Some(len)) => {
                let bytes =
                    net_fetch::fetch_bytes_read(op_id, len).map_err(|err| format!("{:?}", err))?;
                return String::from_utf8(bytes).map_err(|_| String::from("bad utf8"));
            }
            Ok(None) if time::Instant::now() < deadline => {
                vsys::poll_once();
                time::sleep(time::Duration::from_millis(50)).await;
            }
            Ok(None) => {
                net_fetch::fetch_bytes_discard(op_id);
                return Err(String::from("timeout"));
            }
            Err(net_fetch::FetchBytesError::Code(code)) => {
                net_fetch::fetch_bytes_discard(op_id);
                return Err(format!("{} ({})", net_fetch::code_name(code), code));
            }
            Err(err) => {
                net_fetch::fetch_bytes_discard(op_id);
                return Err(format!("{:?}", err));
            }
        }
    }
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

fn build_loading_snapshot() -> CurrencySnapshot {
    CurrencySnapshot {
        header: "CURRENCY CONVERTER".to_string(),
        subheader: "USD BASE  |  EUR GBP JPY".to_string(),
        rows: Vec::new(),
        footer: "LOADING FXFEED...".to_string(),
    }
}

fn build_error_snapshot(message: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "CURRENCY CONVERTER".to_string(),
        subheader: "USD BASE  |  EUR GBP JPY".to_string(),
        rows: Vec::new(),
        footer: message.to_string(),
    }
}

fn build_currency_snapshot(raw: &str) -> Option<CurrencySnapshot> {
    let base = parse_string_field(raw, "base")?;
    let date = parse_string_field(raw, "date")?;
    let eur = parse_rate(raw, "EUR")?;
    let gbp = parse_rate(raw, "GBP")?;
    let jpy = parse_rate(raw, "JPY")?;

    Some(CurrencySnapshot {
        header: "CURRENCY CONVERTER".to_string(),
        subheader: format!("1 {} =", base),
        rows: vec![
            CurrencyRow {
                pair: "EUR".to_string(),
                value: format!("{:.6}", eur),
            },
            CurrencyRow {
                pair: "GBP".to_string(),
                value: format!("{:.6}", gbp),
            },
            CurrencyRow {
                pair: "JPY".to_string(),
                value: format!("{:.4}", jpy),
            },
        ],
        footer: format!("UPDATED {}", date),
    })
}

fn currency_content_size(snapshot: &CurrencySnapshot) -> (u32, u32) {
    let mut max_width = currency_measure_width(snapshot.header.as_str());
    max_width = max_width.max(currency_measure_width(snapshot.subheader.as_str()));
    max_width = max_width.max(currency_measure_width(snapshot.footer.as_str()));
    for row in snapshot.rows.iter() {
        let line = format!("{}  {}", row.pair, row.value);
        max_width = max_width.max(currency_measure_width(line.as_str()));
    }

    let total_lines = 2 + snapshot.rows.len().max(1) + 1;
    let content_w = max_width
        .saturating_add(UI2_CURRENCY_PAD_X * 2)
        .max(UI2_CURRENCY_VIEW_W as usize);
    let content_h = total_lines
        .saturating_mul(LINE_H)
        .saturating_add(UI2_CURRENCY_PAD_Y * 2)
        .max(UI2_CURRENCY_VIEW_H as usize);
    (content_w as u32, content_h as u32)
}

fn currency_measure_width(text: &str) -> usize {
    text.chars().count().saturating_mul(GLYPH_ADVANCE).max(1)
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
    let mut pen_x = x;
    for ch in text.chars() {
        let bits = glyph_bits(ch.to_ascii_uppercase());
        for (row, mask) in bits.iter().enumerate() {
            for col in 0..FONT_W {
                if (mask & (1 << (FONT_W - 1 - col))) != 0 {
                    fill_rect_rgba(
                        dst,
                        dst_width,
                        dst_height,
                        pen_x + col * FONT_SCALE,
                        y + row * FONT_SCALE,
                        FONT_SCALE,
                        FONT_SCALE,
                        rgba,
                    );
                }
            }
        }
        pen_x = pen_x.saturating_add(GLYPH_ADVANCE);
        if pen_x >= dst_width {
            break;
        }
    }
}

fn compose_currency_rgba(snapshot: &CurrencySnapshot, content_w: u32, content_h: u32) -> Vec<u8> {
    let dst_width = content_w as usize;
    let dst_height = content_h as usize;
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
        LINE_H.saturating_mul(2).saturating_add(UI2_CURRENCY_PAD_Y),
        UI2_CURRENCY_HEADER_BG_RGBA,
    );

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
    y = y.saturating_add(LINE_H);
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.subheader.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );
    y = y.saturating_add(LINE_H).saturating_add(2);

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
            LINE_H.saturating_add(4),
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
        y = y.saturating_add(LINE_H);
    }

    y = y.saturating_add(4);
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.footer.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );

    rgba
}

fn present_snapshot(surface: &ui2::SurfaceWindow, snapshot: &CurrencySnapshot) {
    let (content_w, content_h) = currency_content_size(snapshot);
    let rgba = compose_currency_rgba(snapshot, content_w, content_h);
    if vgfx_hosted::upload_texture_rgba_image_now(
        surface.tex_id(),
        content_w,
        content_h,
        rgba.as_slice(),
    ) {
        let _ = surface.id().request_repaint();
    }
}

fn glyph_bits(ch: char) -> [u8; FONT_H] {
    match ch {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        ':' => [
            0b00000, 0b00110, 0b00110, 0b00000, 0b00110, 0b00110, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        ' ' => [0; FONT_H],
        _ => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}
