#![no_std]
// trueos-blueprint: features=["ui4-scene"]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod native {
    use maxpix::{
        PARTICLE_COUNT, POINTER_COUNT, PointerInput, camera, next_point_width_raw, particle_mesh,
    };
    use trueos::{
        clock,
        input::{KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_TEXT},
        logl,
        ui4_scene::{CursorSource, Damage, Error as UiError, Frame},
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
                "maxpix: particles={} topology=POINT_LIST cpu-upload=once motion=native-VS point=1px display-scale=1x camera=-10,-10,-10 pointers=4 input-contract=pointer-v1 mouse-x=speed mouse-y=angle key-1=point-size",
                PARTICLE_COUNT
            ),
        );
        let started = clock::monotonic_millis();
        let mut pointers = [PointerInput::default(); POINTER_COUNT];
        let mut sources: [Option<CursorSource>; POINTER_COUNT] = [None; POINTER_COUNT];
        let mut input_tick = started;
        let mut point_width_raw = 8u32;
        let mut pointer_reports = 0u64;
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
            // Advance using the previous state before applying newly received reports.
            let now = clock::monotonic_millis();
            let seconds = now.saturating_sub(input_tick) as f32 * 0.001;
            input_tick = now;
            for pointer in &mut pointers {
                pointer.advance(seconds);
            }
            while let Some(event) = frame.take_pointer_event().map_err(|_| vgpu::ERR_IO)? {
                let slot = sources
                    .iter()
                    .position(|source| *source == Some(event.source))
                    .or_else(|| sources.iter().position(Option::is_none));
                if let Some(slot) = slot {
                    let new_source = sources[slot].is_none();
                    sources[slot] = Some(event.source);
                    pointers[slot].report(
                        [event.local_x, event.local_y],
                        [width, height],
                        event.buttons_down,
                        event.buttons_pressed,
                        event.buttons_released,
                        event.wheel,
                    );
                    pointer_reports += 1;
                    if new_source || pointer_reports % 120 == 0 {
                        logl::log(
                            logl::level::IMPORTANT,
                            format_args!(
                                "maxpix: pointer slot={} source={}:{}:{}:{} reports={} local={},{} normalized={:.3},{:.3} speed={:.3} angle={:.3}",
                                slot,
                                event.source.controller_id,
                                event.source.slot_id,
                                event.source.ep_target,
                                event.source.hid_kind,
                                pointer_reports,
                                event.local_x,
                                event.local_y,
                                pointers[slot].position[0],
                                pointers[slot].position[1],
                                pointers[slot].position[0] * 2.0,
                                (pointers[slot].position[1] - 0.5) * core::f32::consts::TAU
                            ),
                        );
                    }
                }
            }
            while let Some(event) = frame.take_keyboard_event().map_err(|_| vgpu::ERR_IO)? {
                // Printable digits arrive as TEXT, not as named KEY events.
                if event.kind == KEYBOARD_OUTPUT_KIND_TEXT
                    && event.codepoint == u32::from(b'1')
                    && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                {
                    point_width_raw = next_point_width_raw(point_width_raw);
                    logl::log(
                        logl::level::IMPORTANT,
                        format_args!(
                            "maxpix: point-size={:.3}px native-u8.3={} key=1",
                            point_width_raw as f32 / 8.0,
                            point_width_raw
                        ),
                    );
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
            let mut view = camera().retained(width, height, [0.0; 16]);
            view.jitter_frame[2] = (clock::monotonic_millis() - started) as f32 * 0.001;
            // Dedicated Maxpix native raster control, validated by the kernel.
            view.jitter_frame[3] = point_width_raw as f32;
            for (slot, pointer) in pointers.iter().enumerate() {
                let (state, buttons) = pointer.pack();
                view.inverse_view_projection[slot * 4..slot * 4 + 4].copy_from_slice(&state);
                view.previous_view_projection[slot * 4..slot * 4 + 4].copy_from_slice(&buttons);
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
                    Err(UiError::Busy) => {
                        vsys::poll_once();
                        vsys::sleep_ms(1);
                    }
                    Err(_) => return Err(vgpu::ERR_IO),
                }
            }
            for pointer in &mut pointers {
                pointer.published();
            }
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
                        "maxpix: fps={:.1} frames={} elapsed_ms={} particles={} extent={}x{} pointer-reports={} point-size={:.3}px",
                        fps,
                        fps_frames,
                        elapsed_ms,
                        PARTICLE_COUNT,
                        width,
                        height,
                        pointer_reports,
                        point_width_raw as f32 / 8.0
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
