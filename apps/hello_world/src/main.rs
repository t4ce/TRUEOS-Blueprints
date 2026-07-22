#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use trueos::hid::{
    MOUSE_MOTION_EASING_FAST_LINEAR, MOUSE_MOTION_EASING_NATURAL, MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    MOUSE_MOTION_OPCODE_BUTTONS, MOUSE_MOTION_OPCODE_STROKE, MOUSE_MOTION_PATH_LINE,
    MOUSE_MOTION_PATH_QUADRATIC, MouseMotionCommand, VCursor,
};
use trueos::logl::{self, level};
use trueos::ui4_scene::{CursorIcon, CursorSource, Damage, Frame, rgba};
use trueos::ui4_solara_text::{Font, SceneTextRow};
use trueos::vsys;

const FRAME_X: i32 = 420;
const FRAME_Y: i32 = 220;
const FRAME_WIDTH: u32 = 440;
const FRAME_HEIGHT: u32 = 240;
const SECONDARY_BUTTON: u32 = 1 << 1;

const CURSOR_LABELS: [&str; 5] = [
    "hello-leader",
    "hello-dancer-a",
    "hello-dancer-b",
    "hello-scout-a",
    "hello-scout-b",
];
const CURSOR_ICONS: [CursorIcon; 5] = [
    CursorIcon::Default,
    CursorIcon::Loading,
    CursorIcon::ResizeHorizontal,
    CursorIcon::ResizeVertical,
    CursorIcon::ResizeDiagonal,
];

const CIRCLE: [(i32, i32); 9] = [
    (0, -18),
    (13, -13),
    (18, 0),
    (13, 13),
    (0, 18),
    (-13, 13),
    (-18, 0),
    (-13, -13),
    (0, 0),
];

fn main() {
    logl::log(
        level::INFO,
        "hello_world: opening UI4 five-cursor automation demo",
    );
    let Ok(mut frame) = Frame::open(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(level::ERROR, "hello_world: UI4 frame open failed");
        return;
    };
    if let Err(error) = present_hello(&mut frame) {
        logl::log(
            level::ERROR,
            format_args!("hello_world: initial frame publish failed: {error:?}"),
        );
        return;
    }

    let mut cursors = Vec::with_capacity(CURSOR_LABELS.len());
    for (label, icon) in CURSOR_LABELS.into_iter().zip(CURSOR_ICONS) {
        let Ok(cursor) = VCursor::request(label) else {
            logl::log(
                level::ERROR,
                format_args!("hello_world: virtual cursor request failed label={label}"),
            );
            return;
        };
        let source = CursorSource {
            controller_id: 0,
            slot_id: cursor.slot_id(),
            ep_target: 0,
            hid_kind: 0,
        };
        if let Err(error) = frame.set_cursor_icon_for(source, icon) {
            logl::log(
                level::ERROR,
                format_args!(
                    "hello_world: cursor override failed label={label} slot={} error={error:?}",
                    cursor.slot_id(),
                ),
            );
            return;
        }
        cursors.push(cursor);
    }

    // Cursor capabilities allocate at screen center but remain visually quiet
    // until their first program command. This makes the one-second pause part
    // of the visible Hello World sequence rather than startup latency.
    vsys::sleep_ms(1_000);

    if let Err(error) = queue_demo(cursors.as_slice()) {
        logl::log(
            level::ERROR,
            format_args!("hello_world: cursor program rejected error={error}"),
        );
        return;
    }
    logl::log(
        level::INFO,
        "hello_world: five AI cursor programs queued; leader selects then right-drags the frame",
    );

    loop {
        // The demo is an input target as well as an automation producer. Drain
        // its selected-frame events so a long cursor dance never fills the
        // per-owner input queue.
        while matches!(frame.take_pointer_event(), Ok(Some(_))) {}
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

fn present_hello(frame: &mut Frame) -> Result<(), trueos::ui4_scene::Error> {
    const ROWS: [SceneTextRow<'static>; 4] = [
        SceneTextRow {
            text: "HELLO, UI4",
            x: 38.0,
            y: 42.0,
            font_pixels: 34.0,
        },
        SceneTextRow {
            text: "five mediated AI cursors",
            x: 40.0,
            y: 94.0,
            font_pixels: 22.0,
        },
        SceneTextRow {
            text: "one selects + right-drags this frame",
            x: 40.0,
            y: 136.0,
            font_pixels: 18.0,
        },
        SceneTextRow {
            text: "the others dance along the top",
            x: 40.0,
            y: 170.0,
            font_pixels: 18.0,
        },
    ];
    frame.begin(rgba(10, 16, 28, 255))?;
    frame.draw_text_scene(
        Font::Default,
        (FRAME_WIDTH, FRAME_HEIGHT),
        rgba(126, 224, 255, 255),
        &ROWS,
    )?;
    frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT))
}

