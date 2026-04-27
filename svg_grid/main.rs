#![no_std]
#![no_main]

use core::panic::PanicInfo;
use trueos::{TrueosAllocator, panic_abort};
use trueos::{ui2, vgfx, vsys};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const WINDOW_TITLE: &str = "SVG Grid BP";
const WINDOW_X: i32 = 960;
const WINDOW_Y: i32 = 120;
const WINDOW_WIDTH: u32 = 272;
const WINDOW_HEIGHT: u32 = 204;
const TEX_ID: u32 = 4_761;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("svg_grid bp: panic\n")
}

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
        vsys::log_error("svg_grid bp: window create failed\n");
        return;
    };

    let rc = vgfx::upload_svg_to_texture(TEX_ID, SVG_GRID.as_bytes());
    if rc != 0 {
        vsys::log_errorf(format_args!("svg_grid bp: svg upload failed rc={}\n", rc));
        return;
    }
    let _ = window.id().request_repaint();
    vsys::log_info("svg_grid bp: rendered via encoded SVG upload\n");

    loop {
        vsys::poll_once();
    }
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

  <svg x="4" y="4" width="64" height="64" viewBox="0 0 96 96">
    <defs>
      <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#132a4f"/>
        <stop offset="55%" stop-color="#f26b5b"/>
        <stop offset="100%" stop-color="#ffd27a"/>
      </linearGradient>
      <radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0%" stop-color="#fff3bf"/>
        <stop offset="100%" stop-color="#ff9f43"/>
      </radialGradient>
    </defs>
    <rect width="96" height="96" fill="url(#sky)"/>
    <circle cx="48" cy="38" r="18" fill="url(#sun)"/>
    <path d="M0 64 C10 58 20 56 32 60 C42 63 54 66 66 62 C78 58 87 59 96 64 L96 96 L0 96 Z" fill="#553c66"/>
    <path d="M0 74 C10 70 20 67 32 70 C42 73 56 76 70 72 C82 68 90 69 96 72 L96 96 L0 96 Z" fill="#2c2348"/>
    <path d="M0 84 C12 80 23 78 34 81 C46 84 58 87 70 84 C81 81 90 82 96 84 L96 96 L0 96 Z" fill="#161126"/>
  </svg>

  <svg x="72" y="4" width="64" height="64" viewBox="0 0 96 96">
    <defs>
      <linearGradient id="petal" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="#ff8fb1"/>
        <stop offset="100%" stop-color="#ff4d6d"/>
      </linearGradient>
      <radialGradient id="core" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0%" stop-color="#fff4b5"/>
        <stop offset="100%" stop-color="#ffb703"/>
      </radialGradient>
    </defs>
    <rect width="96" height="96" fill="#fff7ef"/>
    <g fill="url(#petal)" stroke="#7a284a" stroke-width="2" stroke-linejoin="round">
      <path d="M48 18 C60 22 66 31 66 42 C58 45 52 45 48 42 C44 45 38 45 30 42 C30 31 36 22 48 18 Z"/>
      <path d="M78 48 C74 60 65 66 54 66 C51 58 51 52 54 48 C51 44 51 38 54 30 C65 30 74 36 78 48 Z"/>
      <path d="M48 78 C36 74 30 65 30 54 C38 51 44 51 48 54 C52 51 58 51 66 54 C66 65 60 74 48 78 Z"/>
      <path d="M18 48 C22 36 31 30 42 30 C45 38 45 44 42 48 C45 52 45 58 42 66 C31 66 22 60 18 48 Z"/>
    </g>
    <circle cx="48" cy="48" r="10" fill="url(#core)" stroke="#8c5a00" stroke-width="2"/>
  </svg>

  <svg x="140" y="4" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="12" fill="#091a16"/>
    <circle cx="48" cy="48" r="28" fill="#21664c"/>
    <circle cx="48" cy="48" r="12" fill="none" stroke="#7df9c1" stroke-width="2"/>
    <circle cx="48" cy="48" r="24" fill="none" stroke="#4dd9a6" stroke-width="2" stroke-opacity="0.8"/>
    <circle cx="48" cy="48" r="36" fill="none" stroke="#2ca67f" stroke-width="2" stroke-opacity="0.6"/>
    <path d="M48 48 L76 34 A32 32 0 0 1 80 48 Z" fill="#8ff7c8" fill-opacity="0.35"/>
    <path d="M48 14 L48 82 M14 48 L82 48" stroke="#74e7b7" stroke-width="1.5" stroke-linecap="round"/>
    <circle cx="48" cy="48" r="4" fill="#d7fff0"/>
  </svg>

  <svg x="208" y="4" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" fill="#090b1a"/>
    <path d="M20 72 C16 54 20 34 34 24 C46 16 62 16 72 24 C82 32 82 48 72 56 C62 64 46 64 34 56 C24 49 24 38 32 32 C39 27 49 27 56 32" fill="none" stroke="#7dd3fc" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M18 76 C30 68 42 64 54 64 C44 70 32 78 24 88 Z" fill="#7dd3fc" fill-opacity="0.35"/>
    <circle cx="58" cy="34" r="8" fill="#ffb347" stroke="#ffedd5" stroke-width="1.5"/>
    <circle cx="70" cy="22" r="2" fill="#ffffff"/>
    <circle cx="78" cy="30" r="1.5" fill="#ffffff" fill-opacity="0.8"/>
  </svg>

  <svg x="4" y="72" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#e6f6ff"/>
    <circle cx="48" cy="48" r="18" fill="#ffb703" stroke="#d97706" stroke-width="2.5"/>
    <path d="M48 10 L48 22 M48 74 L48 86 M10 48 L22 48 M74 48 L86 48 M21 21 L29 29 M67 67 L75 75 M21 75 L29 67 M67 29 L75 21" stroke="#f59e0b" stroke-width="4" stroke-linecap="round"/>
  </svg>

  <svg x="72" y="72" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#dff3ff"/>
    <circle cx="34" cy="32" r="14" fill="#f59e0b" stroke="#d97706" stroke-width="2"/>
    <path d="M34 10 L34 16 M34 48 L34 54 M12 32 L18 32 M50 32 L56 32 M18 18 L22 22 M46 42 L50 46 M18 46 L22 42 M46 22 L50 18" stroke="#f59e0b" stroke-width="3" stroke-linecap="round"/>
    <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="#f8fbff" stroke="#7b93b7" stroke-width="2.5" stroke-linejoin="round"/>
  </svg>

  <svg x="140" y="72" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#eaf2f8"/>
    <path d="M18 56 C18 48 24 42 32 42 C35 42 38 43 40 45 C43 39 49 35 56 35 C66 35 74 43 74 53 C74 63 66 71 56 71 L32 71 C24 71 18 64 18 56 Z" fill="#b8c6d9" stroke="#8a9aad" stroke-width="2"/>
    <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="#ffffff" stroke="#7b93b7" stroke-width="2.5"/>
  </svg>

  <svg x="208" y="72" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#edf6ff"/>
    <path d="M22 52 C22 43 29 36 38 36 C41 36 45 37 48 39 C51 32 58 28 66 28 C77 28 86 37 86 48 C86 60 77 69 66 69 L38 69 C29 69 22 61 22 52 Z" fill="#f7fbff" stroke="#7b93b7" stroke-width="2.5"/>
    <path d="M34 74 C36 68 39 64 42 60 C45 64 48 68 50 74 C50 78 46 82 42 82 C38 82 34 78 34 74 Z" fill="#2563eb"/>
    <path d="M50 80 C52 74 55 70 58 66 C61 70 64 74 66 80 C66 84 62 88 58 88 C54 88 50 84 50 80 Z" fill="#2563eb"/>
    <path d="M66 74 C68 68 71 64 74 60 C77 64 80 68 82 74 C82 78 78 82 74 82 C70 82 66 78 66 74 Z" fill="#2563eb"/>
  </svg>

  <svg x="4" y="140" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#e8edf5"/>
    <path d="M20 50 C20 41 27 34 36 34 C40 34 43 35 46 37 C49 30 56 26 64 26 C76 26 86 36 86 48 C86 60 76 70 64 70 L36 70 C27 70 20 62 20 50 Z" fill="#97a7bd" stroke="#6f8197" stroke-width="2.5"/>
    <path d="M52 48 L42 66 L50 66 L44 86 L66 60 L56 60 L64 48 Z" fill="#facc15" stroke="#ca8a04" stroke-width="2" stroke-linejoin="round"/>
  </svg>

  <svg x="72" y="140" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="18" fill="#eef7ff"/>
    <path d="M22 50 C22 41 29 34 38 34 C41 34 45 35 48 37 C51 30 58 26 66 26 C77 26 86 35 86 46 C86 58 77 67 66 67 L38 67 C29 67 22 59 22 50 Z" fill="#f9fcff" stroke="#88a0bb" stroke-width="2.5"/>
    <path d="M34 76 L42 76 M38 72 L38 80 M35 73 L41 79 M41 73 L35 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
    <path d="M54 84 L62 84 M58 80 L58 88 M55 81 L61 87 M61 81 L55 87" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
    <path d="M70 76 L78 76 M74 72 L74 80 M71 73 L77 79 M77 73 L71 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
  </svg>

  <svg x="140" y="140" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="16" fill="#E6D34D"/>
    <path fill="#2F2F2F" d="M10 72 V24 H22 L30 47 L38 24 H50 V72 H42 V38 L33 63 H27 L18 38 V72 Z"/>
    <path fill="#2F2F2F" d="M54 24 H82 V32 H70 V72 H62 V32 H54 Z"/>
    <path fill="#2F2F2F" d="M52 60 C52 69 46 74 36 74 H30 V66 H35 C40 66 44 64 44 58 V24 H52 Z"/>
  </svg>

  <svg x="208" y="140" width="64" height="64" viewBox="0 0 96 96">
    <rect width="96" height="96" rx="14" fill="#132238"/>
    <path d="M8 28 C20 16 34 16 46 28 C58 40 72 40 88 28" fill="none" stroke="#6ee7f9" stroke-width="8" stroke-linecap="round"/>
    <path d="M8 48 C20 36 34 36 46 48 C58 60 72 60 88 48" fill="none" stroke="#f97316" stroke-width="8" stroke-linecap="round"/>
    <path d="M8 68 C20 56 34 56 46 68 C58 80 72 80 88 68" fill="none" stroke="#6ee7f9" stroke-width="8" stroke-linecap="round"/>
  </svg>
</svg>"##;
