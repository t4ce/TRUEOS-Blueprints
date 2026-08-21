//! WGPU custom backend over the VMX vGPU ABI.
//!
//! This is deliberately a partial backend: unsupported GPU object classes fail
//! loudly. Buffers and linear RGBA8 textures are real vGPU allocations; the
//! latter deliberately reuse VMX's generic opaque buffer residency rather than
//! introducing an application- or voxel-specific kernel object.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use helio::{FlyCamera, FlyCameraConfig, PerspectiveLens};
use trueos::clock;
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame};
use trueos::vgpu::{
    self, BUFFER_USAGE_COPY_DST, BUFFER_USAGE_COPY_SRC, BUFFER_USAGE_INDEX, BUFFER_USAGE_MAP_READ,
    BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_STORAGE, BUFFER_USAGE_VERTEX, Capabilities, IndexedDraw,
    QueueClass, SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64,
};
use wgpu::custom::*;

use crate::ui4_input::Ui4FlyInput;

const WITNESS: &[u8] = b"HelioV/WGPU/custom/VMX";
const ERR_INVALID_ARGUMENT: i32 = -22;
// Linear textures are opaque WGPU resources, but Queue::write_texture reaches
// VMX through the same bounded CPU transfer primitive as Queue::write_buffer.
// COPY_DST therefore carries the broker-internal MAP_WRITE authority just as
// `vmx_buffer_usage` does for ordinary WGPU buffers.
const LINEAR_TEXTURE_BACKING_USAGE: u32 =
    BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_SRC | BUFFER_USAGE_COPY_DST | BUFFER_USAGE_MAP_WRITE;
pub const VOXEL_SHADER_WGSL: &str = "@vertex\nfn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {\n    return vec4<f32>(position, 1.0);\n}\n\n@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(0.18, 0.72, 0.32, 1.0);\n}\n";

pub const SCENE_BASELINE_CONTRACT: &str = "constant-rgba/indexed-world/no-texture-assets";
const AUTHENTICATED_SHADER_DIGEST: u64 = SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64;

pub struct BackendFailure {
    pub stage: &'static str,
    pub code: i32,
}

pub struct SurfaceProbe {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bytes: u64,
    pub timeline: u64,
    pub aspect: f32,
    frame: Frame,
    device: wgpu::Device,
    queue: wgpu::Queue,
    graphics: VoxelGraphics,
    camera: FlyCamera,
    lens: PerspectiveLens,
    input: Ui4FlyInput,
    last_input_millis: u64,
    camera_dirty: bool,
    input_live: bool,
}

pub struct ResizePresentation {
    pub old_width: u32,
    pub old_height: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bytes: u64,
    pub timeline: u64,
    pub aspect: f32,
}

pub struct InputPresentation {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub timeline: u64,
}

pub fn probe_wgpu_buffer_path() -> Result<usize, BackendFailure> {
    let (device, queue) = open_device_queue()?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("HelioV custom-backend witness"),
        size: WITNESS.len() as u64,
        usage: wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, WITNESS);

    let custom = buffer
        .as_custom::<VmxBuffer>()
        .ok_or_else(|| fail("buffer-downcast", vgpu::ERR_BAD_HANDLE))?;
    if let Some(code) = custom.shared.take_error() {
        return Err(fail("wgpu-buffer-write", code));
    }
    let mut readback = [0u8; WITNESS.len()];
    let read = custom
        .shared
        .device
        .read_buffer(custom.buffer, 0, &mut readback)
        .map_err(|code| fail("wgpu-buffer-readback", code))?;
    if read != WITNESS.len() || readback != WITNESS {
        return Err(fail("wgpu-buffer-roundtrip", vgpu::ERR_IO));
    }
    drop(buffer);
    drop(queue);
    drop(device);
    Ok(read)
}

pub fn open_device_queue() -> Result<(wgpu::Device, wgpu::Queue), BackendFailure> {
    let device = vgpu::Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
        .map_err(|code| fail("wgpu-device-open", code))?;
    let queue = match device.create_queue(QueueClass::Render) {
        Ok(queue) => queue,
        Err(code) => {
            let _ = device.close();
            return Err(fail("wgpu-queue-open", code));
        }
    };
    let shared = Arc::new(SharedDevice {
        device,
        last_error: AtomicI32::new(0),
        last_submission: AtomicU64::new(0),
    });
    Ok((
        wgpu::Device::from_custom(VmxDevice {
            shared: Arc::clone(&shared),
        }),
        wgpu::Queue::from_custom(VmxQueue { shared, queue }),
    ))
}

