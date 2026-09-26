//! Host-only link seams for process tests. GPU execution is not mocked as success:
//! any accidental call aborts the test rather than pretending to render.
#![allow(unused_variables)]

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_queue_create(device: u64, class: u32, out_queue: *mut u64) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_queue_create");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_buffer_write(
    device: u64,
    buffer: u64,
    offset: usize,
    data: *const u8,
    data_len: usize,
) -> isize {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_buffer_write");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_buffer_create(
    device: u64,
    bytes: usize,
    usage: u32,
    out_buffer: *mut u64,
) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_buffer_create");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_ui4_surface_clear_submit(
    device: u64,
    queue: u64,
    surface: u64,
    rgba8_srgb: u32,
    out_point: *mut trueos::vgpu::TimelinePoint,
) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_ui4_surface_clear_submit");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_ui4_surface_acquire(
    device: u64,
    window_id: u32,
    out: *mut trueos::vgpu::SurfaceInfo,
) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_ui4_surface_acquire");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_shader_module_create(
    device: u64,
    package_digest: u64,
    out_shader: *mut u64,
) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_shader_module_create");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_render_pipeline_create(
    device: u64,
    shader: u64,
    vertex_stride: u32,
    position_offset: u32,
    out_pipeline: *mut u64,
) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_render_pipeline_create");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_ui4_indexed_batch_submit_v2(
    device: u64,
    queue: u64,
    batch: *const trueos::vgpu::IndexedDrawBatchV2,
    out_point: *mut trueos::vgpu::TimelinePoint,
) -> i32 {
    panic!(
        "GPU ABI called by host-only process test: trueos_cabi_vgpu_ui4_indexed_batch_submit_v2"
    );
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_open(requested_caps: u64, out_device: *mut u64) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_open");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_wait(device: u64, queue: u64, value: u64) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_wait");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_close(device: u64) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_close");
}

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_vgpu_ui4_surface_discard(device: u64, surface: u64) -> i32 {
    panic!("GPU ABI called by host-only process test: trueos_cabi_vgpu_ui4_surface_discard");
}
