#![no_std]
// trueos-blueprint: features=["ui4-scene"]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod native {
    use maxpix::{PARTICLE_COUNT, camera, particle_mesh};
    use trueos::{
        clock, logl,
        ui4_scene::{Damage, Error as UiError, Frame},
        vgpu::{
            self, Capabilities, Device, QueueClass, RetainedFrameSubmit, RetainedMeshDescriptor,
            RetainedTransformSeed,
        },
        vsys,
    };

    pub fn run() -> Result<(), i32> {
        let (mut width, mut height) = (800, 600);
        let mut frame = Frame::open_streaming(80, 64, width, height).map_err(|_| vgpu::ERR_IO)?;
        let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
        let queue = device.create_queue(QueueClass::Render)?;
        let (vertices, indices) = particle_mesh(PARTICLE_COUNT);
        let vertex_buffer = device.create_buffer(
            vertices.len(),
            vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
        )?;
        let index_buffer = device.create_buffer(
            indices.len(),
            vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
        )?;
        if device.write_buffer(vertex_buffer, 0, &vertices)? != vertices.len()
            || device.write_buffer(index_buffer, 0, &indices)? != indices.len()
        {
            return Err(vgpu::ERR_IO);
        }
        let mesh = device.create_retained_mesh(
            vertex_buffer,
            index_buffer,
            RetainedMeshDescriptor {
                vertex_count: PARTICLE_COUNT,
                index_count: PARTICLE_COUNT,
                vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_MAXPIX_TORNADO,
                topology: vgpu::PRIMITIVE_TOPOLOGY_POINT_LIST,
                ..RetainedMeshDescriptor::default()
            },
        )?;
        drop(vertices);
        drop(indices);
        logl::log(
            logl::level::IMPORTANT,
            format_args!(
                "maxpix: particles={} topology=POINT_LIST cpu-upload=once motion=native-VS point=1px display-scale=1x camera=-10,-10,-10",
                PARTICLE_COUNT
            ),
        );
        let started = clock::monotonic_millis();
        let mut previous = [0.0; 16];
        let mut pending_resize = None;
        let mut frames = 0u64;
        let mut fps_started = started;
        let mut fps_frames = 0u64;
        loop {
            while let Some(event) = frame.take_resize_event().map_err(|_| vgpu::ERR_IO)? {
                pending_resize = Some((event.width.max(1), event.height.max(1)));
            }
            if let Some((w, h)) = pending_resize {
                match frame.resize(w, h) {
                    Ok(()) => {
                        width = w;
                        height = h;
                        pending_resize = None;
                    }
                    Err(UiError::Busy) => {
                        vsys::sleep_ms(1);
                        continue;
                    }
                    Err(_) => return Err(vgpu::ERR_IO),
                }
            }
            match frame.begin_gpu_frame() {
                Ok(()) => {}
                Err(UiError::Busy) => {
                    vsys::sleep_ms(1);
                    continue;
                }
                Err(_) => return Err(vgpu::ERR_IO),
            }
            let surface = device.acquire_ui4_surface(frame.window_id())?;
            let mut view = camera().retained(width, height, previous);
            view.jitter_frame[2] = (clock::monotonic_millis() - started) as f32 * 0.001;
            if frames == 0 {
                view.previous_view_projection = view.view_projection;
            }
            let mut submit = RetainedFrameSubmit {
                camera: view,
                clear_rgba8_srgb: 0x0000_0000,
                seed_count: 1,
                ..RetainedFrameSubmit::default()
            };
            submit.seeds[0] = RetainedTransformSeed {
                scale: [1.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                local_radius: 7.1,
                ..RetainedTransformSeed::default()
            };
            let point = device.submit_retained_frame(
                queue,
                surface,
                mesh,
                vertex_buffer,
                index_buffer,
                submit,
            )?;
            device.wait(queue, point.value)?;
            loop {
                match frame.publish(Damage::full(width, height)) {
                    Ok(()) => break,
                    Err(UiError::Busy) => { vsys::poll_once(); vsys::sleep_ms(1); }
                    Err(_) => return Err(vgpu::ERR_IO),
                }
            }
            previous = view.view_projection;
            frames += 1;
            if frames <= 4 {
                logl::log(
                    logl::level::IMPORTANT,
                    format_args!(
                        "maxpix: frame={} particles={} extent={}x{} time={:.3} timeline={}",
                        frames, PARTICLE_COUNT, width, height, view.jitter_frame[2], point.value
                    ),
                );
            }
            fps_frames += 1;
            let now = clock::monotonic_millis();
            let elapsed_ms = now.saturating_sub(fps_started);
            if elapsed_ms >= 4_000 {
                let fps = fps_frames as f64 * 1_000.0 / elapsed_ms as f64;
                logl::log(
                    logl::level::IMPORTANT,
                    format_args!(
                        "maxpix: fps={:.1} frames={} elapsed_ms={} particles={} extent={}x{}",
                        fps, fps_frames, elapsed_ms, PARTICLE_COUNT, width, height
                    ),
                );
                fps_started = now;
                fps_frames = 0;
            }
            vsys::poll_once();
            vsys::sleep_ms(16);
        }
    }
}

fn main() {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    if let Err(code) = native::run() {
        trueos::logl::log(
            trueos::logl::level::ERROR,
            format_args!("maxpix: failed code={code}"),
        );
        trueos::panic_abort("maxpix failed\n");
    }
}