pub fn acquire_ui4_texture(
    device: &wgpu::Device,
    window_id: u32,
) -> Result<wgpu::Texture, BackendFailure> {
    let device = device
        .as_custom::<VmxDevice>()
        .ok_or_else(|| fail("ui4-surface-device", vgpu::ERR_BAD_HANDLE))?;
    let surface = device
        .shared
        .device
        .acquire_ui4_surface(window_id)
        .map_err(|code| fail("ui4-surface-acquire", code))?;
    let info = surface.info();
    let descriptor = wgpu::TextureDescriptor {
        label: Some("HelioV UI4 surface"),
        size: wgpu::Extent3d {
            width: info.width,
            height: info.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    Ok(wgpu::Texture::from_custom(
        VmxTexture {
            storage: Arc::new(VmxTextureStorage {
                shared: Arc::clone(&device.shared),
                backing: VmxTextureBacking::Ui4 {
                    surface: Mutex::new(Some(surface)),
                    info,
                },
            }),
        },
        &descriptor,
    ))
}

pub fn probe_ui4_surface_path(mesh: &helio::MeshUpload) -> Result<SurfaceProbe, BackendFailure> {
    let mut frame = Frame::open_streaming(120, 96, 640, 360)
        .map_err(|_| fail("ui4-frame-open", vgpu::ERR_IO))?;
    let (device, queue) = open_device_queue()?;
    let camera = FlyCamera::look_at(
        glam::Vec3::new(65.0, 42.0, 68.0),
        glam::Vec3::new(24.0, 7.0, 24.0),
        FlyCameraConfig {
            movement_speed: 12.0,
            boost_multiplier: 3.0,
            normalize_movement: true,
            ..FlyCameraConfig::default()
        },
    );
    let lens = PerspectiveLens {
        fov_y_radians: 48.0_f32.to_radians(),
        near: 0.1,
        far: 180.0,
    };
    let graphics = VoxelGraphics::new(
        &device,
        &queue,
        mesh,
        aspect(frame.width(), frame.height()),
        &camera,
        lens,
    )?;
    trueos::logl::log(
        trueos::logl::level::INFO,
        format_args!(
            "HelioV: constant-RGBA scene baseline submits complete Helio voxel world vertices={} indices={} shader_package=clip-position3-rgba material_source=constant-fragment-no-assets",
            mesh.vertices.len(),
            mesh.indices.len(),
        ),
    );
    let first = render_voxel_frame(&mut frame, &device, &queue, &graphics)?;
    let presentation_deadline = clock::monotonic_millis().saturating_add(3_000);
    loop {
        match frame.take_first_presentation() {
            Ok(true) => break,
            Ok(false) if clock::monotonic_millis() < presentation_deadline => {
                trueos::vsys::poll_once();
                trueos::vsys::sleep_ms(5);
            }
            Ok(false) => return Err(fail("ui4-surflive-timeout", vgpu::ERR_BUSY)),
            Err(_) => return Err(fail("ui4-surflive", vgpu::ERR_IO)),
        }
    }
    Ok(SurfaceProbe {
        width: first.width,
        height: first.height,
        pitch: first.pitch,
        bytes: first.bytes,
        timeline: first.timeline,
        aspect: aspect(first.width, first.height),
        frame,
        device,
        queue,
        graphics,
        camera,
        lens,
        input: Ui4FlyInput::new(),
        last_input_millis: clock::monotonic_millis(),
        camera_dirty: false,
        input_live: false,
    })
}

impl SurfaceProbe {
    /// Consume UI4's latest maximize/restore request and publish the first
    /// complete GPU frame for the replacement lease. UI4 keeps the previous
    /// SURFLIVE front until this succeeds, so no stretched or blank generation
    /// is exposed between extents.
    pub fn present_pending_resize(&mut self) -> Result<Option<ResizePresentation>, BackendFailure> {
        let mut pending = None;
        loop {
            match self.frame.take_resize_event() {
                Ok(Some(event)) => pending = Some(event),
                Ok(None) => break,
                Err(_) => return Err(fail("ui4-resize-event", vgpu::ERR_IO)),
            }
        }
        let Some(event) = pending else {
            return Ok(None);
        };
        if event.width == self.width && event.height == self.height {
            return Ok(None);
        }
        if event.width == 0 || event.height == 0 {
            return Err(fail("ui4-resize-extent", ERR_INVALID_ARGUMENT));
        }

        self.frame
            .resize(event.width, event.height)
            .map_err(|_| fail("ui4-resize-stage", vgpu::ERR_IO))?;
        self.graphics.update_projection(
            &self.queue,
            aspect(event.width, event.height),
            &self.camera,
            self.lens,
        )?;
        let rendered =
            render_voxel_frame(&mut self.frame, &self.device, &self.queue, &self.graphics)?;
        self.width = rendered.width;
        self.height = rendered.height;
        self.pitch = rendered.pitch;
        self.bytes = rendered.bytes;
        self.timeline = rendered.timeline;
        self.aspect = aspect(rendered.width, rendered.height);
        Ok(Some(ResizePresentation {
            old_width: event.old_width,
            old_height: event.old_height,
            width: rendered.width,
            height: rendered.height,
            pitch: rendered.pitch,
            bytes: rendered.bytes,
            timeline: rendered.timeline,
            aspect: self.aspect,
        }))
    }

    /// Consume the application-focused UI4 cursor/keyboard route, update the
    /// shared Helio camera, and publish only when the pose changed. A primary
    /// drag looks; WASD/Space/Shift move; Control boosts.
    pub fn present_pending_input(&mut self) -> Result<Option<InputPresentation>, BackendFailure> {
        let now = clock::monotonic_millis();
        let delta_seconds = now.saturating_sub(self.last_input_millis) as f32 / 1_000.0;
        self.last_input_millis = now;
        let input = self
            .input
            .sample(&mut self.frame)
            .map_err(|error| fail("ui4-input-route", ui4_error_code(error)))?;
        self.camera_dirty |= self.camera.update(input, delta_seconds);
        if !self.camera_dirty {
            return Ok(None);
        }

        self.graphics.update_projection(
            &self.queue,
            aspect(self.width, self.height),
            &self.camera,
            self.lens,
        )?;
        let rendered =
            match render_voxel_frame(&mut self.frame, &self.device, &self.queue, &self.graphics) {
                Ok(rendered) => rendered,
                Err(failure)
                    if failure.stage == "ui4-frame-begin" && failure.code == vgpu::ERR_BUSY =>
                {
                    return Ok(None);
                }
                Err(failure) => return Err(failure),
            };
        self.timeline = rendered.timeline;
        self.camera_dirty = false;

        if self.input_live {
            return Ok(None);
        }
        self.input_live = true;
        Ok(Some(InputPresentation {
            position: self.camera.position().to_array(),
            yaw: self.camera.yaw(),
            pitch: self.camera.pitch(),
            timeline: rendered.timeline,
        }))
    }
}

struct RenderedFrame {
    width: u32,
    height: u32,
    pitch: u32,
    bytes: u64,
    timeline: u64,
}

struct VoxelGraphics {
    world_positions: Vec<[f32; 3]>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    index_count: u32,
}

impl VoxelGraphics {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &helio::MeshUpload,
        aspect: f32,
        camera: &FlyCamera,
        lens: PerspectiveLens,
    ) -> Result<Self, BackendFailure> {
        let world_positions: Vec<_> = mesh.vertices.iter().map(|vertex| vertex.position).collect();
        let projected =
            crate::voxel::project_clip_positions(&world_positions, aspect, camera, lens);
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Helio voxel projected positions"),
            size: byte_len(&projected) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Helio voxel indices"),
            size: byte_len(&mesh.indices) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytes_of_slice(&projected));
        queue.write_buffer(&index_buffer, 0, bytes_of_slice(&mesh.indices));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HelioV authenticated position3 constant-RGBA shader package"),
            source: wgpu::ShaderSource::Wgsl(VOXEL_SHADER_WGSL.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HelioV VMX voxel pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<[f32; 3]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        if let Some(code) = take_device_error(device) {
            return Err(fail("voxel-graphics-create", code));
        }
        Ok(Self {
            world_positions,
            vertex_buffer,
            index_buffer,
            pipeline,
            index_count: u32::try_from(mesh.indices.len())
                .map_err(|_| fail("voxel-index-count", ERR_INVALID_ARGUMENT))?,
        })
    }

    fn update_projection(
        &mut self,
        queue: &wgpu::Queue,
        aspect: f32,
        camera: &FlyCamera,
        lens: PerspectiveLens,
    ) -> Result<(), BackendFailure> {
        let projected =
            crate::voxel::project_clip_positions(&self.world_positions, aspect, camera, lens);
        queue.write_buffer(&self.vertex_buffer, 0, bytes_of_slice(&projected));
        Ok(())
    }
}

fn render_voxel_frame(
    frame: &mut Frame,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    graphics: &VoxelGraphics,
) -> Result<RenderedFrame, BackendFailure> {
    frame
        .begin_gpu_frame()
        .map_err(|error| fail("ui4-frame-begin", ui4_error_code(error)))?;
    let texture = acquire_ui4_texture(device, frame.window_id())?;
    let custom_texture = texture
        .as_custom::<VmxTexture>()
        .ok_or_else(|| fail("ui4-texture-downcast", vgpu::ERR_BAD_HANDLE))?;
    let VmxTextureBacking::Ui4 { info, .. } = &custom_texture.storage.backing else {
        return Err(fail("ui4-texture-backing", vgpu::ERR_BAD_HANDLE));
    };
    let info = *info;
    if info.width != frame.width() || info.height != frame.height() {
        return Err(fail("ui4-surface-extent", ERR_INVALID_ARGUMENT));
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("HelioV VMX indexed voxel encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HelioV VMX UI4 voxel draw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.025,
                        g: 0.055,
                        b: 0.12,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if graphics.index_count != 0 {
            pass.set_pipeline(&graphics.pipeline);
            pass.set_vertex_buffer(0, graphics.vertex_buffer.slice(..));
            pass.set_index_buffer(graphics.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..graphics.index_count, 0, 0..1);
        }
    }
    let _submission = queue.submit([encoder.finish()]);
    if let Some(code) = take_device_error(device) {
        return Err(fail("ui4-indexed-submit", code));
    }
    frame
        .publish(Damage::full(info.width, info.height))
        .map_err(|_| fail("ui4-surface-publish", vgpu::ERR_IO))?;
    let timeline = device
        .as_custom::<VmxDevice>()
        .expect("VMX device")
        .shared
        .last_submission
        .load(Ordering::Acquire);
    Ok(RenderedFrame {
        width: info.width,
        height: info.height,
        pitch: info.pitch,
        bytes: info.bytes,
        timeline,
    })
}

fn byte_len<T>(values: &[T]) -> usize {
    values.len().saturating_mul(core::mem::size_of::<T>())
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), byte_len(values)) }
}

