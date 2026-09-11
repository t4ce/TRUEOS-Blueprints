// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

mod convert;
mod gallery;
mod view;
use gallery::{Gallery, next_index};
use view::{Alignment, View, contained_extent};

use alloc::{format, string::String, vec, vec::Vec};
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, output_dimensions, rgba};
use trueos::{async_fs, image_source, input, replication, vmedia, vsys};

const MAX_SOURCE_PIXELS: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 32;
const CHECKPOINT_VERSION: u64 = 1;
const RESUME_FRAME_CADENCE_MS: u64 = 150;

struct Image {
    format: Option<convert::Format>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct OpenFrame {
    frame: Frame,
    view: View,
    image: Image,
    source: String,
    alignment: Alignment,
    hit_testable: bool,
    gallery: Option<Gallery>,
}

/// Everything valuable about an open image, without any old-kernel UI4
/// capability. Decoded pixels make restore independent of TRUEOSFS and media
/// decoder readiness.
struct SuspendedFrame {
    view: View,
    image: Image,
    source: String,
    alignment: Alignment,
    hit_testable: bool,
    gallery: Option<Gallery>,
}

fn main() {
    let mut frames = Vec::new();
    let args: Vec<String> = trueos::env::args()
        .skip(1)
        .filter(|argument| argument != "--vmx-minishell")
        .collect();

    // Runscripts remain a distinct, one-shot input surface.  They are applied
    // before argv so startup.json can still arrange several kernel images.
    if let Ok(bytes) = async_fs::block_on(async_fs::read_file(b"vFile:launch"))
        && let Ok(script) = String::from_utf8(bytes)
    {
        for line in script.lines() {
            run_line(line, &mut frames);
        }
    }

    if !args.is_empty() {
        if args.len() > 1 && args[1..].iter().all(|argument| is_show_option(argument)) {
            let mut command = String::from("show ");
            command.push_str(args.join(" ").as_str());
            run_line(command.as_str(), &mut frames);
        } else {
            // Each argv item is an independent source.  This is the contract
            // used by termdir's multi-file Show action.
            for path in args {
                show_source(path.as_str(), Alignment::Center, true, &mut frames);
            }
        }
    }

    if frames.is_empty() {
        open_default_frame(&mut frames);
    }

    let mut command = Vec::new();

    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            prepare_pause(prepare, &mut frames);
            continue;
        }
        service_frames(&mut frames);
        if service_command_channel(&mut command, &mut frames) {
            break;
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
    let _ = trueos::vshell::shutdown_current_blueprint("img viewer exited");
}

fn terminal_line(text: &str) {
    let _ = trueos::vshell::line(text);
}

fn service_command_channel(command: &mut Vec<u8>, frames: &mut Vec<OpenFrame>) -> bool {
    let mut bytes = [0u8; 512];
    let len = trueos::vshell::attached_read_available(&mut bytes);
    for byte in &bytes[..len] {
        match *byte {
            3 => {
                command.clear();
            }
            b'\r' | b'\n' => {
                let line = core::mem::take(command);
                let line = String::from_utf8(line).unwrap_or_default();
                match line.trim() {
                    "" => {}
                    "help" => terminal_line(
                        "img: list | show PATH [alignment] [hit|nohit] | convert | close all | exit",
                    ),
                    "list" => list_frames(frames),
                    "convert" => convert_frame(frames),
                    "close all" | "clear" => {
                        frames.clear();
                        terminal_line("img: all frames closed");
                    }
                    "exit" | "quit" => {
                        return true;
                    }
                    line if line.starts_with("show ") => run_line(line, frames),
                    _ => terminal_line("img: unknown command; use `help`"),
                }
            }
            8 | 127 if !command.is_empty() => {
                command.pop();
            }
            byte if byte >= 0x20 => {
                command.push(byte);
            }
            _ => {}
        }
    }
    false
}

fn convert_frame(frames: &[OpenFrame]) {
    let selection: Result<Vec<bool>, Ui4Error> = frames
        .iter()
        .map(|open| {
            open.frame
                .input_routes()
                .map(|routes| routes.iter().any(|route| route.selected_for_window))
        })
        .collect();
    let Ok(selection) = selection else {
        return;
    };
    let Some(index) = convert::selected_index(selection.into_iter()) else {
        return;
    };
    let open = &frames[index];
    let Some((path, format)) = convert::target(&open.source, open.image.format) else {
        return;
    };
    let started = trueos::clock::monotonic_millis();
    let result = convert::encode(
        format,
        open.image.width,
        open.image.height,
        &open.image.rgba,
    )
    .and_then(|bytes| {
        let content_type = match format {
            convert::Format::Png => async_fs::ContentTypeId::PNG,
            convert::Format::Jpeg => async_fs::ContentTypeId::JPEG,
        };
        async_fs::block_on(async_fs::write_file_typed(
            path.as_bytes(),
            &bytes,
            content_type,
        ))
        .map_err(|code| format!("trueosfs write code={code}"))?;
        Ok(bytes.len())
    });
    match result {
        Ok(bytes) => terminal_line(format!(
            "img: convert source={} output={path} format={format:?} bytes={bytes} size={}x{} convert_ms={}",
            open.source, open.image.width, open.image.height,
            trueos::clock::monotonic_millis().saturating_sub(started),
        ).as_str()),
        Err(error) => terminal_line(format!("img: convert {path}: {error}").as_str()),
    }
}

fn list_frames(frames: &[OpenFrame]) {
    terminal_line(format!("img: {} of {} frames", frames.len(), MAX_FRAMES).as_str());
    for (index, open) in frames.iter().enumerate() {
        let mode = if let Some(gallery) = &open.gallery {
            format!("gallery {}/{}", gallery.index + 1, gallery.paths.len())
        } else {
            String::from("fixed")
        };
        terminal_line(
            format!(
                "  {}: window={} {} {}x{} {}",
                index + 1,
                open.frame.window_id(),
                open.source,
                open.image.width,
                open.image.height,
                mode,
            )
            .as_str(),
        );
    }
}

fn is_show_option(value: &str) -> bool {
    matches!(
        value,
        "center" | "top-left" | "top-right" | "bottom-left" | "bottom-right" | "hit" | "nohit"
    )
}

fn run_line(line: &str, frames: &mut Vec<OpenFrame>) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line == "fs-scope trueosfs" {
        return;
    }
    let Some(command) = line.strip_prefix("show ") else {
        logl::log(level::WARN, format_args!("img: ignored command={line}"));
        return;
    };
    let mut words = command.split_ascii_whitespace();
    let Some(path) = words.next() else {
        logl::log(level::WARN, format_args!("img: show requires a source"));
        return;
    };
    let mut alignment = Alignment::Center;
    let mut hit_testable = true;
    for option in words {
        match option {
            "center" => alignment = Alignment::Center,
            "top-left" => alignment = Alignment::TopLeft,
            "top-right" => alignment = Alignment::TopRight,
            "bottom-left" => alignment = Alignment::BottomLeft,
            "bottom-right" => alignment = Alignment::BottomRight,
            "nohit" => hit_testable = false,
            "hit" => hit_testable = true,
            _ => {
                logl::log(
                    level::WARN,
                    format_args!("img: unknown show option={option}"),
                );
                return;
            }
        }
    }
    show_source(path, alignment, hit_testable, frames);
}

