#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use trueos::hid::{
    MOUSE_MOTION_EASING_FAST_LINEAR, MOUSE_MOTION_EASING_NATURAL, MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    MOUSE_MOTION_OPCODE_BUTTONS, MOUSE_MOTION_OPCODE_STROKE, MOUSE_MOTION_PATH_CUBIC,
    MOUSE_MOTION_PATH_LINE, MouseMotionCommand, VCursor,
};
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Frame, output_dimensions, rgba};
use trueos::vsys;

const WINDOW_COUNT: usize = 5;
const FRAME_WIDTH: u32 = 480;
const FRAME_HEIGHT: u32 = 320;
const ORBIT_RADIUS: i32 = 300;
const PRIMARY_BUTTON: u32 = 1 << 0;
const SECONDARY_BUTTON: u32 = 1 << 1;
const ORBIT_CONTROL_PERCENT: i32 = 13;

const CURSOR_LABELS: [&str; WINDOW_COUNT] = [
    "hello-orbit-a",
    "hello-orbit-b",
    "hello-orbit-c",
    "hello-orbit-d",
    "hello-orbit-e",
];

const FRAME_COLORS: [u32; WINDOW_COUNT] = [
    rgba(8, 52, 28, 255),
    rgba(8, 44, 52, 255),
    rgba(20, 34, 68, 255),
    rgba(52, 28, 64, 255),
    rgba(72, 46, 12, 255),
];

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

const START_POINT: [usize; WINDOW_COUNT] = [0, 3, 6, 9, 15];
const CLOCKWISE: [bool; WINDOW_COUNT] = [true, false, true, false, true];

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
    logl::log(
        level::INFO,
        format_args!(
            "hello_world: five-window orbit demo output={}x{} center={},{} radius={ORBIT_RADIUS}",
            output_width, output_height, center.0, center.1,
        ),
    );

    let mut frames = Vec::with_capacity(WINDOW_COUNT);
    for index in 0..WINDOW_COUNT {
        let frame_center = orbit_position(center, START_POINT[index], index as i32);
        let frame_origin = (
            frame_center.0 - FRAME_WIDTH as i32 / 2,
            frame_center.1 - FRAME_HEIGHT as i32 / 2,
        );
        let frame = match open_frame(index, frame_origin) {
            Ok(frame) => frame,
            Err(()) => return,
        };
        frames.push(frame);
    }
    if wait_for_first_presentations(frames.as_mut_slice()).is_err() {
        return;
    }

    let mut cursors = Vec::with_capacity(WINDOW_COUNT);
    for index in 0..WINDOW_COUNT {
        let cursor = match VCursor::request(CURSOR_LABELS[index]) {
            Ok(cursor) => cursor,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "hello_world: cursor request failed index={index} label={} error={error}",
                        CURSOR_LABELS[index],
                    ),
                );
                return;
            }
        };
        cursors.push(cursor);
    }

    // Start every cursor's move from the shared spawn point to its own frame
    // center first. The selection/orbit programs are fully armed before those
    // synchronized lead-ins end.
    for (index, cursor) in cursors.iter().enumerate() {
        let frame_center = orbit_position(center, START_POINT[index], index as i32);
        if let Err(error) = cursor.submit(stroke(
            frame_center.0,
            frame_center.1,
            240,
            MOUSE_MOTION_EASING_FAST_LINEAR,
            MOUSE_MOTION_FLAG_CLEAR_QUEUE,
        )) {
            logl::log(
                level::ERROR,
                format_args!("hello_world: cursor sync failed index={index} error={error}"),
            );
            return;
        }
    }

    for (index, cursor) in cursors.iter().enumerate() {
        let radius_multiple = index as i32;
        let frame_center = orbit_position(center, START_POINT[index], radius_multiple);
        logl::log(
            level::INFO,
            format_args!(
                "hello_world: pair ready index={index} window={} cursor_handle={} cursor_slot={} frame_center={},{} radius={} direction={}",
                frames[index].window_id(),
                cursor.handle(),
                cursor.slot_id(),
                frame_center.0,
                frame_center.1,
                ORBIT_RADIUS * radius_multiple,
                if CLOCKWISE[index] {
                    "clockwise"
                } else {
                    "counter-clockwise"
                },
            ),
        );

        if let Err(error) = queue_orbit(
            cursor,
            center,
            START_POINT[index],
            radius_multiple,
            CLOCKWISE[index],
        ) {
            logl::log(
                level::ERROR,
                format_args!("hello_world: orbit program rejected index={index} error={error}"),
            );
            return;
        }
    }
    if let Err((index, error)) = wait_for_cursors(cursors.as_slice(), frames.as_mut_slice()) {
        logl::log(
            level::ERROR,
            format_args!("hello_world: orbit wait failed index={index} error={error}"),
        );
        return;
    }

    logl::log(
        level::INFO,
        "hello_world: five concurrent window orbits complete; all frames and cursors retained",
    );
    loop {
        drain_pointer_events(frames.as_mut_slice());
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

fn open_frame(index: usize, origin: (i32, i32)) -> Result<Frame, ()> {
    let mut frame =
        Frame::open_immutable(origin.0, origin.1, FRAME_WIDTH, FRAME_HEIGHT).map_err(|error| {
            logl::log(
                level::ERROR,
                format_args!("hello_world: frame open failed index={index} error={error:?}"),
            );
        })?;
    frame
        .begin(FRAME_COLORS[index])
        .and_then(|()| frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT)))
        .map_err(|error| {
            logl::log(
                level::ERROR,
                format_args!("hello_world: frame publish failed index={index} error={error:?}"),
            );
        })?;
    Ok(frame)
}