fn aspect(width: u32, height: u32) -> f32 {
    width as f32 / height as f32
}

const fn ui4_error_code(error: Ui4Error) -> i32 {
    match error {
        Ui4Error::Invalid => ERR_INVALID_ARGUMENT,
        Ui4Error::Busy => vgpu::ERR_BUSY,
        Ui4Error::Unknown(code) => code,
        _ => vgpu::ERR_IO,
    }
}

pub fn take_device_error(device: &wgpu::Device) -> Option<i32> {
    device
        .as_custom::<VmxDevice>()
        .and_then(|device| device.shared.take_error())
}

#[derive(Debug)]
struct SharedDevice {
    device: vgpu::Device,
    last_error: AtomicI32,
    last_submission: AtomicU64,
}

impl SharedDevice {
    fn record_error(&self, code: i32) {
        let _ = self
            .last_error
            .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
    }

    fn take_error(&self) -> Option<i32> {
        let code = self.last_error.swap(0, Ordering::AcqRel);
        (code != 0).then_some(code)
    }
}

impl Drop for SharedDevice {
    fn drop(&mut self) {
        let _ = self.device.close();
    }
}

#[derive(Debug)]
struct VmxDevice {
    shared: Arc<SharedDevice>,
}

#[derive(Debug)]
struct VmxQueue {
    shared: Arc<SharedDevice>,
    queue: vgpu::Queue,
}

impl Drop for VmxQueue {
    fn drop(&mut self) {
        let _ = self.shared.device.destroy_queue(self.queue);
    }
}

#[derive(Debug)]
struct VmxBuffer {
    shared: Arc<SharedDevice>,
    buffer: vgpu::Buffer,
    destroyed: AtomicBool,
}

#[derive(Clone, Copy, Debug)]
struct LinearTextureInfo {
    width: u32,
    height: u32,
    layers: u32,
    bytes_per_pixel: u32,
    bytes_per_row: u32,
}

#[derive(Debug)]
enum VmxTextureBacking {
    Ui4 {
        surface: Mutex<Option<vgpu::Ui4Surface>>,
        info: vgpu::SurfaceInfo,
    },
    Linear {
        buffer: vgpu::Buffer,
        info: LinearTextureInfo,
        destroyed: AtomicBool,
    },
}

#[derive(Debug)]
struct VmxTextureStorage {
    shared: Arc<SharedDevice>,
    backing: VmxTextureBacking,
}

impl Drop for VmxTextureStorage {
    fn drop(&mut self) {
        match &self.backing {
            VmxTextureBacking::Ui4 { surface, .. } => {
                let _ = surface.lock().expect("VMX surface mutex").take();
            }
            VmxTextureBacking::Linear {
                buffer, destroyed, ..
            } => {
                if !destroyed.swap(true, Ordering::AcqRel) {
                    let _ = self.shared.device.destroy_buffer(*buffer);
                }
            }
        }
    }
}

struct VmxTexture {
    storage: Arc<VmxTextureStorage>,
}

impl core::fmt::Debug for VmxTexture {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("VmxTexture")
            .field("device", &self.storage.shared.device.raw())
            .field("backing", &self.storage.backing)
            .finish_non_exhaustive()
    }
}

impl VmxTexture {
    fn destroy_once(&self) {
        match &self.storage.backing {
            VmxTextureBacking::Ui4 { surface, .. } => {
                let _ = surface.lock().expect("VMX surface mutex").take();
            }
            VmxTextureBacking::Linear {
                buffer, destroyed, ..
            } => {
                if !destroyed.swap(true, Ordering::AcqRel) {
                    let _ = self.storage.shared.device.destroy_buffer(*buffer);
                }
            }
        }
    }
}

#[derive(Debug)]
struct VmxTextureView {
    storage: Arc<VmxTextureStorage>,
}

#[derive(Debug)]
struct VmxSampler;

#[derive(Debug)]
struct VmxShaderModule {
    shared: Arc<SharedDevice>,
    shader: vgpu::ShaderModule,
}

impl Drop for VmxShaderModule {
    fn drop(&mut self) {
        let _ = self.shared.device.destroy_shader_module(self.shader);
    }
}

impl ShaderModuleInterface for VmxShaderModule {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        Box::pin(std::future::ready(wgpu::CompilationInfo {
            messages: Vec::new(),
        }))
    }
}

