#![no_std]
#![no_main]

use core::panic::PanicInfo;

use trueos::{bp_error, bp_info, panic_abort, ui2, vclock, vgfx, vgfx_hosted, vsys, TrueosAllocator};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const MAIN_WINDOW_TITLE: &str = "API Demo";
const MAIN_WINDOW_X: i32 = 120;
const MAIN_WINDOW_Y: i32 = 120;
const MAIN_WINDOW_WIDTH: u32 = 360;
const MAIN_WINDOW_HEIGHT: u32 = 220;
const MAIN_TEX_ID: u32 = 48_100;
const AUX_TEX_ID: u32 = 48_101;
const ICON_TEX_ID: u32 = 48_102;
const HOSTED_TEX_ID: u32 = 48_103;
const ASYNC_TEX_ID: u32 = 48_104;
const CLEAR_RGB: u32 = 0x16202A;

const TRIANGLE: [vgfx::RgbVertex; 3] = [
    vgfx::RgbVertex::new(0.0, -0.72, [0xFF, 0x83, 0x4D, 0xFF]),
    vgfx::RgbVertex::new(-0.76, 0.58, [0x46, 0xD6, 0x9D, 0xFF]),
    vgfx::RgbVertex::new(0.76, 0.58, [0x61, 0x92, 0xFF, 0xFF]),
];

const ICON_PIXELS: [u8; 16] = [
    0xF8, 0x63, 0x63, 0xFF, 0xFF, 0xD1, 0x66, 0xFF, 0x4F, 0xD8, 0x9D, 0xFF, 0x66, 0xA3, 0xFF,
    0xFF,
];

const ASYNC_PIXELS: [u8; 16] = [
    0x18, 0x24, 0x34, 0xFF, 0x26, 0x3A, 0x54, 0xFF, 0x42, 0x6B, 0x86, 0xFF, 0x5A, 0x93, 0xB8,
    0xFF,
];

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("apidemo bp: panic\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    bp_info!("apidemo: start");
    demo_vsys();
    demo_vclock();
    demo_owned_window();
    demo_surface_window();
    bp_info!("apidemo: done");
}

fn demo_vsys() {
    bp_info!("apidemo/vsys: begin");
    vsys::write_stream(1, b"apidemo/vsys: write_stream\n");
    vsys::write_log_stream(1, "apidemo/vsys: write_log_stream\n");
    vsys::log_info("apidemo/vsys: log_info\n");
    vsys::log_error("apidemo/vsys: log_error\n");
    vsys::log_infof(format_args!("apidemo/vsys: log_infof answer={}", 42));
    vsys::log_errorf(format_args!("apidemo/vsys: log_errorf sample={}", -7));
    vsys::log_info_with_args("apidemo/vsys: log_info_with_args", &["alpha", "beta"]);
    vsys::log_error_with_args("apidemo/vsys: log_error_with_args", &[]);
    vsys::poll_once();
}

fn demo_vclock() {
    bp_info!("apidemo/vclock: begin");
    match vclock::ntp_current_unix_seconds() {
        Some(unix) => bp_info!("apidemo/vclock: unix={}", unix),
        None => bp_error!("apidemo/vclock: unix unavailable"),
    }
    match vclock::ntp_kernel_date_day_month_year() {
        Some(date) => bp_info!("apidemo/vclock: date={}", date),
        None => bp_error!("apidemo/vclock: date unavailable"),
    }
    vsys::poll_once();
}

fn demo_owned_window() {
    bp_info!("apidemo/ui2: owned window");
    let _ = vgfx_hosted::ensure_texture_rgba_now(ICON_TEX_ID, 2, 2, [0x3A, 0xAF, 0x7C, 0xFF]);
    let _ = vgfx_hosted::upload_texture_rgba_image_now(ICON_TEX_ID, 2, 2, &ICON_PIXELS);

    let rect = ui2::Rect {
        x: MAIN_WINDOW_X + 30,
        y: MAIN_WINDOW_Y + 34,
        width: 240,
        height: 120,
    };
    let Some(window) = ui2::OwnedWindow::create("API Demo Control", rect) else {
        bp_error!("apidemo/ui2: OwnedWindow::create failed");
        return;
    };

    match window.info() {
        Some(info) => log_window_info("owned.create", info),
        None => bp_error!("apidemo/ui2: owned info unavailable"),
    }

    let id = window.id();
    log_bool("owned.set_title", id.set_title("API Demo Control / active"));
    log_bool("owned.set_icon", id.set_icon(ICON_TEX_ID));
    log_bool("owned.set_position", id.set_position(MAIN_WINDOW_X + 40, MAIN_WINDOW_Y + 44));
    log_bool("owned.set_size", id.set_size(260, 136));
    log_bool("owned.set_decorations", id.set_decorations(ui2::WindowDecorationMode::Client));
    log_bool("owned.set_hit_test_visible", id.set_hit_test_visible(true));
    log_bool(
        "owned.set_vertical_scrollbar_side",
        id.set_vertical_scrollbar_side(ui2::VerticalScrollbarSide::Left),
    );
    log_bool(
        "owned.set_horizontal_scrollbar_side",
        id.set_horizontal_scrollbar_side(ui2::HorizontalScrollbarSide::Top),
    );
    log_bool("owned.focus", id.focus());
    log_bool("owned.request_repaint", id.request_repaint());
    log_bool("owned.minimize", id.minimize());
    vsys::poll_once();
    log_bool("owned.restore", id.restore());
    vsys::poll_once();
    log_bool("owned.maximize", id.maximize());
    vsys::poll_once();
    log_bool("owned.restore.again", id.restore());
    log_bool("owned.begin_move", id.begin_move());
    log_bool(
        "owned.begin_resize",
        id.begin_resize(ui2::RESIZE_RIGHT | ui2::RESIZE_BOTTOM),
    );
    match id.info() {
        Some(info) => log_window_info("owned.after", info),
        None => bp_error!("apidemo/ui2: owned post-info unavailable"),
    }

    let leaked = ui2::OwnedWindow::create_with_options(
        "API Demo Temp",
        ui2::Rect {
            x: MAIN_WINDOW_X + 310,
            y: MAIN_WINDOW_Y + 24,
            width: 120,
            height: 72,
        },
        ui2::CreateOptions { z: 1, alpha: 224 },
    )
    .map(|temp| temp.leak());
    match leaked {
        Some(raw) => {
            bp_info!("apidemo/ui2: leaked temp raw={}", raw.raw());
            let wrapped = ui2::OwnedWindow::from_existing(raw);
            match wrapped.info() {
                Some(info) => log_window_info("owned.leaked", info),
                None => bp_error!("apidemo/ui2: leaked info unavailable"),
            }
            log_bool("owned.leaked.close", wrapped.id().close());
        }
        None => bp_error!("apidemo/ui2: OwnedWindow::create_with_options failed"),
    }

    vsys::poll_once();
}