fn queue_demo(cursors: &[VCursor]) -> Result<(), i32> {
    if cursors.len() != 5 {
        return Err(-1);
    }
    queue_leader(&cursors[0])?;
    queue_dancer(&cursors[1], FRAME_X + 150, FRAME_Y + 30, false)?;
    queue_dancer(&cursors[2], FRAME_X + 285, FRAME_Y + 30, true)?;
    queue_scout(&cursors[3], FRAME_X + 70, FRAME_Y + 42, -1)?;
    queue_scout(
        &cursors[4],
        FRAME_X + FRAME_WIDTH as i32 - 70,
        FRAME_Y + 42,
        1,
    )
}

fn queue_leader(cursor: &VCursor) -> Result<(), i32> {
    let target = (FRAME_X + 86, FRAME_Y + 92);
    cursor.submit(stroke(
        target.0,
        target.1,
        320,
        MOUSE_MOTION_EASING_FAST_LINEAR,
        MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    ))?;

    // The first right-click selects the frame and is intentionally absorbed by
    // UI4. Short stationary strokes make down/up observable as a real gesture.
    cursor.submit(buttons(SECONDARY_BUTTON, 0))?;
    cursor.submit(stroke(
        target.0,
        target.1,
        72,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;
    cursor.submit(buttons(0, SECONDARY_BUTTON))?;
    cursor.submit(stroke(
        target.0,
        target.1,
        96,
        MOUSE_MOTION_EASING_NATURAL,
        0,
    ))?;

    // The second right-button gesture is delivered to the already-selected
    // frame. The closed path moves the frame in a tiny circle and returns it.
    cursor.submit(buttons(SECONDARY_BUTTON, 0))?;
    for (dx, dy) in CIRCLE {
        cursor.submit(curved_stroke(target.0 + dx, target.1 + dy, 140))?;
    }
    cursor.submit(buttons(0, SECONDARY_BUTTON))
}

fn queue_dancer(cursor: &VCursor, base_x: i32, base_y: i32, mirror: bool) -> Result<(), i32> {
    cursor.submit(stroke(
        base_x,
        base_y,
        360,
        MOUSE_MOTION_EASING_FAST_LINEAR,
        MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    ))?;
    cursor.submit(stroke(base_x, base_y, 290, MOUSE_MOTION_EASING_NATURAL, 0))?;
    for (index, (dx, dy)) in CIRCLE.into_iter().enumerate() {
        let wiggle = if index.is_multiple_of(2) { 11 } else { -11 };
        let (dance_x, dance_y) = if mirror {
            (base_x + dx + wiggle, base_y + dy - wiggle / 2)
        } else {
            (base_x + dx - wiggle, base_y + dy + wiggle / 2)
        };
        cursor.submit(curved_stroke(dance_x, dance_y, 140))?;
    }
    Ok(())
}

fn queue_scout(cursor: &VCursor, base_x: i32, base_y: i32, direction: i32) -> Result<(), i32> {
    cursor.submit(stroke(
        base_x,
        base_y,
        430,
        MOUSE_MOTION_EASING_FAST_LINEAR,
        MOUSE_MOTION_FLAG_CLEAR_QUEUE,
    ))?;
    cursor.submit(stroke(base_x, base_y, 220, MOUSE_MOTION_EASING_NATURAL, 0))?;
    for step in 0..8i32 {
        let side = if step % 2 == 0 { 1 } else { -1 };
        cursor.submit(curved_stroke(
            base_x + direction * (18 + side * 8),
            base_y + side * 10,
            170,
        ))?;
    }
    cursor.submit(curved_stroke(base_x, base_y, 170))
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

fn curved_stroke(x: i32, y: i32, duration_ms: u32) -> MouseMotionCommand {
    MouseMotionCommand {
        opcode: MOUSE_MOTION_OPCODE_STROKE,
        path: MOUSE_MOTION_PATH_QUADRATIC,
        easing: MOUSE_MOTION_EASING_NATURAL,
        duration_ms,
        x,
        y,
        control1_x: x,
        control1_y: y - 9,
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