#[derive(Debug)]
struct VmxRenderPipeline {
    shared: Arc<SharedDevice>,
    pipeline: vgpu::RenderPipeline,
}

impl Drop for VmxRenderPipeline {
    fn drop(&mut self) {
        let _ = self.shared.device.destroy_render_pipeline(self.pipeline);
    }
}

impl RenderPipelineInterface for VmxRenderPipeline {
    fn get_bind_group_layout(&self, _index: u32) -> DispatchBindGroupLayout {
        unsupported("constant-RGBA package exposes no bind groups")
    }
}

#[derive(Debug)]
enum VmxCommand {
    Clear {
        surface: Arc<Mutex<Option<vgpu::Ui4Surface>>>,
        rgba8_srgb: u32,
    },
    Indexed {
        surface: Arc<Mutex<Option<vgpu::Ui4Surface>>>,
        pipeline: DispatchRenderPipeline,
        vertex: DispatchBuffer,
        vertex_offset: u64,
        index: DispatchBuffer,
        index_offset: u64,
        first_index: u32,
        index_count: u32,
        clear_rgba8_srgb: u32,
    },
}

#[derive(Debug)]
struct VmxCommandEncoder {
    commands: Arc<Mutex<Vec<VmxCommand>>>,
}

#[derive(Debug)]
struct VmxCommandBuffer {
    commands: Mutex<Vec<VmxCommand>>,
}

#[derive(Debug)]
struct VmxRenderPass {
    commands: Arc<Mutex<Vec<VmxCommand>>>,
    surface: Arc<Mutex<Option<vgpu::Ui4Surface>>>,
    clear_rgba8_srgb: u32,
    pipeline: Option<DispatchRenderPipeline>,
    vertex: Option<(DispatchBuffer, u64)>,
    index: Option<(DispatchBuffer, u64)>,
    emitted: bool,
}

impl Drop for VmxRenderPass {
    fn drop(&mut self) {
        if !self.emitted {
            self.commands
                .lock()
                .expect("VMX command encoder mutex")
                .push(VmxCommand::Clear {
                    surface: Arc::clone(&self.surface),
                    rgba8_srgb: self.clear_rgba8_srgb,
                });
        }
    }
}

impl Drop for VmxBuffer {
    fn drop(&mut self) {
        self.destroy_once();
    }
}

impl VmxBuffer {
    fn destroy_once(&self) {
        if !self.destroyed.swap(true, Ordering::AcqRel) {
            let _ = self.shared.device.destroy_buffer(self.buffer);
        }
    }
}

impl BufferInterface for VmxBuffer {
    fn map_async(
        &self,
        _mode: wgpu::MapMode,
        _range: core::ops::Range<wgpu::BufferAddress>,
        callback: BufferMapCallback,
    ) {
        callback(Err(wgpu::BufferAsyncError));
    }

    fn get_mapped_range(
        &self,
        _sub_range: core::ops::Range<wgpu::BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, wgpu::MapRangeError> {
        unsupported("mapped buffer ranges")
    }

    fn unmap(&self) {}

    fn destroy(&self) {
        self.destroy_once();
    }
}

impl TextureInterface for VmxTexture {
    fn create_view(&self, _desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        DispatchTextureView::custom(VmxTextureView {
            storage: Arc::clone(&self.storage),
        })
    }

    fn destroy(&self) {
        self.destroy_once();
    }
}

impl TextureViewInterface for VmxTextureView {}

impl SamplerInterface for VmxSampler {}

impl DeviceInterface for VmxDevice {
    fn features(&self) -> wgpu::Features {
        wgpu::Features::empty()
    }

    fn limits(&self) -> wgpu::Limits {
        wgpu::Limits::default()
    }

