#![no_std]

use trueos::hid::{
    MOUSE_MOTION_EASING_FAST_LINEAR, MOUSE_MOTION_EASING_NATURAL, MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    MOUSE_MOTION_OPCODE_BUTTONS, MOUSE_MOTION_OPCODE_STROKE, MOUSE_MOTION_PATH_CUBIC,
    MOUSE_MOTION_PATH_LINE, MouseMotionCommand, VCursor,
};
use trueos::logl::{self, level};
use trueos::ui4_scene::output_dimensions;
use trueos::{clock, rng, vshell, vsys};

const WINDOW_WIDTH: u32 = 480;
const WINDOW_HEIGHT: u32 = 320;
const ORBIT_RADIUS: i32 = 300;
const PRIMARY_BUTTON: u32 = 1 << 0;
const SECONDARY_BUTTON: u32 = 1 << 1;
const ORBIT_CONTROL_PERCENT: i32 = 13;
const MIN_CLOSE_DELAY_MS: u64 = 2_000;
const MAX_CLOSE_DELAY_MS: u64 = 5_000;

#[derive(Clone, Copy)]
struct Orbit {
    label: &'static str,
    start: usize,
    radius_multiple: i32,
    clockwise: bool,
}

const ORBITS: [Orbit; 8] = [
    Orbit {
        label: "hello-orbit-a",
        start: 0,
        radius_multiple: 0,
        clockwise: true,
    },
    Orbit {
        label: "hello-orbit-b",
        start: 3,
        radius_multiple: 1,
        clockwise: false,
    },
    Orbit {
        label: "hello-orbit-c",
        start: 6,
        radius_multiple: 2,
        clockwise: true,
    },
    Orbit {
        label: "hello-orbit-d",
        start: 9,
        radius_multiple: 3,
        clockwise: false,
    },
    Orbit {
        label: "hello-orbit-e",
        start: 11,
        radius_multiple: 1,
        clockwise: false,
    },
    Orbit {
        label: "hello-orbit-f",
        start: 0,
        radius_multiple: 2,
        clockwise: true,
    },
    Orbit {
        label: "hello-orbit-g",
        start: 13,
        radius_multiple: 2,
        clockwise: true,
    },
    Orbit {
        label: "hello-orbit-h",
        start: 1,
        radius_multiple: 3,
        clockwise: false,
    },
];

struct WindowRun {
    window: u32,
    cursor: Option<VCursor>,
    close_at_ms: Option<u64>,
}

struct SoftRng(u64);

impl SoftRng {
    fn close_delay_ms(&mut self) -> u64 {
        // PlotTwist uses the same xorshift stream for demo randomness.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        MIN_CLOSE_DELAY_MS + self.0 % (MAX_CLOSE_DELAY_MS - MIN_CLOSE_DELAY_MS + 1)
    }
}

// Sixteen points keep each program comfortably below the mediated cursor's
// bounded queue while cubic controls turn the polygon into a smooth circle.
const ORBIT_POINTS: [(i32, i32); 16] = [
    orbit_point(100, 0),
    orbit_point(92, 38),
    orbit_point(71, 71),
    orbit_point(38, 92),
    orbit_point(0, 100),
    orbit_point(-38, 92),
    orbit_point(-71, 71),
    orbit_point(-92, 38),
    orbit_point(-100, 0),
    orbit_point(-92, -38),
    orbit_point(-71, -71),
    orbit_point(-38, -92),
    orbit_point(0, -100),
    orbit_point(38, -92),
    orbit_point(71, -71),
    orbit_point(92, -38),
];

const fn orbit_point(x_percent: i32, y_percent: i32) -> (i32, i32) {
    (
        x_percent * ORBIT_RADIUS / 100,
        y_percent * ORBIT_RADIUS / 100,
    )
}

