// trueos-blueprint: features=["gridpaper"]
#![no_std]

use gridpaper::{
    AnimationIteration, AnimationTiming, COLOR_KEYFRAME_CAPACITY, Cell, CellStyle, Color,
    ColorAnimation, ColorChannels, ColorKeyframe, FontInstanceProgram, FontStyle, GridPaper,
    GridPaperConfig, PublishMode, Rgba8, SnapshotCadence, TrigAnimation,
};
use trueos::{
    clock, env,
    logl::{self, level},
    platform, print2d, replication, vshell,
};

const TRACKED_PRINT_JOBS: usize = 8;
const PRINT_STATUS_INTERVAL_MS: u64 = 250;
const CHECKPOINT_VERSION: u64 = 1;

#[derive(Clone, Copy)]
struct TrackedPrintJob {
    id: print2d::JobId,
    state: print2d::JobState,
}

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
const FONT_INSTANCE_DEMO_COLORS: [Color; 5] = [
    Color::BrightRed,
    Color::BrightYellow,
    Color::BrightGreen,
    Color::BrightCyan,
    Color::BrightMagenta,
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
const ASCII_DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const ASCII_LETTERS: [&str; 7] = ["a", "b", "c", "d", "e", "f", "g"];
const ASCII_SPECIMEN_ROWS: [(usize, usize); 3] = [(14, 17), (31, 34), (48, 51)];
const ASCII_SPECIMEN_STYLES: [CellStyle; ASCII_SPECIMEN_ROWS.len()] =
    [CellStyle::NONE, CellStyle::BOLD, CellStyle::ITALIC];
const ASCII_SPECIMEN_COLOR_PHASES: [usize; ASCII_SPECIMEN_ROWS.len()] = [0, 5, 10];

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
    let grid_size = match requested_grid_size() {
        Ok(size) => size,
        Err(error) => {
            logl::log(level::ERROR, error);
            return;
        }
    };
    let start_ms = clock::monotonic_millis();
    let config = GridPaperConfig {
        cadence: SnapshotCadence::EveryEditsOrMillis {
            edits: 32,
            millis: 16,
        },
        publish_mode: PublishMode::PreserveIncrementalEdits,
        initial_time_ms: start_ms,
    };
    let mut page = GridPaper::new(config);
    page.set_scale_percent(100);
    install_full_rainbow_text_animations(&mut page);
    initialize_unicode_demo(&mut page, start_ms);
    let mut submitted_animation_generation = u64::MAX;
    submit_to_kernel(&page, grid_size, &mut submitted_animation_generation);

    let mut input = [0_u8; 64];
    let mut print_jobs = [None; TRACKED_PRINT_JOBS];
    let mut next_print_status_ms = start_ms;
    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            prepare_pause(
                prepare,
                &mut page,
                grid_size,
                &mut submitted_animation_generation,
                &mut print_jobs,
                &mut next_print_status_ms,
            );
            continue;
        }
        drain_print_screen_requests(&mut print_jobs);
        let now_ms = clock::monotonic_millis();
        if now_ms >= next_print_status_ms {
            poll_print_jobs(&mut print_jobs);
            next_print_status_ms = now_ms.saturating_add(PRINT_STATUS_INTERVAL_MS);
        }

        let read = vshell::read(&mut input);
        if read == 0 {
            platform::poll_once();
            platform::sleep_ms(16);
            continue;
        }
        let command = trim_ascii(&input[..read]);
        match command {
            b"quit" => break,
            b"snapshot" => {
                let now_ms = clock::monotonic_millis();
                let _ = page.publish(now_ms);
                submit_to_kernel(&page, grid_size, &mut submitted_animation_generation);
            }
            b"clear" => {
                let now_ms = clock::monotonic_millis();
                let mut edit = page.edit(now_ms);
                edit.raw_mut().fill(0);
                let _ = edit.finish();
                let _ = page.publish(now_ms);
                submit_to_kernel(&page, grid_size, &mut submitted_animation_generation);
            }
            bytes => match core::str::from_utf8(bytes) {
                Ok(text) => match Cell::new(text, Color::BrightBlue, Color::White, CellStyle::BOLD)
                {
                    Ok(cell) => {
                        let now_ms = clock::monotonic_millis();
                        let mut edit = page.edit(now_ms);
                        let _ = edit.set_cell(0, 0, cell);
                        let _ = edit.finish();
                        let _ = page.publish(now_ms);
                        submit_to_kernel(&page, grid_size, &mut submitted_animation_generation);
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
            format_args!("gridpaper: kernel close failed error={error:?}"),
        );
    }
}

fn prepare_pause(
    prepare: replication::PreparePause,
    page: &mut GridPaper,
    grid_size: trueos::gridpaper::GridSize,
    submitted_animation_generation: &mut u64,
    print_jobs: &mut [Option<TrackedPrintJob>; TRACKED_PRINT_JOBS],
    next_print_status_ms: &mut u64,
) {
    if let Err(error) = checkpoint_and_release_kernel_page(page) {
        logl::log(
            level::WARN,
            format_args!(
                "gridpaper: PreparePause operation={} page checkpoint/release failed error={error:?}; not Ready",
                prepare.operation()
            ),
        );
        return;
    }

    // Print jobs are host-owned work, not cloneable Blueprint capabilities.
    // Submitted jobs continue independently; resumed instances start tracking
    // only jobs they submit themselves.
    print_jobs.fill(None);
    logl::log(
        level::INFO,
        format_args!(
            "gridpaper: PreparePause operation={} reason={:?}; page checkpointed, UI4 frame and scene release committed, Ready",
            prepare.operation(),
            prepare.reason
        ),
    );

    let resume = replication::ready(prepare, CHECKPOINT_VERSION);

    // Checkpoint/release already succeeded even if Ready became stale, so
    // always rebuild the disposable projection before returning to the loop.
    *submitted_animation_generation = u64::MAX;
    submit_to_kernel(page, grid_size, submitted_animation_generation);
    *next_print_status_ms = clock::monotonic_millis().saturating_add(PRINT_STATUS_INTERVAL_MS);

    match resume {
        Ok(resume) => logl::log(
            level::INFO,
            format_args!(
                "gridpaper: Resume instance={} lineage={} generation={} clone={}; page and animations resubmitted",
                resume.instance_guid(),
                resume.lineage_guid(),
                resume.generation,
                resume.is_clone
            ),
        ),
        Err(error) => logl::log(
            level::WARN,
            format_args!("gridpaper: Ready rejected error={error:?}; kernel projection restored"),
        ),
    }
}

fn checkpoint_and_release_kernel_page(
    page: &mut GridPaper,
) -> Result<(), trueos::gridpaper::Error> {
    let mut raw = [0u8; gridpaper::PAGE_BYTES];
    trueos::gridpaper::checkpoint_snapshot(&mut raw)?;
    if page.snapshot().raw() != &raw {
        let now_ms = clock::monotonic_millis();
        {
            let mut edit = page.edit(now_ms);
            edit.raw_mut().copy_from_slice(&raw);
            let _ = edit.finish();
        }
        let _ = page.publish(now_ms);
    }
    Ok(())
}

fn drain_print_screen_requests(jobs: &mut [Option<TrackedPrintJob>; TRACKED_PRINT_JOBS]) {
    while let Some(request) = trueos::gridpaper::take_print_request() {
        match print2d::submit_gridpaper_request(request.token()) {
            Ok(id) => {
                remember_print_job(jobs, id);
                logl::log(
                    level::INFO,
                    format_args!(
                        "gridpaper: print2d job={} state=Queued trigger=PrintScreen",
                        id.get()
                    ),
                );
            }
            Err(print2d::Error::QueueFull) => {
                // The kernel deliberately leaves this token available; retry
                // it after the spooler drains one slot.
                break;
            }
            Err(error) => {
                logl::log(
                    level::WARN,
                    format_args!("gridpaper: Print Screen submit failed: {error:?}"),
                );
                break;
            }
        }
    }
}

fn remember_print_job(
    jobs: &mut [Option<TrackedPrintJob>; TRACKED_PRINT_JOBS],
    id: print2d::JobId,
) {
    if let Some(slot) = jobs.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(TrackedPrintJob {
            id,
            state: print2d::JobState::Queued,
        });
    } else {
        logl::log(
            level::WARN,
            format_args!("gridpaper: print2d tracking full job={}", id.get()),
        );
    }
}

