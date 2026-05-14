#![no_std]
#![no_main]

extern crate alloc;

use alloc::{format, string::String};
use trueos::logl::{self, level};
use trueos::platform;
use trueos::ui2::{self, gfx};

const WINDOW_TITLE: &str = "SVG Grid BP";
const WINDOW_X: i32 = 960;
const WINDOW_Y: i32 = 120;
const WINDOW_WIDTH: u32 = 272;
const WINDOW_HEIGHT: u32 = 204;
const TEX_ID: u32 = 4_761;

fn open_window() -> Option<ui2::SurfaceWindow> {
    ui2::SurfaceWindow::create(
        WINDOW_TITLE,
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TEX_ID,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = open_window() else {
        logl::log(level::ERROR, "svg_grid bp: window create failed\n");
        return;
    };

    let window_id = window.id();
    let _ = window_id.set_vertical_scrollbar_visible(false);
    let _ = window_id.set_horizontal_scrollbar_visible(false);
    let _ = window_id.set_resize_mode(ui2::WindowResizeMode::PreviewCommit);
    let _ = window_id.set_resize_maintain_aspect(true);
    let _ = window_id.set_content_preserve_scale(true);

    let mut rendered_size =
        current_content_size(window_id).unwrap_or((WINDOW_WIDTH, WINDOW_HEIGHT));
    if !render_svg(window_id, rendered_size.0, rendered_size.1) {
        return;
    }

    loop {
        platform::poll_once();
        let Some(size) = current_content_size(window_id) else {
            continue;
        };
        if size != rendered_size && render_svg(window_id, size.0, size.1) {
            rendered_size = size;
        }
    }
}

fn current_content_size(window_id: ui2::WindowId) -> Option<(u32, u32)> {
    let info = window_id.info()?;
    Some((info.content.width.max(1), info.content.height.max(1)))
}

fn render_svg(window_id: ui2::WindowId, width: u32, height: u32) -> bool {
    let svg = svg_for_texture_size(width, height);
    let rc = gfx::upload_svg_to_texture(TEX_ID, svg.as_bytes());
    if rc != 0 {
        logl::log(
            level::ERROR,
            format_args!("svg_grid bp: svg upload failed rc={} size={}x{}\n", rc, width, height),
        );
        return false;
    }
    let _ = window_id.request_repaint();
    logl::log(
        level::INFO,
        format_args!("svg_grid bp: rendered svg texture {}x{}\n", width, height),
    );
    true
}

fn svg_for_texture_size(width: u32, height: u32) -> String {
    let mut svg = String::from(SVG_GRID);
    let head = format!(
        r#"<svg width="{}" height="{}" viewBox="0 0 272 204" xmlns="http://www.w3.org/2000/svg">"#,
        width.max(1),
        height.max(1)
    );
    if let Some(tag_end) = svg.find('>') {
        svg.replace_range(..=tag_end, &head);
    }
    svg
}

const SVG_GRID: &str = r##"<svg width="272" height="204" viewBox="0 0 272 204" xmlns="http://www.w3.org/2000/svg">
  <rect width="272" height="204" fill="#0A0E14"/>
  <g fill="#141922" stroke="#252C38" stroke-width="1">
    <rect x="4" y="4" width="64" height="64"/>
    <rect x="72" y="4" width="64" height="64"/>
    <rect x="140" y="4" width="64" height="64"/>
    <rect x="208" y="4" width="64" height="64"/>
    <rect x="4" y="72" width="64" height="64"/>
    <rect x="72" y="72" width="64" height="64"/>
    <rect x="140" y="72" width="64" height="64"/>
    <rect x="208" y="72" width="64" height="64"/>
    <rect x="4" y="140" width="64" height="64"/>
    <rect x="72" y="140" width="64" height="64"/>
    <rect x="140" y="140" width="64" height="64"/>
    <rect x="208" y="140" width="64" height="64"/>
  </g>
</svg>"##;