    fn adapter_info(&self) -> wgpu::AdapterInfo {
        // WGPU 30 has no public `Backend::Custom` discriminator; `Noop` is the
        // only non-hosted placeholder. Execution is not a noop: the custom
        // dispatch object below owns the real VMX device.
        let mut info = wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Noop);
        info.name = "TRUEOS VMX vGPU".into();
        info.vendor = 0x8086;
        info.driver = "TRUEOS mediated Intel GPU".into();
        info.driver_info = "vmx-vgpu-v1".into();
        info
    }

    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        let usage = vmx_buffer_usage(desc.usage);
        let buffer = self.shared.device.create_buffer(desc.size as usize, usage).unwrap_or_else(
            |code| {
                let info = self.shared.device.info().ok();
                panic!(
                    "VMX vGPU buffer creation failed: {code}; label={:?} bytes={} wgpu_usage={:?} vmx_usage={usage:#x} device={info:?}",
                    desc.label,
                    desc.size,
                    desc.usage,
                )
            },
        );
        DispatchBuffer::custom(VmxBuffer {
            shared: Arc::clone(&self.shared),
            buffer,
            destroyed: AtomicBool::new(false),
        })
    }

    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        _checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        let wgpu::ShaderSource::Wgsl(source) = desc.source else {
            return unsupported("non-WGSL shader module");
        };
        let digest = fnv1a64(source.as_bytes());
        if source.as_ref() != VOXEL_SHADER_WGSL || digest != AUTHENTICATED_SHADER_DIGEST {
            return unsupported("WGSL has no authenticated TRUEOS AOT package");
        }
        let shader = self
            .shared
            .device
            .create_shader_module(digest)
            .unwrap_or_else(|code| panic!("VMX shader package admission failed: {code}"));
        DispatchShaderModule::custom(VmxShaderModule {
            shared: Arc::clone(&self.shared),
            shader,
        })
    }

    unsafe fn create_shader_module_passthrough(
        &self,
        _desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        unsupported("shader module passthrough")
    }

    fn create_bind_group_layout(
        &self,
        _: &wgpu::BindGroupLayoutDescriptor<'_>,
    ) -> DispatchBindGroupLayout {
        unsupported("constant-RGBA package has no bind group layouts")
    }
    fn create_bind_group(&self, _: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        unsupported("constant-RGBA package has no bind groups")
    }
    fn create_pipeline_layout(
        &self,
        _: &wgpu::PipelineLayoutDescriptor<'_>,
    ) -> DispatchPipelineLayout {
        unsupported("constant-RGBA package uses the implicit empty pipeline layout")
    }
    fn create_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        let shader = desc
            .vertex
            .module
            .as_custom::<VmxShaderModule>()
            .expect("VMX pipeline received a foreign vertex shader");
        let fragment = desc
            .fragment
            .as_ref()
            .expect("VMX graphics package requires a fragment stage");
        let fragment_shader = fragment
            .module
            .as_custom::<VmxShaderModule>()
            .expect("VMX pipeline received a foreign fragment shader");
        let buffers: Vec<_> = desc.vertex.buffers.iter().flatten().collect();
        let targets: Vec<_> = fragment.targets.iter().flatten().collect();
        if desc.layout.is_some()
            || shader.shader != fragment_shader.shader
            || desc.vertex.entry_point != Some("vs_main")
            || fragment.entry_point != Some("fs_main")
            || buffers.len() != 1
            || targets.len() != 1
            || targets[0].format != wgpu::TextureFormat::Rgba8UnormSrgb
            || targets[0].blend.is_some()
            || targets[0].write_mask != wgpu::ColorWrites::ALL
            || desc.primitive.topology != wgpu::PrimitiveTopology::TriangleList
            || desc.primitive.strip_index_format.is_some()
            || desc.primitive.front_face != wgpu::FrontFace::Ccw
            || desc.primitive.cull_mode.is_some()
            || desc.primitive.polygon_mode != wgpu::PolygonMode::Fill
            || desc.primitive.unclipped_depth
            || desc.primitive.conservative
            || desc.depth_stencil.is_some()
            || desc.multisample.count != 1
            || desc.multisample.mask != !0
            || desc.multisample.alpha_to_coverage_enabled
            || desc.multiview_mask.is_some()
            || desc.cache.is_some()
        {
            return unsupported("render pipeline outside authenticated package interface");
        }
        let attributes = buffers[0].attributes;
        if buffers[0].step_mode != wgpu::VertexStepMode::Vertex
            || buffers[0].array_stride != core::mem::size_of::<[f32; 3]>() as u64
            || attributes.len() != 1
            || attributes[0].shader_location != 0
            || attributes[0].format != wgpu::VertexFormat::Float32x3
            || attributes[0].offset != 0
        {
            return unsupported("vertex layout outside position3 package interface");
        }
        let pipeline = self
            .shared
            .device
            .create_render_pipeline(
                shader.shader,
                u32::try_from(buffers[0].array_stride).expect("VMX vertex stride"),
                u32::try_from(attributes[0].offset).expect("VMX position offset"),
            )
            .unwrap_or_else(|code| panic!("VMX render pipeline admission failed: {code}"));
        DispatchRenderPipeline::custom(VmxRenderPipeline {
            shared: Arc::clone(&self.shared),
            pipeline,
        })
    }
    fn create_mesh_pipeline(&self, _: &wgpu::MeshPipelineDescriptor<'_>) -> DispatchRenderPipeline {
        unsupported("mesh pipelines")
    }
    fn create_compute_pipeline(
        &self,
        _: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> DispatchComputePipeline {
        unsupported("compute pipelines")
    }
    unsafe fn create_pipeline_cache(
        &self,
        _: &wgpu::PipelineCacheDescriptor<'_>,
    ) -> DispatchPipelineCache {
        unsupported("pipeline caches")
    }
    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        let supported_format = matches!(
            desc.format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        );
        if !supported_format
            || desc.dimension != wgpu::TextureDimension::D2
            || desc.size.width == 0
            || desc.size.height == 0
            || desc.size.depth_or_array_layers == 0
            || desc.mip_level_count != 1
            || desc.sample_count != 1
            || !desc.view_formats.is_empty()
        {
            return unsupported("texture outside linear RGBA8 VMX contract");
        }
        let bytes_per_pixel = 4u32;
        let Some(bytes_per_row) = desc.size.width.checked_mul(bytes_per_pixel) else {
            return unsupported("texture row size overflow");
        };
        let Some(bytes) = u64::from(bytes_per_row)
            .checked_mul(u64::from(desc.size.height))
            .and_then(|bytes| bytes.checked_mul(u64::from(desc.size.depth_or_array_layers)))
            .and_then(|bytes| usize::try_from(bytes).ok())
        else {
            return unsupported("texture allocation overflow");
        };
        let buffer = self
            .shared
            .device
            .create_buffer(bytes, LINEAR_TEXTURE_BACKING_USAGE)
            .unwrap_or_else(|code| {
                panic!(
                    "VMX vGPU texture backing creation failed: {code}; label={:?} extent={}x{}x{} bytes={bytes}",
                    desc.label,
                    desc.size.width,
                    desc.size.height,
                    desc.size.depth_or_array_layers,
                )
            });
        DispatchTexture::custom(VmxTexture {
            storage: Arc::new(VmxTextureStorage {
                shared: Arc::clone(&self.shared),
                backing: VmxTextureBacking::Linear {
                    buffer,
                    info: LinearTextureInfo {
                        width: desc.size.width,
                        height: desc.size.height,
                        layers: desc.size.depth_or_array_layers,
                        bytes_per_pixel,
                        bytes_per_row,
                    },
                    destroyed: AtomicBool::new(false),
                },
            }),
        })
    }
    fn create_external_texture(
        &self,
        _: &wgpu::ExternalTextureDescriptor<'_>,
        _: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        unsupported("external textures")
    }
    fn create_blas(
        &self,
        _: &wgpu::CreateBlasDescriptor<'_>,
        _: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        unsupported("BLAS")
    }
    fn create_tlas(&self, _: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        unsupported("TLAS")
    }
    fn create_sampler(&self, desc: &wgpu::SamplerDescriptor<'_>) -> DispatchSampler {
        if desc.compare.is_some()
            || desc.anisotropy_clamp != 1
            || !desc.lod_min_clamp.is_finite()
            || !desc.lod_max_clamp.is_finite()
            || desc.lod_min_clamp < 0.0
            || desc.lod_max_clamp < desc.lod_min_clamp
            || !matches!(
                desc.address_mode_u,
                wgpu::AddressMode::ClampToEdge | wgpu::AddressMode::Repeat
            )
            || !matches!(
                desc.address_mode_v,
                wgpu::AddressMode::ClampToEdge | wgpu::AddressMode::Repeat
            )
            || !matches!(
                desc.address_mode_w,
                wgpu::AddressMode::ClampToEdge | wgpu::AddressMode::Repeat
            )
        {
            return unsupported("sampler outside baseline repeat/clamp contract");
        }
        DispatchSampler::custom(VmxSampler)
    }
    fn create_query_set(&self, _: &wgpu::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        unsupported("query sets")
    }
    fn create_command_encoder(
        &self,
        _: &wgpu::CommandEncoderDescriptor<'_>,
    ) -> DispatchCommandEncoder {
        DispatchCommandEncoder::custom(VmxCommandEncoder {
            commands: Arc::new(Mutex::new(Vec::new())),
        })
    }
    fn create_render_bundle_encoder(
        &self,
        _: &wgpu::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        unsupported("render bundles")
    }

    fn set_device_lost_callback(&self, _callback: BoxDeviceLostCallback) {}
    fn on_uncaptured_error(&self, _handler: Arc<dyn wgpu::UncapturedErrorHandler>) {}
    fn push_error_scope(&self, _filter: wgpu::ErrorFilter) -> u32 {
        0
    }
    fn pop_error_scope(&self, _index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        Box::pin(std::future::ready(None))
    }
    unsafe fn start_graphics_debugger_capture(&self) {}
    unsafe fn stop_graphics_debugger_capture(&self) {}
    fn poll(&self, _poll: wgpu::wgt::PollType<u64>) -> Result<wgpu::PollStatus, wgpu::PollError> {
        Ok(wgpu::PollStatus::QueueEmpty)
    }
    fn get_internal_counters(&self) -> wgpu::InternalCounters {
        wgpu::InternalCounters::default()
    }
    fn generate_allocator_report(&self) -> Option<wgpu::AllocatorReport> {
        None
    }
    fn destroy(&self) {}
}