fn poll_print_jobs(jobs: &mut [Option<TrackedPrintJob>; TRACKED_PRINT_JOBS]) {
    for slot in jobs {
        let Some(mut tracked) = *slot else {
            continue;
        };
        match print2d::status(tracked.id) {
            Ok(state) => {
                if state != tracked.state {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "gridpaper: print2d job={} state={state:?}",
                            tracked.id.get()
                        ),
                    );
                    tracked.state = state;
                }
                if state.is_done() {
                    *slot = None;
                } else {
                    *slot = Some(tracked);
                }
            }
            Err(error) => {
                logl::log(
                    level::WARN,
                    format_args!(
                        "gridpaper: print2d status failed job={} error={error:?}",
                        tracked.id.get()
                    ),
                );
                *slot = None;
            }
        }
    }
}

/// Place three sparse Unicode waves and normal/bold/italic ASCII specimens in
/// one edit. Untouched cells remain empty.
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
        for (specimen, ((digit_row, letter_row), style)) in ASCII_SPECIMEN_ROWS
            .iter()
            .copied()
            .zip(ASCII_SPECIMEN_STYLES.iter().copied())
            .enumerate()
        {
            let color_phase = ASCII_SPECIMEN_COLOR_PHASES[specimen];
            for (index, glyph) in ASCII_DIGITS.iter().enumerate() {
                let cell = Cell::new(
                    glyph,
                    ACTIVE_TEXT_COLORS[(index + color_phase) % ACTIVE_TEXT_COLORS.len()],
                    Color::Transparent,
                    style,
                )
                .expect("static ASCII digit fits one cell");
                edit.set_cell(9 + index * 2, digit_row, cell)
                    .expect("static ASCII digit coordinate is in bounds");
            }
            for (index, glyph) in ASCII_LETTERS.iter().enumerate() {
                let cell = Cell::new(
                    glyph,
                    ACTIVE_TEXT_COLORS[(index + 10 + color_phase) % ACTIVE_TEXT_COLORS.len()],
                    Color::Transparent,
                    style,
                )
                .expect("static ASCII letter fits one cell");
                edit.set_cell(12 + index * 2, letter_row, cell)
                    .expect("static ASCII letter coordinate is in bounds");
            }
        }
        edit.set_cell(
            18,
            11,
            Cell::with_upper(
                "x",
                "²",
                Color::BrightBlue,
                Color::Transparent,
                CellStyle::NONE,
            )
            .expect("static x-squared demo fits one cell"),
        )
        .expect("static x-squared demo coordinate is in bounds");
        let _ = edit.finish();
    }
    let _ = page.publish(now_ms);
}