fn show_source(path: &str, alignment: Alignment, hit_testable: bool, frames: &mut Vec<OpenFrame>) {
    if frames.len() >= MAX_FRAMES {
        logl::log(
            level::WARN,
            format_args!("img: frame cap={} source={path}", MAX_FRAMES),
        );
        return;
    }
    let started = trueos::clock::monotonic_millis();
    let gallery = if !path.starts_with("kernel:")
        && async_fs::block_on(async_fs::metadata(path.as_bytes())).is_ok_and(|meta| meta.is_dir())
    {
        match list_gallery(path) {
            Ok(gallery) => Some(gallery),
            Err(error) => {
                terminal_line(format!("img: {error}").as_str());
                return;
            }
        }
    } else {
        None
    };
    let source_path = gallery
        .as_ref()
        .map_or(path, |gallery| gallery.paths[0].as_str());
    let image = match load_image(source_path) {
        Ok(image) => image,
        Err(error) => {
            terminal_line(format!("img: {path}: {error}").as_str());
            logl::log(
                level::ERROR,
                format_args!("img: show source={path} error={error}"),
            );
            return;
        }
    };
    let source = String::from(source_path);
    match open_decoded_image(
        source,
        image,
        alignment,
        hit_testable,
        None,
        gallery.is_some(),
    ) {
        Ok(mut open) => {
            open.gallery = gallery;
            terminal_line(
                format!(
                    "img: {} load_to_publish_ms={}",
                    open.source,
                    trueos::clock::monotonic_millis().saturating_sub(started)
                )
                .as_str(),
            );
            logl::log(
                level::INFO,
                format_args!(
                    "img: show source={} size={}x{} viewport={}x{} window={} align={alignment:?} hit={} native=1 frames={}/{}",
                    open.source,
                    open.image.width,
                    open.image.height,
                    open.view.viewport_width,
                    open.view.viewport_height,
                    open.frame.window_id(),
                    hit_testable as u8,
                    frames.len() + 1,
                    MAX_FRAMES,
                ),
            );
            frames.push(open);
        }
        Err((_suspended, error)) => logl::log(
            level::ERROR,
            format_args!("img: show source={path} error={error}"),
        ),
    }
}