impl QueueInterface for VmxQueue {
    fn write_buffer(&self, buffer: &DispatchBuffer, offset: wgpu::BufferAddress, data: &[u8]) {
        let buffer = buffer
            .as_custom::<VmxBuffer>()
            .expect("VMX queue received a foreign WGPU buffer");
        if let Err(code) = self
            .shared
            .device
            .write_buffer(buffer.buffer, offset as usize, data)
        {
            self.shared.record_error(code);
        }
    }

    fn create_staging_buffer(&self, _size: wgpu::BufferSize) -> Option<DispatchQueueWriteBuffer> {
        None
    }
    fn validate_write_buffer(
        &self,
        _buffer: &DispatchBuffer,
        _offset: wgpu::BufferAddress,
        _size: wgpu::BufferSize,
    ) -> Option<()> {
        Some(())
    }
    fn write_staging_buffer(
        &self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: &DispatchQueueWriteBuffer,
    ) {
        unsupported::<()>("staging buffers")
    }
    fn write_texture(
        &self,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        let Some(texture) = destination.texture.as_custom::<VmxTexture>() else {
            return unsupported::<()>("foreign texture write");
        };
        let VmxTextureBacking::Linear {
            buffer,
            info,
            destroyed,
        } = &texture.storage.backing
        else {
            return unsupported::<()>("writes to imported UI4 texture");
        };
        if destroyed.load(Ordering::Acquire)
            || destination.mip_level != 0
            || destination.aspect != wgpu::TextureAspect::All
            || size.width == 0
            || size.height == 0
            || size.depth_or_array_layers == 0
            || destination.origin.x.saturating_add(size.width) > info.width
            || destination.origin.y.saturating_add(size.height) > info.height
            || destination
                .origin
                .z
                .saturating_add(size.depth_or_array_layers)
                > info.layers
        {
            self.shared.record_error(ERR_INVALID_ARGUMENT);
            return;
        }
        let source_row_bytes = size.width.saturating_mul(info.bytes_per_pixel);
        let source_pitch = layout.bytes_per_row.unwrap_or(source_row_bytes);
        let source_rows = layout.rows_per_image.unwrap_or(size.height);
        if source_pitch < source_row_bytes || source_rows < size.height {
            self.shared.record_error(ERR_INVALID_ARGUMENT);
            return;
        }
        for layer in 0..size.depth_or_array_layers {
            for row in 0..size.height {
                let source_offset = layout.offset
                    + u64::from(layer) * u64::from(source_rows) * u64::from(source_pitch)
                    + u64::from(row) * u64::from(source_pitch);
                let destination_offset = (u64::from(destination.origin.z + layer)
                    * u64::from(info.height)
                    + u64::from(destination.origin.y + row))
                    * u64::from(info.bytes_per_row)
                    + u64::from(destination.origin.x) * u64::from(info.bytes_per_pixel);
                let Ok(source_offset) = usize::try_from(source_offset) else {
                    self.shared.record_error(ERR_INVALID_ARGUMENT);
                    return;
                };
                let Some(source_end) = source_offset.checked_add(source_row_bytes as usize) else {
                    self.shared.record_error(ERR_INVALID_ARGUMENT);
                    return;
                };
                let Some(source) = data.get(source_offset..source_end) else {
                    self.shared.record_error(ERR_INVALID_ARGUMENT);
                    return;
                };
                if let Err(code) =
                    self.shared
                        .device
                        .write_buffer(*buffer, destination_offset as usize, source)
                {
                    self.shared.record_error(code);
                    return;
                }
            }
        }
    }
    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let mut timeline = 0;
        for command_buffer in command_buffers {
            let command_buffer = command_buffer
                .as_custom::<VmxCommandBuffer>()
                .expect("VMX queue received a foreign command buffer");
            let commands = core::mem::take(
                &mut *command_buffer
                    .commands
                    .lock()
                    .expect("VMX command buffer mutex"),
            );
            for command in commands {
                match command {
                    VmxCommand::Clear {
                        surface,
                        rgba8_srgb,
                    } => {
                        let target = surface
                            .lock()
                            .expect("VMX surface mutex")
                            .take()
                            .expect("VMX command targeted a consumed UI4 surface");
                        match self
                            .shared
                            .device
                            .submit_ui4_clear(self.queue, target, rgba8_srgb)
                        {
                            Ok(point) => timeline = timeline.max(point.value),
                            Err(code) => self.shared.record_error(code),
                        }
                    }
                    VmxCommand::Indexed {
                        surface,
                        pipeline,
                        vertex,
                        vertex_offset,
                        index,
                        index_offset,
                        first_index,
                        index_count,
                        clear_rgba8_srgb,
                    } => {
                        let target = surface
                            .lock()
                            .expect("VMX surface mutex")
                            .take()
                            .expect("VMX draw targeted a consumed UI4 surface");
                        let pipeline = pipeline
                            .as_custom::<VmxRenderPipeline>()
                            .expect("VMX draw pipeline");
                        let vertex = vertex.as_custom::<VmxBuffer>().expect("VMX vertex buffer");
                        let index = index.as_custom::<VmxBuffer>().expect("VMX index buffer");
                        let descriptor = IndexedDraw {
                            vertex_offset,
                            index_offset,
                            index_count,
                            first_index,
                            base_vertex: 0,
                            clear_rgba8_srgb,
                            ..IndexedDraw::default()
                        };
                        match self.shared.device.submit_ui4_indexed(
                            self.queue,
                            target,
                            pipeline.pipeline,
                            vertex.buffer,
                            index.buffer,
                            descriptor,
                        ) {
                            Ok(point) => timeline = timeline.max(point.value),
                            Err(code) => self.shared.record_error(code),
                        }
                    }
                }
            }
        }
        self.shared
            .last_submission
            .store(timeline, Ordering::Release);
        timeline
    }
    fn get_timestamp_period(&self) -> f32 {
        1.0
    }
    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        callback();
    }
    fn compact_blas(&self, _blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        unsupported("BLAS compaction")
    }
    fn present(&self, _detail: &DispatchSurfaceOutputDetail) {
        unsupported::<()>("presentation")
    }
}

