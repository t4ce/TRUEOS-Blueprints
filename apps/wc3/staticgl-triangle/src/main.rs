#![no_std]

// trueos-blueprint: features=["ui4-scene"]

use staticgl_triangle::{TriangleRenderer, Vertex};
use trueos::{
    logl,
    ui4_scene::{Damage, Frame},
    vgpu::{self, Capabilities, Device, QueueClass},
    vsys,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const CLEAR: u32 = 0xff10_1820;

fn main() {
    if let Err(code) = run() {
        logl::log(
            trueos::logl::level::ERROR,
            format_args!("staticgl-triangle: failed code={code}"),
        );
    }
}

fn run() -> Result<(), i32> {
    let mut frame = Frame::open_streaming(80, 64, WIDTH, HEIGHT).map_err(|_| vgpu::ERR_IO)?;
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let queue = device.create_queue(QueueClass::Render)?;
    let renderer = TriangleRenderer::new(device)?;
    let vertices = [
        Vertex {
            position: [-0.72, -0.62, 0.0],
            color: [1.0, 0.05, 0.05, 1.0],
        },
        Vertex {
            position: [0.72, -0.62, 0.0],
            color: [0.05, 1.0, 0.05, 1.0],
        },
        Vertex {
            position: [0.0, 0.72, 0.0],
            color: [0.05, 0.15, 1.0, 1.0],
        },
    ];

    loop {
        frame.begin_gpu_frame().map_err(|_| vgpu::ERR_IO)?;
        let surface = device.acquire_ui4_surface(frame.window_id())?;
        let point = renderer.draw(queue, surface, &vertices, CLEAR)?;
        device.wait(queue, point.value)?;
        frame
            .publish(Damage::full(WIDTH, HEIGHT))
            .map_err(|_| vgpu::ERR_IO)?;
        vsys::sleep_ms(16);
    }
}