fn open_default_frame(frames: &mut Vec<OpenFrame>) {
    const DEFAULT_GALLERY: &str = "apps/common/images";
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    let image = Image {
        format: None,
        width: WIDTH,
        height: HEIGHT,
        rgba: vec![0x78; WIDTH as usize * HEIGHT as usize * 4],
    };
    // The placeholder is deliberately opaque neutral gray.
    let mut image = image;
    for alpha in image.rgba.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }
    if let Ok(open) = open_decoded_image(
        String::from("<empty>"),
        image,
        Alignment::Center,
        true,
        None,
        false,
    ) {
        frames.push(open);
    }

    let (Some(open), Ok(gallery)) = (frames.first_mut(), list_gallery(DEFAULT_GALLERY)) else {
        return;
    };
    let Some(source) = gallery.paths.first().cloned() else {
        return;
    };
    let Ok(image) = load_image(source.as_str()) else {
        return;
    };
    let mut view = View::new(
        open.frame.width(),
        open.frame.height(),
        image.width,
        image.height,
        Alignment::Center,
    );
    view.letterbox = true;
    if present(&mut open.frame, view, &image).is_ok() {
        open.view = view;
        open.image = image;
        open.source = source;
        open.gallery = Some(gallery);
    }
}

fn open_decoded_image(
    source: String,
    image: Image,
    alignment: Alignment,
    hit_testable: bool,
    restored_view: Option<View>,
    fit: bool,
) -> Result<OpenFrame, (SuspendedFrame, String)> {
    let suspended = |view: View, image: Image, error: String| {
        (
            SuspendedFrame {
                view,
                image,
                source: source.clone(),
                alignment,
                hit_testable,
                gallery: None,
            },
            error,
        )
    };
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    // The embedded desktop image uses the ordinary viewer, with a snug fitted
    // frame. Space outside that frame belongs to the display bottom color.
    let logo = source == "kernel:logo";
    let (viewport_width, viewport_height) = if logo {
        let (width, height) = contained_extent(
            image.width as usize,
            image.height as usize,
            output_width.max(1) as usize,
            output_height.max(1) as usize,
        );
        (width as u32, height as u32)
    } else {
        (
            image.width.min(output_width).max(1),
            image.height.min(output_height).max(1),
        )
    };
    let mut view = restored_view.unwrap_or_else(|| {
        View::new(
            viewport_width,
            viewport_height,
            image.width,
            image.height,
            alignment,
        )
    });
    if (fit || logo) && !view.zoomed {
        view.fit_on_open = true;
        view.letterbox = true;
    }
    // A different post-update output mode must not resurrect an invalid
    // extent, but otherwise preserve the exact pan/resize projection.
    view.viewport_width = view.viewport_width.min(output_width).max(1);
    view.viewport_height = view.viewport_height.min(output_height).max(1);
    view.clamp_offsets();
    let (x, y) = aligned_position(
        output_width,
        output_height,
        view.viewport_width,
        view.viewport_height,
        alignment,
    );
    let mut frame = match Frame::open_immutable(x, y, view.viewport_width, view.viewport_height) {
        Ok(frame) => frame,
        Err(error) => {
            return Err(suspended(
                view,
                image,
                format!("viewport rejected error={error:?}"),
            ));
        }
    };
    if let Err(error) = frame.set_hit_testable(hit_testable) {
        return Err(suspended(view, image, format!("hit-test error={error:?}")));
    }
    if let Err(error) = present(&mut frame, view, &image) {
        return Err(suspended(view, image, format!("present error={error:?}")));
    }
    Ok(OpenFrame {
        frame,
        view,
        image,
        source,
        alignment,
        hit_testable,
        gallery: None,
    })
}