fn demo_surface_window() {
    bp_info!("apidemo/vgfx+ui2: surface window");
    let Some(surface) = ui2::SurfaceWindow::create(
        MAIN_WINDOW_TITLE,
        ui2::Rect {
            x: MAIN_WINDOW_X,
            y: MAIN_WINDOW_Y,
            width: MAIN_WINDOW_WIDTH,
            height: MAIN_WINDOW_HEIGHT,
        },
        MAIN_TEX_ID,
    ) else {
        bp_error!("apidemo/ui2: SurfaceWindow::create failed");
        return;
    };

    let (width, height) = surface.size();
    bp_info!(
        "apidemo/ui2: surface tex={} size={}x{}",
        surface.tex_id(),
        width,
        height
    );
    match surface.id().info() {
        Some(info) => log_window_info("surface.create", info),
        None => bp_error!("apidemo/ui2: surface info unavailable"),
    }

    log_bool("hosted.ensure_texture", vgfx_hosted::ensure_texture_rgba_now(HOSTED_TEX_ID, 2, 2, [0x22, 0x2E, 0x3C, 0xFF]));
    log_bool(
        "hosted.upload_texture_now",
        vgfx_hosted::upload_texture_rgba_image_now(HOSTED_TEX_ID, 2, 2, &ICON_PIXELS),
    );
    log_texture_dimensions("hosted.texture_dimensions", HOSTED_TEX_ID);

    log_bool(
        "vgfx.upload_texture_async",
        vgfx::upload_texture_rgba_image_async(ASYNC_TEX_ID, 2, 2, &ASYNC_PIXELS),
    );
    log_texture_dimensions("async.texture_dimensions", ASYNC_TEX_ID);
    log_texture_dimensions("surface.texture_dimensions.before", surface.tex_id());
    log_bool(
        "vgfx.render_rgb_triangles_to_texture",
        vgfx::render_rgb_triangles_to_texture(
            surface.tex_id(),
            width,
            height,
            CLEAR_RGB,
            surface.id().raw(),
            &TRIANGLE,
        ),
    );
    log_texture_dimensions("surface.texture_dimensions.after", surface.tex_id());
    log_bool("surface.render_rgb_triangles", surface.render_rgb_triangles(0x1C2833, &TRIANGLE));
    log_bool("surface.set_title", surface.id().set_title("API Demo / rendered"));
    log_bool("surface.set_icon", surface.id().set_icon(HOSTED_TEX_ID));
    log_bool("surface.request_repaint", surface.id().request_repaint());

    match vgfx::capture_screenshot_data_url() {
        Some(data_url) => bp_info!("apidemo/vgfx: screenshot bytes={}", data_url.len()),
        None => bp_error!("apidemo/vgfx: screenshot unavailable"),
    }

    match ui2::SurfaceWindow::create_with_options(
        "API Demo Aux Surface",
        ui2::Rect {
            x: MAIN_WINDOW_X + 380,
            y: MAIN_WINDOW_Y + 40,
            width: 96,
            height: 96,
        },
        ui2::CreateOptions { z: 2, alpha: 232 },
        AUX_TEX_ID,
        true,
    ) {
        Some(aux) => {
            let _ = aux.render_rgb_triangles(0x202A36, &TRIANGLE);
            let leaked = aux.leak();
            bp_info!("apidemo/ui2: leaked aux surface raw={}", leaked.raw());
            log_bool("surface.leaked.close", leaked.close());
        }
        None => bp_error!("apidemo/ui2: SurfaceWindow::create_with_options failed"),
    }

    for tick in 0..4 {
        bp_info!("apidemo/poll: tick={}", tick);
        vsys::poll_once();
    }
}

fn log_bool(label: &str, ok: bool) {
    if ok {
        bp_info!("apidemo: {} ok", label);
    } else {
        bp_error!("apidemo: {} failed", label);
    }
}

fn log_texture_dimensions(label: &str, tex_id: u32) {
    match vgfx::texture_dimensions(tex_id) {
        Some((width, height)) => bp_info!("apidemo: {} {}x{}", label, width, height),
        None => bp_error!("apidemo: {} unavailable", label),
    }
}

fn log_window_info(label: &str, info: ui2::WindowInfo) {
    bp_info!(
        "apidemo: {} id={} kind={} state={:?} frame=({},{} {}x{}) visible={} selected={}",
        label,
        info.id.raw(),
        info.kind,
        info.state,
        info.frame.x,
        info.frame.y,
        info.frame.width,
        info.frame.height,
        info.visible,
        info.selected,
    );
}
