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
const FRAME_WIDTH: u32 = 96;
const FRAME_HEIGHT: u32 = 64;
const ORBIT_RADIUS: i32 = 100;
const PRIMARY_BUTTON: u32 = 1 << 0;
const SECONDARY_BUTTON: u32 = 1 << 1;

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
    (100, 0),
    (92, 38),
    (71, 71),
    (38, 92),
    (0, 100),
    (-38, 92),
    (-71, 71),
    (-92, 38),
    (-100, 0),
    (-92, -38),
    (-71, -71),
    (-38, -92),
    (0, -100),
    (38, -92),
    (71, -71),
    (92, -38),
];

const START_POINT: [usize; WINDOW_COUNT] = [0, 3, 6, 10, 13];
const CLOCKWISE: [bool; WINDOW_COUNT] = [true, false, true, false, true];

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
    let frame_origin = (
        center.0 - FRAME_WIDTH as i32 / 2,
        center.1 - FRAME_HEIGHT as i32 / 2,
    );
    logl::log(
        level::INFO,
        format_args!(
            "hello_world: five-window orbit demo output={}x{} center={},{} radius={ORBIT_RADIUS}",
            output_width, output_height, center.0, center.1,
        ),
    );

    let mut frames = Vec::with_capacity(WINDOW_COUNT);
    let mut cursors = Vec::with_capacity(WINDOW_COUNT);
    for index in 0..WINDOW_COUNT {
        let mut frame = match open_frame(index, frame_origin) {
            Ok(frame) => frame,
            Err(()) => return,
        };
        if wait_for_first_presentation(&mut frame, index).is_err() {
            return;
        }
        frames.push(frame);

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
        logl::log(
            level::INFO,
            format_args!(
                "hello_world: pair ready index={index} window={} cursor_handle={} cursor_slot={} direction={}",
                frames[index].window_id(),
                cursor.handle(),
                cursor.slot_id(),
                if CLOCKWISE[index] {
                    "clockwise"
                } else {
                    "counter-clockwise"
                },
            ),
        );

        if let Err(error) = queue_orbit(&cursor, center, START_POINT[index], CLOCKWISE[index]) {
            logl::log(
                level::ERROR,
                format_args!("hello_world: orbit program rejected index={index} error={error}"),
            );
            return;
        }
        if let Err(error) = wait_for_cursor(&cursor, frames.as_mut_slice()) {
            logl::log(
                level::ERROR,
                format_args!("hello_world: orbit wait failed index={index} error={error}"),
            );
            return;
        }
        cursors.push(cursor);
    }

    logl::log(
        level::INFO,
        "hello_world: five windows orbited from screen center; all frames and cursors retained",
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

fn wait_for_first_presentation(frame: &mut Frame, index: usize) -> Result<(), ()> {
    loop {
        match frame.take_first_presentation() {
            Ok(true) => return Ok(()),
            Ok(false) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
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
}

fn queue_orbit(
    cursor: &VCursor,
    center: (i32, i32),
    start: usize,
    clockwise: bool,
) -> Result<(), i32> {
    cursor.submit(stroke(
        center.0,
        center.1,
        48,
        MOUSE_MOTION_EASING_FAST_LINEAR,
        MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    ))?;

    // Selection is deliberately a complete primary gesture. UI4 absorbs it,
    // then the separately clocked secondary gesture owns the frame drag.
    cursor.submit(buttons(PRIMARY_BUTTON, 0))?;
    cursor.submit(stroke(
        center.0,
        center.1,
        72,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;
    cursor.submit(buttons(0, PRIMARY_BUTTON))?;
    cursor.submit(stroke(
        center.0,
        center.1,
        96,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;

    cursor.submit(buttons(SECONDARY_BUTTON, 0))?;
    let radial = ORBIT_POINTS[start];
    cursor.submit(stroke(
        center.0 + radial.0,
        center.1 + radial.1,
        240,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;

    let direction = if clockwise { 1isize } else { -1isize };
    let mut from = radial;
    for step in 1..=ORBIT_POINTS.len() {
        let point_index = (start as isize + direction * step as isize)
            .rem_euclid(ORBIT_POINTS.len() as isize) as usize;
        let to = ORBIT_POINTS[point_index];
        cursor.submit(orbit_stroke(center, from, to, clockwise))?;
        from = to;
    }
    cursor.submit(buttons(0, SECONDARY_BUTTON))
}

fn wait_for_cursor(cursor: &VCursor, frames: &mut [Frame]) -> Result<(), i32> {
    loop {
        drain_pointer_events(frames);
        match cursor.idle()? {
            true => return Ok(()),
            false => {
                vsys::poll_once();
                vsys::sleep_ms(8);
            }
        }
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
            direction * -point.1 * 13 / ORBIT_RADIUS,
            direction * point.0 * 13 / ORBIT_RADIUS,
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