fn prepare_pause(prepare: replication::PreparePause, frames: &mut Vec<OpenFrame>) {
    let suspended: Vec<SuspendedFrame> = frames
        .drain(..)
        .map(|open| SuspendedFrame {
            view: open.view,
            image: open.image,
            source: open.source,
            alignment: open.alignment,
            hit_testable: open.hit_testable,
            gallery: open.gallery,
        })
        .collect();
    logl::log(
        level::INFO,
        format_args!(
            "img: PreparePause operation={} reason={:?}; released UI4 frames={} decoded-state=retained Ready",
            prepare.operation(),
            prepare.reason,
            suspended.len(),
        ),
    );

    let resume = replication::ready(prepare, CHECKPOINT_VERSION);
    match &resume {
        Ok(identity) => logl::log(
            level::INFO,
            format_args!(
                "img: Resume instance={} lineage={} generation={} clone={}; replaying frames cadence_ms={}",
                identity.instance_guid(),
                identity.lineage_guid(),
                identity.generation,
                identity.is_clone,
                RESUME_FRAME_CADENCE_MS,
            ),
        ),
        Err(error) => logl::log(
            level::WARN,
            format_args!(
                "img: Ready rejected error={error:?}; rebuilding released UI4 frames locally"
            ),
        ),
    }

    for (index, mut saved) in suspended.into_iter().enumerate() {
        let mut attempt = 0_u64;
        loop {
            attempt = attempt.saturating_add(1);
            if index != 0 || attempt != 1 {
                vsys::sleep_ms(RESUME_FRAME_CADENCE_MS);
            }
            let source = saved.source.clone();
            let gallery = saved.gallery.clone();
            match open_decoded_image(
                saved.source,
                saved.image,
                saved.alignment,
                saved.hit_testable,
                Some(saved.view),
                false,
            ) {
                Ok(mut open) => {
                    open.gallery = gallery;
                    logl::log(
                        level::INFO,
                        format_args!(
                            "img: replay source={} window={} viewport={}x{} attempt={attempt}",
                            open.source,
                            open.frame.window_id(),
                            open.view.viewport_width,
                            open.view.viewport_height,
                        ),
                    );
                    frames.push(open);
                    break;
                }
                Err((returned, error)) => {
                    saved = returned;
                    saved.gallery = gallery;
                    logl::log(
                        level::WARN,
                        format_args!(
                            "img: replay source={source} waiting for UI4 attempt={attempt} error={error}"
                        ),
                    );
                }
            }
        }
    }
}

fn aligned_position(
    output_width: u32,
    output_height: u32,
    width: u32,
    height: u32,
    alignment: Alignment,
) -> (i32, i32) {
    match alignment {
        Alignment::Center => (
            output_width.saturating_sub(width) as i32 / 2,
            output_height.saturating_sub(height) as i32 / 2,
        ),
        Alignment::TopLeft => (0, 0),
        Alignment::TopRight => (output_width.saturating_sub(width) as i32, 0),
        Alignment::BottomLeft => (0, output_height.saturating_sub(height) as i32),
        Alignment::BottomRight => (
            output_width.saturating_sub(width) as i32,
            output_height.saturating_sub(height) as i32,
        ),
    }
}