impl CommandBufferInterface for VmxCommandBuffer {}

impl CommandEncoderInterface for VmxCommandEncoder {
    fn copy_buffer_to_buffer(
        &self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: Option<wgpu::BufferAddress>,
    ) {
        unsupported::<()>("buffer copies")
    }
    fn copy_buffer_to_texture(
        &self,
        _: wgpu::TexelCopyBufferInfo<'_>,
        _: wgpu::TexelCopyTextureInfo<'_>,
        _: wgpu::Extent3d,
    ) {
        unsupported::<()>("buffer-to-texture copies")
    }
    fn copy_texture_to_buffer(
        &self,
        _: wgpu::TexelCopyTextureInfo<'_>,
        _: wgpu::TexelCopyBufferInfo<'_>,
        _: wgpu::Extent3d,
    ) {
        unsupported::<()>("texture-to-buffer copies")
    }
    fn copy_texture_to_texture(
        &self,
        _: wgpu::TexelCopyTextureInfo<'_>,
        _: wgpu::TexelCopyTextureInfo<'_>,
        _: wgpu::Extent3d,
    ) {
        unsupported::<()>("texture copies")
    }
    fn begin_compute_pass(&self, _: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        unsupported("compute passes")
    }
    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        if desc.depth_stencil_attachment.is_some()
            || desc.timestamp_writes.is_some()
            || desc.occlusion_query_set.is_some()
            || desc.color_attachments.len() != 1
        {
            unsupported::<()>("non-clear render attachments");
        }
        let attachment = desc.color_attachments[0]
            .as_ref()
            .expect("VMX clear pass requires one color attachment");
        if attachment.resolve_target.is_some()
            || attachment.depth_slice.is_some()
            || attachment.ops.store != wgpu::StoreOp::Store
        {
            unsupported::<()>("resolved, sliced, or discarded clear attachments");
        }
        let wgpu::LoadOp::Clear(color) = attachment.ops.load else {
            return unsupported("render-pass load without a pipeline");
        };
        let target = attachment
            .view
            .as_custom::<VmxTextureView>()
            .expect("VMX render pass received a foreign texture view");
        let VmxTextureBacking::Ui4 { surface, .. } = &target.storage.backing else {
            return unsupported("render pass target is not an imported UI4 surface");
        };
        DispatchRenderPass::custom(VmxRenderPass {
            commands: Arc::clone(&self.commands),
            surface: Arc::new(Mutex::new(
                surface.lock().expect("VMX surface mutex").take(),
            )),
            clear_rgba8_srgb: opaque_clear_rgba8_srgb(color),
            pipeline: None,
            vertex: None,
            index: None,
            emitted: false,
        })
    }
    fn finish(&mut self) -> DispatchCommandBuffer {
        DispatchCommandBuffer::custom(VmxCommandBuffer {
            commands: Mutex::new(core::mem::take(
                &mut *self.commands.lock().expect("VMX command encoder mutex"),
            )),
        })
    }
    fn clear_texture(&self, _: &DispatchTexture, _: &wgpu::ImageSubresourceRange) {
        unsupported::<()>("texture clears")
    }
    fn clear_buffer(
        &self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: Option<wgpu::BufferAddress>,
    ) {
        unsupported::<()>("buffer clears")
    }
    fn insert_debug_marker(&self, _: &str) {}
    fn push_debug_group(&self, _: &str) {}
    fn pop_debug_group(&self) {}
    fn write_timestamp(&self, _: &DispatchQuerySet, _: u32) {
        unsupported::<()>("timestamps")
    }
    fn resolve_query_set(
        &self,
        _: &DispatchQuerySet,
        _: u32,
        _: u32,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
    ) {
        unsupported::<()>("query resolution")
    }
    fn mark_acceleration_structures_built<'a>(
        &self,
        _: &mut dyn Iterator<Item = &'a wgpu::Blas>,
        _: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        unsupported::<()>("acceleration structures")
    }
    fn build_acceleration_structures<'a>(
        &self,
        _: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        _: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        unsupported::<()>("acceleration structures")
    }
    fn transition_resources<'a>(
        &mut self,
        _: &mut dyn Iterator<Item = wgpu::wgt::BufferTransition<&'a DispatchBuffer>>,
        _: &mut dyn Iterator<Item = wgpu::wgt::TextureTransition<&'a DispatchTexture>>,
    ) {
    }
}

