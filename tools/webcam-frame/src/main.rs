#[cfg(not(target_os = "linux"))]
compile_error!("webcam-frame only supports Ubuntu 26");

use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use jpeg_decoder::PixelFormat;
use softbuffer::{Context, Surface};
use v4l::buffer::Type;
use v4l::capability::Flags;
use v4l::format::FourCC;
use v4l::io::traits::CaptureStream;
use v4l::prelude::MmapStream;
use v4l::video::Capture;
use v4l::{Device, Format};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const WINDOW_WIDTH: u32 = 960;
const WINDOW_HEIGHT: u32 = 540;

#[derive(Clone, Copy, Debug)]
struct Resolution {
    width: u32,
    height: u32,
}

const HD: Resolution = Resolution {
    width: 1280,
    height: 720,
};
const FULL_HD: Resolution = Resolution {
    width: 1920,
    height: 1080,
};

#[derive(Clone, Debug)]
struct Camera {
    name: String,
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct Frame {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    generation: u64,
}

#[derive(Debug)]
enum UserEvent {
    Frame(Frame),
    CaptureError { message: String, generation: u64 },
}

struct App {
    cameras: Vec<Camera>,
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    context: Option<Context<Arc<Window>>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    frame: Option<Frame>,
    stop_capture: Option<Arc<AtomicBool>>,
    generation: u64,
    active_camera: Option<usize>,
    capture_resolution: Resolution,
}

impl App {
    fn new(
        cameras: Vec<Camera>,
        proxy: EventLoopProxy<UserEvent>,
        capture_resolution: Resolution,
    ) -> Self {
        Self {
            cameras,
            proxy,
            window: None,
            context: None,
            surface: None,
            frame: None,
            stop_capture: None,
            generation: 0,
            active_camera: None,
            capture_resolution,
        }
    }

    fn select_camera(&mut self, index: usize) {
        let Some(camera) = self.cameras.get(index).cloned() else {
            eprintln!("No camera assigned to key {}", index + 1);
            return;
        };
        if self.active_camera == Some(index) {
            return;
        }

        if let Some(stop) = self.stop_capture.take() {
            stop.store(true, Ordering::Relaxed);
        }

        self.generation += 1;
        let generation = self.generation;
        let stop = Arc::new(AtomicBool::new(false));
        self.stop_capture = Some(stop.clone());
        self.active_camera = Some(index);
        self.frame = None;

        eprintln!(
            "Camera {}: {} ({})",
            index + 1,
            camera.name,
            camera.path.display()
        );
        spawn_capture(
            camera,
            generation,
            stop,
            self.proxy.clone(),
            self.capture_resolution,
        );
    }

    fn redraw(&mut self) {
        let (Some(window), Some(surface)) = (self.window.as_ref(), self.surface.as_mut()) else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(error) = surface.resize(width, height) {
            eprintln!("Could not resize display buffer: {error}");
            return;
        }
        let Ok(mut pixels) = surface.buffer_mut() else {
            return;
        };
        pixels.fill(0);

        if let Some(frame) = &self.frame {
            draw_scaled(&mut pixels, size.width, size.height, frame);
        }

        window.pre_present_notify();
        if let Err(error) = pixels.present() {
            eprintln!("Could not display frame: {error}");
        }
    }