fn present(frame: &mut Frame, view: View, image: &Image) -> Result<(), Ui4Error> {
    if view.viewport_width == image.width
        && view.viewport_height == image.height
        && view.scale == 1.0
        && view.offset_x == 0.0
        && view.offset_y == 0.0
        && image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255)
    {
        frame.begin(rgba(0, 0, 0, 255))?;
        frame.write_opaque_rgba8(&image.rgba)?;
        return frame.publish(Damage::full(frame.width(), frame.height()));
    }
    let mut viewport = vec![0u8; view.viewport_width as usize * view.viewport_height as usize * 4];
    for alpha in viewport.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }

    if view.letterbox {
        paint_letterboxed(viewport.as_mut_slice(), view, image);
    } else if view.scale != 1.0 {
        for y in 0..view.viewport_height as usize {
            for x in 0..view.viewport_width as usize {
                if let Some((source_x, source_y)) = view.source_at(x, y) {
                    let source = (source_y * image.width as usize + source_x) * 4;
                    let destination = (y * view.viewport_width as usize + x) * 4;
                    viewport[destination..destination + 4]
                        .copy_from_slice(&image.rgba[source..source + 4]);
                }
            }
        }
    } else {
        let source_x = (-view.offset_x).max(0.0) as usize;
        let source_y = (-view.offset_y).max(0.0) as usize;
        let destination_x = view.offset_x.max(0.0) as usize;
        let destination_y = view.offset_y.max(0.0) as usize;
        let copy_width = (image.width as usize)
            .saturating_sub(source_x)
            .min(view.viewport_width as usize - destination_x);
        let copy_height = (image.height as usize)
            .saturating_sub(source_y)
            .min(view.viewport_height as usize - destination_y);
        for row in 0..copy_height {
            let source_start = ((source_y + row) * image.width as usize + source_x) * 4;
            let destination_start =
                ((destination_y + row) * view.viewport_width as usize + destination_x) * 4;
            let byte_len = copy_width * 4;
            viewport[destination_start..destination_start + byte_len]
                .copy_from_slice(&image.rgba[source_start..source_start + byte_len]);
        }
    }

    // PNG pixels are straight-alpha; the opaque viewer composites onto black.
    for pixel in viewport.chunks_exact_mut(4) {
        if pixel[3] != 255 {
            let alpha = pixel[3] as u16;
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u16 * alpha + 127) / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
    frame.begin(rgba(0, 0, 0, 255))?;
    frame.write_opaque_rgba8(viewport.as_slice())?;
    frame.publish(Damage::full(frame.width(), frame.height()))
}

fn paint_letterboxed(viewport: &mut [u8], view: View, image: &Image) {
    let viewport_width = view.viewport_width as usize;
    let viewport_height = view.viewport_height as usize;
    let image_width = image.width as usize;
    let image_height = image.height as usize;
    let (draw_width, draw_height) =
        contained_extent(image_width, image_height, viewport_width, viewport_height);
    let destination_x = (viewport_width - draw_width) / 2;
    let destination_y = (viewport_height - draw_height) / 2;

    for draw_y in 0..draw_height {
        let source_y = draw_y * image_height / draw_height;
        for draw_x in 0..draw_width {
            let source_x = draw_x * image_width / draw_width;
            let source = (source_y * image_width + source_x) * 4;
            let destination =
                ((destination_y + draw_y) * viewport_width + destination_x + draw_x) * 4;
            viewport[destination..destination + 4].copy_from_slice(&image.rgba[source..source + 4]);
        }
    }
}