/// Exercise five independent persistent instances and the maximum keyframe
/// capacity. The remaining palette layers use the kernel's identity fast path.
fn install_full_rainbow_text_animations(page: &mut GridPaper) {
    for (selector, color) in FONT_INSTANCE_DEMO_COLORS.iter().copied().enumerate() {
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
        let phase_permille = (selector as u16 * 59) % 1_000;
        let rotation_amplitude = if selector.is_multiple_of(2) {
            240
        } else {
            -240
        };
        let motion =
            TrigAnimation::new(7_000, phase_permille, rotation_amplitude, 45, -100, 15, 10)
                .expect("bounded selector motion is valid");
        page.set_font_instance_program(
            color,
            Some(FontInstanceProgram::new(
                Some(animation),
                FontStyle::IDENTITY,
                motion,
            )),
        )
        .expect("font-instance selector is an active foreground color");
    }
}

fn requested_grid_size() -> Result<trueos::gridpaper::GridSize, &'static str> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Ok(trueos::gridpaper::GridSize::FULL);
    };
    let Some((columns, rows)) = first
        .split_once('x')
        .or_else(|| first.split_once('X'))
        .or_else(|| first.split_once("by"))
    else {
        return Err("gridpaper: expected grid size as COLUMNSxROWS");
    };
    if args.next().is_some() {
        return Err("gridpaper: expected one grid size, for example 12x20");
    }
    let columns = columns
        .parse::<usize>()
        .map_err(|_| "gridpaper: grid columns must be a positive integer")?;
    let rows = rows
        .parse::<usize>()
        .map_err(|_| "gridpaper: grid rows must be a positive integer")?;
    trueos::gridpaper::GridSize::new(columns, rows)
        .map_err(|_| "gridpaper: grid size must be within 1x1 and 39x55")
}

fn submit_to_kernel(
    page: &GridPaper,
    grid_size: trueos::gridpaper::GridSize,
    submitted_animation_generation: &mut u64,
) {
    let snapshot = page.snapshot();
    let submitted = if grid_size == trueos::gridpaper::GridSize::FULL {
        trueos::gridpaper::submit_snapshot(
            snapshot.generation(),
            snapshot.scale_percent(),
            snapshot.raw(),
        )
    } else {
        trueos::gridpaper::submit_sized_snapshot(
            grid_size,
            snapshot.generation(),
            snapshot.scale_percent(),
            snapshot.raw(),
        )
    };
    if let Err(error) = submitted {
        logl::log(
            level::WARN,
            format_args!("gridpaper: kernel snapshot submit failed error={error:?}"),
        );
    }
    if snapshot.animation_generation() != *submitted_animation_generation {
        match trueos::gridpaper::submit_font_instances(snapshot.font_instance_programs()) {
            Ok(()) => *submitted_animation_generation = snapshot.animation_generation(),
            Err(error) => logl::log(
                level::WARN,
                format_args!("gridpaper: kernel text animations submit failed error={error:?}"),
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