    fn take_picture(&self) {
        let Some(frame) = self.frame.clone() else {
            eprintln!("No camera frame available yet");
            return;
        };
        let Some(camera_index) = self.active_camera else {
            eprintln!("No camera selected");
            return;
        };
        thread::spawn(move || match save_png(&frame, camera_index) {
            Ok(path) => eprintln!("Saved {}", path.display()),
            Err(error) => eprintln!("Could not save picture: {error}"),
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop_capture {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("webcam-frame")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("could not create Wayland window"),
        );
        let context = Context::new(window.clone()).expect("could not create display context");
        let surface =
            Surface::new(&context, window.clone()).expect("could not create display surface");

        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        if !self.cameras.is_empty() {
            self.select_camera(0);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Frame(frame) if frame.generation == self.generation => {
                self.frame = Some(frame);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::CaptureError {
                message,
                generation,
            } if generation == self.generation => eprintln!("Camera error: {message}"),
            _ => {}
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(window) = &self.window
                    && let Err(error) = window.drag_window()
                {
                    eprintln!("Could not drag window: {error}");
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => match physical_key {
                PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                PhysicalKey::Code(KeyCode::Space) => self.take_picture(),
                PhysicalKey::Code(code) => {
                    if let Some(index) = number_key(code) {
                        self.select_camera(index);
                    }
                }
                PhysicalKey::Unidentified(_) => {}
            },
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ensure_ubuntu_26()?;
    let capture_resolution = requested_resolution()?;
    let cameras = list_cameras();
    if cameras.is_empty() {
        eprintln!("No V4L2 webcams found. The window will remain black.");
    } else {
        eprintln!("Webcams:");
        for (index, camera) in cameras.iter().enumerate() {
            let key = if index < 5 {
                (index + 1).to_string()
            } else {
                "-".to_owned()
            };
            eprintln!("  {key}: {} ({})", camera.name, camera.path.display());
        }
        eprintln!(
            "Requested resolution: {}x{}",
            capture_resolution.width, capture_resolution.height
        );
        eprintln!("Space: picture  |  mouse: drag  |  Esc: close");
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut app = App::new(cameras, event_loop.create_proxy(), capture_resolution);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn requested_resolution() -> io::Result<Resolution> {
    let mut resolution = HD;
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--1080p" => resolution = FULL_HD,
            "--720p" => resolution = HD,
            "-h" | "--help" => {
                println!("webcam-frame [--720p | --1080p]");
                std::process::exit(0);
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown option: {argument}"),
                ));
            }
        }
    }
    Ok(resolution)
}

fn ensure_ubuntu_26() -> io::Result<()> {
    let os_release = fs::read_to_string("/etc/os-release")?;
    let ubuntu = os_release.lines().any(|line| line == "ID=ubuntu");
    let version_26 = os_release
        .lines()
        .find_map(|line| line.strip_prefix("VERSION_ID="))
        .is_some_and(|version| version.trim_matches('"').starts_with("26."));
    if ubuntu && version_26 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "webcam-frame requires Ubuntu 26",
        ))
    }
}

fn list_cameras() -> Vec<Camera> {
    let mut cameras = Vec::new();
    let mut nodes = v4l::context::enum_devices();
    nodes.sort_by_key(v4l::context::Node::index);

    for node in nodes {
        let Ok(device) = Device::with_path(node.path()) else {
            continue;
        };
        let Ok(capabilities) = device.query_caps() else {
            continue;
        };
        if !capabilities.capabilities.contains(Flags::VIDEO_CAPTURE) {
            continue;
        }
        let Ok(formats) = device.enum_formats() else {
            continue;
        };
        let usable = formats
            .iter()
            .any(|format| matches!(&format.fourcc.repr, b"YUYV" | b"RGB3" | b"MJPG"));
        if usable {
            cameras.push(Camera {
                name: capabilities.card,
                path: node.path().to_owned(),
            });
        }
    }
    cameras
}

fn number_key(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(0),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(1),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(2),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(3),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(4),
        _ => None,
    }
}

fn spawn_capture(
    camera: Camera,
    generation: u64,
    stop: Arc<AtomicBool>,
    proxy: EventLoopProxy<UserEvent>,
    resolution: Resolution,
) {
    thread::spawn(move || {
        if let Err(error) = capture(camera, generation, &stop, &proxy, resolution) {
            let _ = proxy.send_event(UserEvent::CaptureError {
                message: error.to_string(),
                generation,
            });
        }
    });
}

fn capture(
    camera: Camera,
    generation: u64,
    stop: &AtomicBool,
    proxy: &EventLoopProxy<UserEvent>,
    resolution: Resolution,
) -> io::Result<()> {
    let device = Device::with_path(&camera.path)?;
    let formats = device.enum_formats()?;
    let fourcc = [b"MJPG", b"YUYV", b"RGB3"]
        .into_iter()
        .map(FourCC::new)
        .find(|candidate| formats.iter().any(|format| format.fourcc == *candidate))
        .ok_or_else(|| io::Error::other("no supported pixel format"))?;
    let requested = Format::new(resolution.width, resolution.height, fourcc);
    let format = device.set_format(&requested)?;
    if !matches!(&format.fourcc.repr, b"YUYV" | b"RGB3" | b"MJPG") {
        return Err(io::Error::other(format!(
            "driver selected unsupported format {}",
            format.fourcc
        )));
    }
    eprintln!(
        "Streaming {}x{} {}",
        format.width, format.height, format.fourcc
    );

    let mut stream = MmapStream::with_buffers(&device, Type::VideoCapture, 3)?;
    while !stop.load(Ordering::Relaxed) {
        let (data, _) = stream.next()?;
        let (rgb, width, height) = decode_frame(data, &format)?;
        if proxy
            .send_event(UserEvent::Frame(Frame {
                width,
                height,
                rgb,
                generation,
            }))
            .is_err()
        {
            break;
        }
    }
    Ok(())
}

fn decode_frame(data: &[u8], format: &Format) -> io::Result<(Vec<u8>, u32, u32)> {
    match &format.fourcc.repr {
        b"YUYV" => Ok((
            yuyv_to_rgb(data, format.width, format.height, format.stride)?,
            format.width,
            format.height,
        )),
        b"RGB3" => Ok((
            packed_rgb(data, format.width, format.height, format.stride)?,
            format.width,
            format.height,
        )),
        b"MJPG" => decode_mjpeg(data),
        _ => Err(io::Error::other("unsupported frame format")),
    }
}

fn packed_rgb(data: &[u8], width: u32, height: u32, stride: u32) -> io::Result<Vec<u8>> {
    let row_bytes = width as usize * 3;
    let stride = (stride as usize).max(row_bytes);
    if data.len() < stride * height as usize {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short RGB frame",
        ));
    }
    let mut rgb = Vec::with_capacity(row_bytes * height as usize);
    for row in data.chunks(stride).take(height as usize) {
        rgb.extend_from_slice(&row[..row_bytes]);
    }
    Ok(rgb)
}