fn service_frames(frames: &mut Vec<OpenFrame>) {
    let mut index = 0;
    while index < frames.len() {
        let mut close = false;
        let mut repaint = false;
        {
            let open = &mut frames[index];
            loop {
                match open.frame.take_keyboard_event() {
                    Ok(Some(event))
                        if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                            && event.key_code == input::KEYBOARD_KEY_ESCAPE
                            && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0 =>
                    {
                        close = true;
                    }
                    Ok(Some(event))
                        if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                            && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
                            && matches!(
                                event.key_code,
                                input::KEYBOARD_KEY_ARROW_LEFT
                                    | input::KEYBOARD_KEY_ARROW_RIGHT
                                    | input::KEYBOARD_KEY_ARROW_UP
                                    | input::KEYBOARD_KEY_ARROW_DOWN
                            ) =>
                    {
                        navigate(
                            open,
                            matches!(
                                event.key_code,
                                input::KEYBOARD_KEY_ARROW_RIGHT | input::KEYBOARD_KEY_ARROW_DOWN
                            ),
                        );
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(level::WARN, format_args!("img: keyboard error={error:?}"));
                        break;
                    }
                }
            }
            loop {
                match open.frame.take_pan_event() {
                    Ok(Some(event)) => {
                        open.view.pan(event.dx, event.dy);
                        repaint = true;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(level::WARN, format_args!("img: pan event error={error:?}"));
                        break;
                    }
                }
            }
            loop {
                match open.frame.take_pointer_event() {
                    Ok(Some(event)) => {
                        repaint |= open.view.zoom_at(event.wheel, event.local_x, event.local_y);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(
                            level::WARN,
                            format_args!("img: pointer event error={error:?}"),
                        );
                        break;
                    }
                }
            }
            let mut resize = None;
            loop {
                match open.frame.take_resize_event() {
                    Ok(Some(event)) => resize = Some(event),
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(
                            level::WARN,
                            format_args!("img: resize event error={error:?}"),
                        );
                        break;
                    }
                }
            }
            if let Some(event) = resize {
                match open.frame.resize(event.width, event.height) {
                    Ok(()) => {
                        open.view.resize(event.width, event.height);
                        if open.gallery.is_some() && !open.view.zoomed {
                            open.view.letterbox = true;
                        }
                        repaint = true;
                        logl::log(
                            level::INFO,
                            format_args!(
                                "img: resize window={} old={}x{} new={}x{} letterbox={}",
                                open.frame.window_id(),
                                event.old_width,
                                event.old_height,
                                event.width,
                                event.height,
                                open.view.letterbox as u8,
                            ),
                        );
                    }
                    Err(error) => logl::log(
                        level::WARN,
                        format_args!(
                            "img: resize window={} old={}x{} requested={}x{} error={error:?}",
                            open.frame.window_id(),
                            event.old_width,
                            event.old_height,
                            event.width,
                            event.height,
                        ),
                    ),
                }
            }
            if repaint && let Err(error) = present(&mut open.frame, open.view, &open.image) {
                logl::log(level::WARN, format_args!("img: repaint error={error:?}"));
            }
        }
        if close {
            let closed = frames.swap_remove(index);
            logl::log(
                level::INFO,
                format_args!(
                    "img: close window={} source=escape",
                    closed.frame.window_id()
                ),
            );
            drop(closed);
        } else {
            index += 1;
        }
    }
}

fn load_image(source: &str) -> Result<Image, String> {
    if source.starts_with("kernel:") {
        let (info, bytes) =
            image_source::read(source).map_err(|code| format!("kernel source code={code}"))?;
        return match info.format {
            image_source::FORMAT_JPEG => decode_jpeg(bytes.as_slice()),
            image_source::FORMAT_RGBA8 => image_from_rgba(info.width, info.height, bytes),
            image_source::FORMAT_PNG => decode_png(bytes.as_slice()),
            _ => Err(String::from("unsupported kernel image format")),
        };
    }
    let started = trueos::clock::monotonic_millis();
    let bytes = async_fs::block_on(async_fs::read_file(source.as_bytes()))
        .map_err(|code| format!("trueosfs read code={code}"))?;
    logl::log(
        level::INFO,
        format_args!(
            "img: read source={source} bytes={} read_ms={}",
            bytes.len(),
            trueos::clock::monotonic_millis().saturating_sub(started)
        ),
    );
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(&bytes)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        decode_jpeg(&bytes)
    } else {
        Err(String::from(
            "unsupported image signature (expected PNG or JPEG)",
        ))
    }
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    decode_media(vmedia::ImageFormat::Jpeg, bytes)
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    decode_media(vmedia::ImageFormat::Png, bytes)
}