fn main() {
    let (output_width, output_height) = match output_dimensions() {
        Ok(dimensions) => dimensions,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("hello_world: UI4 output dimensions unavailable: {error:?}"),
            );
            return;
        }
    };
    let center = ((output_width / 2) as i32, (output_height / 2) as i32);
    let mut windows = ORBITS.map(|orbit| {
        let (x, y) = orbit_position(center, orbit.start, orbit.radius_multiple);
        match vshell::spawn_window(
            x - WINDOW_WIDTH as i32 / 2,
            y - WINDOW_HEIGHT as i32 / 2,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        ) {
            Ok(window) => Some(WindowRun {
                window,
                cursor: None,
                close_at_ms: None,
            }),
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "hello_world: shell spawn failed label={} error={error}",
                        orbit.label
                    ),
                );
                None
            }
        }
    });

    for (slot, orbit) in windows.iter_mut().zip(ORBITS) {
        let Some(run) = slot else { continue };
        match VCursor::request(orbit.label) {
            Ok(cursor) => {
                if let Err(error) = start_orbit(&cursor, center, orbit) {
                    logl::log(
                        level::ERROR,
                        format_args!(
                            "hello_world: cursor program failed window={} error={error}",
                            run.window
                        ),
                    );
                    release_cursor(cursor);
                    continue;
                }
                logl::log(
                    level::INFO,
                    format_args!(
                        "hello_world: pair ready window={} cursor_handle={} cursor_slot={}",
                        run.window,
                        cursor.handle(),
                        cursor.slot_id(),
                    ),
                );
                run.cursor = Some(cursor);
            }
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "hello_world: cursor request failed window={} error={error}",
                        run.window
                    ),
                );
            }
        }
    }

    let mut soft_rng = SoftRng((rng::u64() ^ clock::monotonic_nanos()).max(1));
    while windows.iter().any(Option::is_some) {
        for slot in &mut windows {
            let Some(run) = slot else { continue };
            if run.close_at_ms.is_none() {
                if let Some(cursor) = &run.cursor {
                    match cursor.idle() {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(error) => logl::log(
                            level::ERROR,
                            format_args!(
                                "hello_world: cursor wait failed window={} error={error}",
                                run.window
                            ),
                        ),
                    }
                }
                let delay_ms = soft_rng.close_delay_ms();
                run.close_at_ms = Some(clock::monotonic_millis().saturating_add(delay_ms));
                logl::log(
                    level::INFO,
                    format_args!(
                        "hello_world: shell window {} closing in {delay_ms} ms",
                        run.window
                    ),
                );
            }
            if run
                .close_at_ms
                .is_some_and(|deadline| clock::monotonic_millis() >= deadline)
                && let Some(run) = slot.take()
            {
                finish_window(run);
            }
        }
        vsys::poll_once();
        vsys::sleep_ms(8);
    }
}

fn finish_window(run: WindowRun) {
    let window = run.window;
    match vshell::close_window(window) {
        Ok(()) => logl::log(
            level::INFO,
            format_args!("hello_world: shell window {window} closed"),
        ),
        Err(error) => logl::log(
            level::ERROR,
            format_args!("hello_world: shell window {window} close failed error={error}"),
        ),
    }
    if let Some(cursor) = run.cursor {
        release_cursor(cursor);
    }
}

fn release_cursor(cursor: VCursor) {
    match cursor.close() {
        Ok(()) => logl::log(level::INFO, "hello_world: cursor released"),
        Err(error) => logl::log(
            level::ERROR,
            format_args!("hello_world: cursor release failed error={error}"),
        ),
    }
}

fn start_orbit(cursor: &VCursor, center: (i32, i32), orbit: Orbit) -> Result<(), i32> {
    let window_center = orbit_position(center, orbit.start, orbit.radius_multiple);
    cursor.submit(stroke(
        window_center.0,
        window_center.1,
        240,
        MOUSE_MOTION_EASING_FAST_LINEAR,
        MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    ))?;
    queue_orbit(
        cursor,
        center,
        orbit.start,
        orbit.radius_multiple,
        orbit.clockwise,
    )
}