fn yuyv_to_rgb(data: &[u8], width: u32, height: u32, stride: u32) -> io::Result<Vec<u8>> {
    let row_bytes = width as usize * 2;
    let stride = (stride as usize).max(row_bytes);
    if !width.is_multiple_of(2) || data.len() < stride * height as usize {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short YUYV frame",
        ));
    }
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for row in data.chunks(stride).take(height as usize) {
        for pair in row[..row_bytes].as_chunks::<4>().0 {
            let y0 = pair[0];
            let u = pair[1];
            let y1 = pair[2];
            let v = pair[3];
            rgb.extend_from_slice(&yuv_pixel(y0, u, v));
            rgb.extend_from_slice(&yuv_pixel(y1, u, v));
        }
    }
    Ok(rgb)
}

fn yuv_pixel(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = i32::from(y).saturating_sub(16);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let clamp = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
    [
        clamp(298 * c + 409 * e),
        clamp(298 * c - 100 * d - 208 * e),
        clamp(298 * c + 516 * d),
    ]
}

fn decode_mjpeg(data: &[u8]) -> io::Result<(Vec<u8>, u32, u32)> {
    let mut decoder = jpeg_decoder::Decoder::new(data);
    let decoded = decoder
        .decode()
        .map_err(|error| io::Error::other(format!("MJPEG decode failed: {error}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| io::Error::other("MJPEG frame has no dimensions"))?;
    let rgb = match info.pixel_format {
        PixelFormat::RGB24 => decoded,
        PixelFormat::L8 => decoded.into_iter().flat_map(|value| [value; 3]).collect(),
        other => {
            return Err(io::Error::other(format!(
                "unsupported MJPEG pixel format {other:?}"
            )));
        }
    };
    Ok((rgb, u32::from(info.width), u32::from(info.height)))
}

fn draw_scaled(target: &mut [u32], target_width: u32, target_height: u32, frame: &Frame) {
    let scale =
        (target_width as f64 / frame.width as f64).min(target_height as f64 / frame.height as f64);
    let draw_width = (frame.width as f64 * scale).round() as u32;
    let draw_height = (frame.height as f64 * scale).round() as u32;
    let x_offset = (target_width - draw_width) / 2;
    let y_offset = (target_height - draw_height) / 2;

    for y in 0..draw_height {
        let source_y = (u64::from(y) * u64::from(frame.height) / u64::from(draw_height)) as usize;
        let target_row = (y + y_offset) as usize * target_width as usize;
        for x in 0..draw_width {
            let source_x = (u64::from(x) * u64::from(frame.width) / u64::from(draw_width)) as usize;
            let source = (source_y * frame.width as usize + source_x) * 3;
            let color = (u32::from(frame.rgb[source]) << 16)
                | (u32::from(frame.rgb[source + 1]) << 8)
                | u32::from(frame.rgb[source + 2]);
            target[target_row + (x + x_offset) as usize] = color;
        }
    }
}

fn save_png(frame: &Frame, camera_index: usize) -> io::Result<PathBuf> {
    let directory = pictures_directory()?.join(format!("webcam_{}", camera_index + 1));
    fs::create_dir_all(&directory)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let path = directory.join(format!("{}-{:03}.png", now.as_secs(), now.subsec_millis()));
    let file = File::create(&path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&frame.rgb)?;
    drop(writer);
    Ok(path)
}

fn pictures_directory() -> io::Result<PathBuf> {
    let output = Command::new("xdg-user-dir").arg("PICTURES").output()?;
    if !output.status.success() {
        return Err(io::Error::other("xdg-user-dir PICTURES failed"));
    }
    let path = String::from_utf8(output.stdout)
        .map_err(io::Error::other)?
        .trim()
        .to_owned();
    if path.is_empty() {
        return Err(io::Error::other("XDG Pictures directory is empty"));
    }
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_yuyv_pair_to_rgb() {
        let rgb = yuyv_to_rgb(&[16, 128, 235, 128], 2, 1, 4).unwrap();
        assert_eq!(rgb, [0, 0, 0, 255, 255, 255]);
    }

    #[test]
    fn removes_rgb_row_padding() {
        let rgb = packed_rgb(&[1, 2, 3, 4, 5, 6, 99, 99], 2, 1, 8).unwrap();
        assert_eq!(rgb, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn scales_and_letterboxes_a_frame() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgb: vec![255, 0, 0, 0, 255, 0],
            generation: 0,
        };
        let mut target = [0; 8];
        draw_scaled(&mut target, 4, 2, &frame);
        assert_eq!(
            target,
            [
                0xff0000, 0xff0000, 0x00ff00, 0x00ff00, 0xff0000, 0xff0000, 0x00ff00, 0x00ff00,
            ]
        );
    }
}