impl RenderPassInterface for VmxRenderPass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.pipeline = Some(pipeline.clone());
    }
    fn set_bind_group(
        &mut self,
        _index: u32,
        _bind_group: Option<&DispatchBindGroup>,
        _dynamic_offsets: &[wgpu::DynamicOffset],
    ) {
        unsupported::<()>("constant-RGBA package exposes no bind groups");
    }
    fn set_index_buffer(
        &mut self,
        buffer: &DispatchBuffer,
        format: wgpu::IndexFormat,
        offset: wgpu::BufferAddress,
        _: Option<wgpu::BufferSize>,
    ) {
        if format != wgpu::IndexFormat::Uint32 {
            unsupported::<()>("non-u32 index buffer");
        }
        self.index = Some((buffer.clone(), offset));
    }
    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: Option<&DispatchBuffer>,
        offset: wgpu::BufferAddress,
        _: Option<wgpu::BufferSize>,
    ) {
        if slot != 0 {
            unsupported::<()>("vertex buffer slot other than zero");
        }
        self.vertex = buffer.map(|buffer| (buffer.clone(), offset));
    }
    fn set_immediates(&mut self, _: u32, _: &[u8]) {
        unsupported::<()>("immediates")
    }
    fn set_blend_constant(&mut self, _: wgpu::Color) {}
    fn set_scissor_rect(&mut self, _: u32, _: u32, _: u32, _: u32) {}
    fn set_viewport(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn set_stencil_reference(&mut self, _: u32) {}
    fn draw(&mut self, _: core::ops::Range<u32>, _: core::ops::Range<u32>) {
        unsupported::<()>("draw")
    }
    fn draw_indexed(
        &mut self,
        indices: core::ops::Range<u32>,
        base_vertex: i32,
        instances: core::ops::Range<u32>,
    ) {
        if self.emitted || indices.is_empty() || base_vertex != 0 || instances != (0..1) {
            unsupported::<()>("indexed draw range outside frontier contract");
        }
        let pipeline = self
            .pipeline
            .take()
            .expect("draw_indexed requires a pipeline");
        let (vertex, vertex_offset) = self
            .vertex
            .take()
            .expect("draw_indexed requires vertex buffer 0");
        let (index, index_offset) = self
            .index
            .take()
            .expect("draw_indexed requires an index buffer");
        self.commands
            .lock()
            .expect("VMX command encoder mutex")
            .push(VmxCommand::Indexed {
                surface: Arc::clone(&self.surface),
                pipeline,
                vertex,
                vertex_offset,
                index,
                index_offset,
                first_index: indices.start,
                index_count: indices.end - indices.start,
                clear_rgba8_srgb: self.clear_rgba8_srgb,
            });
        self.emitted = true;
    }
    fn draw_mesh_tasks(&mut self, _: u32, _: u32, _: u32) {
        unsupported::<()>("mesh draw")
    }
    fn draw_indirect(&mut self, _: &DispatchBuffer, _: wgpu::BufferAddress) {
        unsupported::<()>("indirect draw")
    }
    fn draw_indexed_indirect(&mut self, _: &DispatchBuffer, _: wgpu::BufferAddress) {
        unsupported::<()>("indexed indirect draw")
    }
    fn draw_mesh_tasks_indirect(&mut self, _: &DispatchBuffer, _: wgpu::BufferAddress) {
        unsupported::<()>("mesh indirect draw")
    }
    fn multi_draw_indirect(&mut self, _: &DispatchBuffer, _: wgpu::BufferAddress, _: u32) {
        unsupported::<()>("multi draw")
    }
    fn multi_draw_indexed_indirect(&mut self, _: &DispatchBuffer, _: wgpu::BufferAddress, _: u32) {
        unsupported::<()>("multi indexed draw")
    }
    fn multi_draw_indirect_count(
        &mut self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: u32,
    ) {
        unsupported::<()>("counted multi draw")
    }
    fn multi_draw_mesh_tasks_indirect(
        &mut self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: u32,
    ) {
        unsupported::<()>("multi mesh draw")
    }
    fn multi_draw_indexed_indirect_count(
        &mut self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: u32,
    ) {
        unsupported::<()>("counted indexed draw")
    }
    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: &DispatchBuffer,
        _: wgpu::BufferAddress,
        _: u32,
    ) {
        unsupported::<()>("counted mesh draw")
    }
    fn insert_debug_marker(&mut self, _: &str) {}
    fn push_debug_group(&mut self, _: &str) {}
    fn pop_debug_group(&mut self) {}
    fn write_timestamp(&mut self, _: &DispatchQuerySet, _: u32) {
        unsupported::<()>("timestamps")
    }
    fn begin_occlusion_query(&mut self, _: u32) {
        unsupported::<()>("occlusion queries")
    }
    fn end_occlusion_query(&mut self) {
        unsupported::<()>("occlusion queries")
    }
    fn begin_pipeline_statistics_query(&mut self, _: &DispatchQuerySet, _: u32) {
        unsupported::<()>("pipeline statistics")
    }
    fn end_pipeline_statistics_query(&mut self) {
        unsupported::<()>("pipeline statistics")
    }
    fn execute_bundles(&mut self, _: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        unsupported::<()>("render bundles")
    }
}

fn opaque_clear_rgba8_srgb(color: wgpu::Color) -> u32 {
    if color.a != 1.0 || !color.r.is_finite() || !color.g.is_finite() || !color.b.is_finite() {
        return unsupported("non-opaque or non-finite UI4 clear color");
    }
    fn encode(channel: f64) -> u8 {
        let linear = channel.clamp(0.0, 1.0);
        let srgb = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (srgb * 255.0 + 0.5) as u8
    }
    u32::from_le_bytes([encode(color.r), encode(color.g), encode(color.b), 255])
}

fn vmx_buffer_usage(usage: wgpu::BufferUsages) -> u32 {
    let mut out = 0;
    if usage.contains(wgpu::BufferUsages::MAP_READ) {
        out |= BUFFER_USAGE_MAP_READ;
    }
    if usage.contains(wgpu::BufferUsages::MAP_WRITE) {
        out |= BUFFER_USAGE_MAP_WRITE;
    }
    // The VMX v1 broker implements WGPU queue uploads/readback through its
    // bounded CPU transfer calls. Grant those internal capabilities whenever
    // the public WGPU buffer permits the corresponding copy direction; this
    // does not expose mapping through WGPU's public Buffer API.
    if usage.contains(wgpu::BufferUsages::COPY_SRC) {
        out |= BUFFER_USAGE_COPY_SRC | BUFFER_USAGE_MAP_READ;
    }
    if usage.contains(wgpu::BufferUsages::COPY_DST) {
        out |= BUFFER_USAGE_COPY_DST | BUFFER_USAGE_MAP_WRITE;
    }
    if usage.intersects(
        wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::INDEX
            | wgpu::BufferUsages::INDIRECT,
    ) {
        out |= BUFFER_USAGE_STORAGE;
    }
    if usage.contains(wgpu::BufferUsages::VERTEX) {
        out |= BUFFER_USAGE_VERTEX;
    }
    if usage.contains(wgpu::BufferUsages::INDEX) {
        out |= BUFFER_USAGE_INDEX;
    }
    out
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn unsupported<T>(operation: &'static str) -> T {
    panic!("VMX WGPU backend operation is not implemented yet: {operation}")
}

const fn fail(stage: &'static str, code: i32) -> BackendFailure {
    BackendFailure { stage, code }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_wgsl_digest_matches_the_public_vgpu_contract() {
        assert_eq!(
            fnv1a64(VOXEL_SHADER_WGSL.as_bytes()),
            AUTHENTICATED_SHADER_DIGEST,
        );
    }

    #[test]
    fn linear_texture_copy_destination_carries_internal_upload_authority() {
        assert_ne!(LINEAR_TEXTURE_BACKING_USAGE & BUFFER_USAGE_COPY_DST, 0);
        assert_ne!(LINEAR_TEXTURE_BACKING_USAGE & BUFFER_USAGE_MAP_WRITE, 0);
    }
}