fn queue_orbit(
    cursor: &VCursor,
    center: (i32, i32),
    start: usize,
    radius_multiple: i32,
    clockwise: bool,
) -> Result<(), i32> {
    let window_center = orbit_position(center, start, radius_multiple);

    // Selection is deliberately a complete primary gesture. UI4 absorbs it,
    // then the separately clocked secondary gesture owns the window drag.
    cursor.submit(buttons(PRIMARY_BUTTON, 0))?;
    cursor.submit(stroke(
        window_center.0,
        window_center.1,
        72,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;
    cursor.submit(buttons(0, PRIMARY_BUTTON))?;
    cursor.submit(stroke(
        window_center.0,
        window_center.1,
        96,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;

    // Radius zero is the retained center reference: select it, but do not
    // start a drag program.
    if radius_multiple == 0 {
        return Ok(());
    }

    cursor.submit(buttons(SECONDARY_BUTTON, 0))?;
    let radial = scaled_orbit_point(start, radius_multiple);
    cursor.submit(stroke(
        window_center.0,
        window_center.1,
        240,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;

    let direction = if clockwise { 1isize } else { -1isize };
    let mut from = radial;
    for step in 1..=ORBIT_POINTS.len() {
        let point_index = (start as isize + direction * step as isize)
            .rem_euclid(ORBIT_POINTS.len() as isize) as usize;
        let to = scaled_orbit_point(point_index, radius_multiple);
        cursor.submit(orbit_stroke(center, from, to, clockwise))?;
        from = to;
    }
    cursor.submit(buttons(0, SECONDARY_BUTTON))
}

fn scaled_orbit_point(point_index: usize, radius_multiple: i32) -> (i32, i32) {
    let radial = ORBIT_POINTS[point_index];
    (radial.0 * radius_multiple, radial.1 * radius_multiple)
}

fn orbit_position(center: (i32, i32), point_index: usize, radius_multiple: i32) -> (i32, i32) {
    let radial = scaled_orbit_point(point_index, radius_multiple);
    (center.0 + radial.0, center.1 + radial.1)
}

fn stroke(x: i32, y: i32, duration_ms: u32, easing: u8, flags: u8) -> MouseMotionCommand {
    MouseMotionCommand {
        opcode: MOUSE_MOTION_OPCODE_STROKE,
        path: MOUSE_MOTION_PATH_LINE,
        easing,
        flags,
        duration_ms,
        x,
        y,
        ..MouseMotionCommand::default()
    }
}

fn orbit_stroke(
    center: (i32, i32),
    from: (i32, i32),
    to: (i32, i32),
    clockwise: bool,
) -> MouseMotionCommand {
    let direction = if clockwise { 1 } else { -1 };
    let tangent = |point: (i32, i32)| {
        (
            direction * -point.1 * ORBIT_CONTROL_PERCENT / 100,
            direction * point.0 * ORBIT_CONTROL_PERCENT / 100,
        )
    };
    let from_tangent = tangent(from);
    let to_tangent = tangent(to);
    MouseMotionCommand {
        opcode: MOUSE_MOTION_OPCODE_STROKE,
        path: MOUSE_MOTION_PATH_CUBIC,
        easing: MOUSE_MOTION_EASING_NATURAL,
        duration_ms: 110,
        x: center.0 + to.0,
        y: center.1 + to.1,
        control1_x: center.0 + from.0 + from_tangent.0,
        control1_y: center.1 + from.1 + from_tangent.1,
        control2_x: center.0 + to.0 - to_tangent.0,
        control2_y: center.1 + to.1 - to_tangent.1,
        ..MouseMotionCommand::default()
    }
}

fn buttons(set: u32, clear: u32) -> MouseMotionCommand {
    MouseMotionCommand {
        opcode: MOUSE_MOTION_OPCODE_BUTTONS,
        buttons_set: set,
        buttons_clear: clear,
        ..MouseMotionCommand::default()
    }
}
