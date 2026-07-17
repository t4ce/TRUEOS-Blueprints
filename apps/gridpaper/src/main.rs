// trueos-blueprint: features=["gridpaper"]
#![no_std]

use gridpaper::{
    AnimationIteration, AnimationTiming, COLOR_KEYFRAME_CAPACITY, Cell, CellStyle, Color,
    ColorAnimation, ColorChannels, ColorKeyframe, GridPaper, GridPaperConfig, PublishMode, Rgba8,
    SnapshotCadence,
};
use trueos::{
    clock,
    logl::{self, level},
    vshell,
};

const ACTIVE_TEXT_COLORS: [Color; gridpaper::TEXT_COLOR_ANIMATION_SLOTS] = [
    Color::Default,
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::White,
    Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    Color::BrightWhite,
];

const UNICODE_WAVES: [[&str; gridpaper::TEXT_COLOR_ANIMATION_SLOTS]; 3] = [
    [
        "α", "β", "γ", "δ", "λ", "π", "Σ", "Ω", "∞", "∫", "√", "≈", "≠", "≤", "≥", "±", "∂",
    ],
    [
        "Ж", "Я", "Д", "Ф", "Ю", "♪", "♫", "▲", "△", "◆", "◇", "●", "○", "♠", "♣", "♥", "♦",
    ],
    [
        "←", "↖", "↑", "↗", "→", "↘", "↓", "↙", "⇐", "⇑", "⇒", "⇓", "⇔", "⊕", "⊗", "⊙", "⊥",
    ],
];

const WAVE_BASE_ROWS: [usize; UNICODE_WAVES.len()] = [5, 22, 39];
const WAVE_ROW_OFFSETS: [usize; gridpaper::TEXT_COLOR_ANIMATION_SLOTS] =
    [0, 1, 3, 5, 6, 5, 3, 1, 0, 1, 3, 5, 6, 5, 3, 1, 0];
const RAINBOW_KEYFRAME_OFFSETS: [u16; COLOR_KEYFRAME_CAPACITY] =
    [0, 143, 286, 429, 571, 714, 857, 1_000];
const RAINBOW_COLORS: [Rgba8; COLOR_KEYFRAME_CAPACITY - 1] = [
    Rgba8::new(255, 72, 96, 255),
    Rgba8::new(255, 142, 56, 224),
    Rgba8::new(255, 220, 72, 255),
    Rgba8::new(72, 224, 112, 224),
    Rgba8::new(64, 216, 232, 255),
    Rgba8::new(72, 112, 255, 224),
    Rgba8::new(224, 72, 240, 255),
];

fn main() {
    let start_ms = clock::monotonic_millis();
    let mut page = GridPaper::new(GridPaperConfig {
        cadence: SnapshotCadence::EveryEditsOrMillis {
            edits: 32,
            millis: 16,
        },
        publish_mode: PublishMode::PreserveIncrementalEdits,
        initial_time_ms: start_ms,
    });
    install_full_rainbow_text_animations(&mut page);

    initialize_unicode_demo(&mut page, start_ms);
    let mut submitted_animation_generation = u64::MAX;
    submit_to_kernel(&page, &mut submitted_animation_generation);

    let mut input = [0_u8; gridpaper::CELL_TEXT_CAPACITY + 2];
    loop {
        let read = vshell::read_blocking(&mut input);
        let command = trim_ascii(&input[..read]);
        match command {
            b"quit" => break,
            b"snapshot" => {
                let event = page.publish(clock::monotonic_millis());
                submit_to_kernel(&page, &mut submitted_animation_generation);
                logl::log(
                    level::INFO,
                    format_args!("gridpaper: snapshot generation={}", event.generation()),
                );
            }
            b"clear" => {
                {
                    let mut edit = page.edit(clock::monotonic_millis());
                    edit.raw_mut().fill(0);
                    let _ = edit.finish();
                }
                let event = page.publish(clock::monotonic_millis());
                submit_to_kernel(&page, &mut submitted_animation_generation);
                logl::log(
                    level::INFO,
                    format_args!("gridpaper: cleared generation={}", event.generation()),
                );
            }
            bytes => match core::str::from_utf8(bytes) {
                Ok(text) => match Cell::new(text, Color::BrightBlue, Color::White, CellStyle::BOLD)
                {
                    Ok(cell) => {
                        let mut edit = page.edit(clock::monotonic_millis());
                        let _ = edit.set_cell(0, 0, cell);
                        let _ = edit.finish();
                        let event = page.publish(clock::monotonic_millis());
                        submit_to_kernel(&page, &mut submitted_animation_generation);
                        logl::log(
                            level::INFO,
                            format_args!(
                                "gridpaper: staged {:?} generation={}",
                                text,
                                event.generation(),
                            ),
                        );
                    }
                    Err(error) => {
                        logl::log(level::WARN, format_args!("gridpaper: {error}"));
                    }
                },
                Err(_) => logl::log(level::WARN, format_args!("gridpaper: invalid UTF-8")),
            },
        }
    }
    if let Err(error) = trueos::gridpaper::close() {
        logl::log(
            level::WARN,
            format_args!("gridpaper: kernel close failed: {error:?}"),
        );
    }
}