fn wait_for_first_presentations(frames: &mut [Frame]) -> Result<(), ()> {
    let mut presented = [false; WINDOW_COUNT];
    let mut presented_count = 0;
    while presented_count < frames.len() {
        for (index, frame) in frames.iter_mut().enumerate() {
            if presented[index] {
                continue;
            }
            match frame.take_first_presentation() {
                Ok(true) => {
                    presented[index] = true;
                    presented_count += 1;
                }
                Ok(false) => {}
                Err(error) => {
                    logl::log(
                        level::ERROR,
                        format_args!(
                            "hello_world: first presentation failed index={index} error={error:?}"
                        ),
                    );
                    return Err(());
                }
            }
        }
        if presented_count < frames.len() {
            vsys::poll_once();
            vsys::sleep_ms(1);
        }
    }
    Ok(())
}

fn queue_orbit(
    cursor: &VCursor,
    center: (i32, i32),
    start: usize,
    radius_multiple: i32,
    clockwise: bool,
) -> Result<(), i32> {
    let frame_center = orbit_position(center, start, radius_multiple);

    // Selection is deliberately a complete primary gesture. UI4 absorbs it,
    // then the separately clocked secondary gesture owns the frame drag.
    cursor.submit(buttons(PRIMARY_BUTTON, 0))?;
    cursor.submit(stroke(
        frame_center.0,
        frame_center.1,
        72,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;
    cursor.submit(buttons(0, PRIMARY_BUTTON))?;
    cursor.submit(stroke(
        frame_center.0,
        frame_center.1,
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
        frame_center.0,
        frame_center.1,
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

fn wait_for_cursors(cursors: &[VCursor], frames: &mut [Frame]) -> Result<(), (usize, i32)> {
    loop {
        drain_pointer_events(frames);
        let mut all_idle = true;
        for (index, cursor) in cursors.iter().enumerate() {
            match cursor.idle() {
                Ok(true) => {}
                Ok(false) => all_idle = false,
                Err(error) => return Err((index, error)),
            }
        }
        if all_idle {
            return Ok(());
        }
        vsys::poll_once();
        vsys::sleep_ms(8);
    }
}

fn drain_pointer_events(frames: &mut [Frame]) {
    for frame in frames {
        while matches!(frame.take_pointer_event(), Ok(Some(_))) {}
    }
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