fn decode_media(format: vmedia::ImageFormat, bytes: &[u8]) -> Result<Image, String> {
    let started = trueos::clock::monotonic_millis();
    let decoded = async_fs::block_on(vmedia::decode(format, bytes))
        .map_err(|code| format!("kernel {format:?} decode code={code}"))?;
    let expected = checked_rgba_len(decoded.info.width, decoded.info.height)
        .ok_or_else(|| String::from("image dimensions rejected"))?;
    if decoded.rgba.len() != expected {
        return Err(String::from("kernel decoded size mismatch"));
    }
    logl::log(
        level::INFO,
        format_args!(
            "img: decode format={format:?} bytes={} size={}x{} decode_ms={} backend={:?}",
            bytes.len(),
            decoded.info.width,
            decoded.info.height,
            trueos::clock::monotonic_millis().saturating_sub(started),
            decoded.info.backend,
        ),
    );
    Ok(Image {
        format: match format {
            vmedia::ImageFormat::Png => Some(convert::Format::Png),
            vmedia::ImageFormat::Jpeg => Some(convert::Format::Jpeg),
            _ => None,
        },
        width: decoded.info.width,
        height: decoded.info.height,
        rgba: decoded.rgba,
    })
}

fn image_from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Result<Image, String> {
    let expected =
        checked_rgba_len(width, height).ok_or_else(|| String::from("RGBA dimensions rejected"))?;
    if rgba.len() != expected {
        return Err(String::from("RGBA byte length mismatch"));
    }
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }
    Ok(Image {
        format: None,
        width,
        height,
        rgba,
    })
}

fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 || pixels > MAX_SOURCE_PIXELS {
        return None;
    }
    pixels.checked_mul(4)
}

fn list_gallery(path: &str) -> Result<Gallery, String> {
    let listing = async_fs::block_on(async_fs::list_dir_typed(path.as_bytes()))
        .map_err(|code| format!("folder read code={code}"))?;
    if listing.truncated {
        return Err(String::from("folder listing truncated"));
    }
    let mut paths: Vec<String> = listing
        .entries
        .into_iter()
        .filter(|entry| {
            entry.kind == async_fs::NodeKind::File
                && matches!(
                    entry.content_type,
                    async_fs::ContentTypeId::JPEG | async_fs::ContentTypeId::PNG
                )
        })
        .map(|entry| format!("{}/{}", path.trim_end_matches('/'), entry.name))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(String::from("folder contains no inferred PNG/JPEG images"));
    }
    Ok(Gallery { paths, index: 0 })
}

fn navigate(open: &mut OpenFrame, forward: bool) {
    let Some(gallery) = open.gallery.as_ref() else {
        return;
    };
    let count = gallery.paths.len();
    let Some(next) = next_index(gallery.index, count, forward) else {
        return;
    };
    let source = gallery.paths[next].clone();
    let started = trueos::clock::monotonic_millis();
    let image = match load_image(&source) {
        Ok(image) => image,
        Err(error) => {
            terminal_line(format!("img: {source}: {error}").as_str());
            return;
        }
    };
    let mut view = View::new(
        open.frame.width(),
        open.frame.height(),
        image.width,
        image.height,
        open.alignment,
    );
    view.letterbox = true;
    if let Err(error) = present(&mut open.frame, view, &image) {
        terminal_line(format!("img: present failed: {error:?}").as_str());
        return;
    }
    open.view = view;
    open.image = image;
    open.source = source;
    open.gallery.as_mut().unwrap().index = next;
    let elapsed = trueos::clock::monotonic_millis().saturating_sub(started);
    terminal_line(
        format!(
            "img: [{}/{}] {} load_to_publish_ms={}",
            next + 1,
            count,
            open.source,
            elapsed
        )
        .as_str(),
    );
    logl::log(
        level::INFO,
        format_args!(
            "img: navigate index={}/{} source={} load_to_publish_ms={elapsed}",
            next + 1,
            count,
            open.source
        ),
    );
}