/// Place three sparse Unicode waves in one edit. Every foreground selector is
/// represented in every wave while untouched cells remain empty.
fn initialize_unicode_demo(page: &mut GridPaper, now_ms: u64) {
    {
        let mut edit = page.edit(now_ms);
        edit.raw_mut().fill(0);
        for (wave_index, glyphs) in UNICODE_WAVES.iter().enumerate() {
            for (selector, glyph) in glyphs.iter().enumerate() {
                let column = 2 + selector * 2;
                let row = WAVE_BASE_ROWS[wave_index] + WAVE_ROW_OFFSETS[selector];
                let style = match wave_index {
                    0 => CellStyle::NONE,
                    1 => CellStyle::BOLD,
                    _ => match selector % 4 {
                        0 => CellStyle::UNDERLINE,
                        1 => CellStyle::STRIKEOUT,
                        2 => CellStyle::ITALIC,
                        _ => CellStyle::BOLD.union(CellStyle::UNDERLINE),
                    },
                };
                let cell = Cell::new(
                    glyph,
                    ACTIVE_TEXT_COLORS[selector],
                    Color::Transparent,
                    style,
                )
                .expect("static Unicode demo glyph fits one cell");
                edit.set_cell(column, row, cell)
                    .expect("static Unicode demo coordinate is in bounds");
            }
        }
        let _ = edit.finish();
    }
    let _ = page.publish(now_ms);
}

/// Exercise the complete animation table and the maximum keyframe capacity.
/// Rotating the seven unique stops gives each selector a stable spatial phase;
/// the eighth stop closes its loop without discontinuity.
fn install_full_rainbow_text_animations(page: &mut GridPaper) {
    for (selector, color) in ACTIVE_TEXT_COLORS.iter().copied().enumerate() {
        let phase = selector % RAINBOW_COLORS.len();
        let mut keyframes = [ColorKeyframe::new(0, Rgba8::TRANSPARENT); COLOR_KEYFRAME_CAPACITY];
        for (index, keyframe) in keyframes.iter_mut().enumerate() {
            let stop = if index + 1 == COLOR_KEYFRAME_CAPACITY {
                phase
            } else {
                (index + phase) % RAINBOW_COLORS.len()
            };
            *keyframe = ColorKeyframe::new(RAINBOW_KEYFRAME_OFFSETS[index], RAINBOW_COLORS[stop]);
        }
        let animation = ColorAnimation::keyframes(
            &keyframes,
            ColorChannels::RGBA,
            7_000,
            AnimationTiming::Linear,
            AnimationIteration::Loop,
        )
        .expect("static full-capacity rainbow keyframes are valid");
        page.set_text_color_animation(color, Some(animation))
            .expect("animation selector is an active foreground color");
    }
}

fn submit_to_kernel(page: &GridPaper, submitted_animation_generation: &mut u64) {
    let snapshot = page.snapshot();
    if let Err(error) = trueos::gridpaper::submit_snapshot(
        snapshot.generation(),
        snapshot.scale_percent(),
        snapshot.raw(),
    ) {
        logl::log(
            level::WARN,
            format_args!("gridpaper: kernel snapshot submit failed: {error:?}"),
        );
    }
    if snapshot.animation_generation() != *submitted_animation_generation {
        match trueos::gridpaper::submit_text_animations(snapshot.text_color_animations()) {
            Ok(()) => *submitted_animation_generation = snapshot.animation_generation(),
            Err(error) => logl::log(
                level::WARN,
                format_args!("gridpaper: kernel text animations submit failed: {error:?}"),
            ),
        }
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
