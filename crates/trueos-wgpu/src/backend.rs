#![allow(unused_variables)]
#![cfg_attr(not(target_os = "trueos"), allow(dead_code))]
use std::{
    fmt,
    ops::Range,
    pin::Pin,
    sync::{Arc, Mutex},
};
use v::vgpu;
use wgpu::custom::*;
use wgpu::{Blas, Tlas};

const VOXY_WGSL: &str = include_str!("voxy_headless.wgsl");
const MAX_BUFFER_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FRAME_VERTICES: usize = 780_000;

#[derive(Clone, Copy, Debug)]
pub struct ShaderPackage {
    pub wgsl: &'static str,
    pub digest: u64,
}
impl ShaderPackage {
    pub const fn new(wgsl: &'static str, digest: u64) -> Self {
        Self { wgsl, digest }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub code: i32,
    pub operation: &'static str,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TRUEOS GPU {} failed ({})", self.operation, self.code)
    }
}
impl std::error::Error for Error {}
fn native<T>(operation: &'static str, result: Result<T, i32>) -> Result<T, Error> {
    result.map_err(|code| Error { code, operation })
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Debug)]
struct Native {
    device: Option<vgpu::Device>,
    queue: Option<vgpu::Queue>,
    package: ShaderPackage,
    error: Mutex<Option<Error>>,
    serial: Mutex<u64>,
    buffer_bytes: Mutex<u64>,
    submit_lock: Mutex<()>,
}
impl Drop for Native {
    fn drop(&mut self) {
        #[cfg(target_os = "trueos")]
        if let Some(device) = self.device {
            if let Some(queue) = self.queue {
                let _ = device.destroy_queue(queue);
            }
            let _ = device.close();
        }
    }
}

#[derive(Debug)]
pub struct Context {
    native: Arc<Native>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}
impl Context {
    pub fn open_with_package(package: ShaderPackage) -> Result<Self, Error> {
        if package.digest != vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64
            || package.wgsl != VOXY_WGSL
            || fnv(package.wgsl.as_bytes()) != package.digest
        {
            return Err(Error {
                code: vgpu::ERR_UNSUPPORTED,
                operation: "shader package",
            });
        }
        #[cfg(not(target_os = "trueos"))]
        return Err(Error {
            code: vgpu::ERR_NO_DEVICE,
            operation: "open (requires TRUEOS)",
        });
        #[cfg(target_os = "trueos")]
        {
            let device = native(
                "open",
                vgpu::Device::open(
                    vgpu::Capabilities::BUFFER
                        .union(vgpu::Capabilities::QUEUE)
                        .union(vgpu::Capabilities::TIMELINE)
                        .union(vgpu::Capabilities::RENDER)
                        .union(vgpu::Capabilities::PRESENT),
                ),
            )?;
            let shader = match native(
                "shader admission",
                device.create_shader_module(package.digest),
            ) {
                Ok(shader) => shader,
                Err(error) => {
                    let _ = device.close();
                    return Err(error);
                }
            };
            if let Err(error) = native("shader release", device.destroy_shader_module(shader)) {
                let _ = device.close();
                return Err(error);
            }
            let queue = match native("queue", device.create_queue(vgpu::QueueClass::Render)) {
                Ok(queue) => queue,
                Err(error) => {
                    let _ = device.close();
                    return Err(error);
                }
            };
            let native = Arc::new(Native {
                device: Some(device),
                queue: Some(queue),
                package,
                error: Mutex::new(None),
                serial: Mutex::new(0),
                buffer_bytes: Mutex::new(0),
                submit_lock: Mutex::new(()),
            });
            Ok(Self {
                device: wgpu::Device::from_custom(Device(native.clone())),
                queue: wgpu::Queue::from_custom(Queue(native.clone())),
                native,
            })
        }
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn acquire_frame(&self, window_id: u32) -> Result<wgpu::Texture, Error> {
        self.wait()?;
        #[cfg(not(target_os = "trueos"))]
        return Err(Error {
            code: vgpu::ERR_NO_DEVICE,
            operation: "surface acquire",
        });
        #[cfg(target_os = "trueos")]
        {
            let surface = native(
                "surface acquire",
                self.native.device.unwrap().acquire_ui4_surface(window_id),
            )?;
            let info = surface.info();
            Ok(wgpu::Texture::from_custom(Texture {
                native: self.native.clone(),
                size: wgpu::Extent3d {
                    width: info.width,
                    height: info.height,
                    depth_or_array_layers: 1,
                },
                kind: TextureKind::Frame(Arc::new(Mutex::new(Some(surface)))),
                destroyed: Arc::new(Mutex::new(false)),
            }))
        }
    }
    pub fn wait(&self) -> Result<(), Error> {
        if let Some(error) = self.native.error.lock().unwrap().clone() {
            Err(error)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
struct Device(Arc<Native>);
#[derive(Clone, Debug)]
struct Queue(Arc<Native>);
#[derive(Clone, Debug)]
struct Shader(Arc<Native>);
#[derive(Clone, Debug)]
struct Layout(Arc<Native>);
#[derive(Clone, Debug)]
struct Bind {
    native: Arc<Native>,
    camera: Arc<BufferData>,
    range: Range<usize>,
}
#[derive(Clone, Debug)]
struct Buffer(Arc<BufferData>);
#[derive(Debug)]
struct BufferData {
    native: Arc<Native>,
    bytes: Mutex<Vec<u8>>,
    usage: wgpu::BufferUsages,
    size: u64,
    destroyed: Mutex<bool>,
}
#[derive(Clone, Debug)]
struct Pipeline(Arc<PipelineData>);
#[derive(Debug)]
struct PipelineData {
    native: Arc<Native>,
    shader: Option<vgpu::ShaderModule>,
    pipeline: Option<vgpu::RenderPipeline>,
}
impl Drop for BufferData {
    fn drop(&mut self) {
        *self.native.buffer_bytes.lock().unwrap() -= self.size;
    }
}
impl Drop for PipelineData {
    fn drop(&mut self) {
        #[cfg(target_os = "trueos")]
        {
            if let Some(pipeline) = self.pipeline {
                let _ = self
                    .native
                    .device
                    .unwrap()
                    .destroy_render_pipeline(pipeline);
            }
            if let Some(shader) = self.shader {
                let _ = self.native.device.unwrap().destroy_shader_module(shader);
            }
        }
    }
}
#[cfg(target_os = "trueos")]
type FrameLease = vgpu::Ui4Surface;
#[cfg(not(target_os = "trueos"))]
type FrameLease = ();
#[derive(Clone, Debug)]
enum TextureKind {
    Frame(Arc<Mutex<Option<FrameLease>>>),
    Depth,
}
#[derive(Clone, Debug)]
struct Texture {
    native: Arc<Native>,
    size: wgpu::Extent3d,
    kind: TextureKind,
    destroyed: Arc<Mutex<bool>>,
}
#[derive(Clone, Debug)]
struct View(Texture);
#[derive(Debug)]
struct Encoder {
    native: Arc<Native>,
    state: Arc<Mutex<EncoderState>>,
}
#[derive(Debug, Default)]
struct EncoderState {
    active: bool,
    finished: bool,
    passes: Vec<PassData>,
}
#[derive(Debug)]
struct Commands {
    native: Arc<Native>,
    passes: Mutex<Option<Vec<PassData>>>,
}
impl CommandBufferInterface for Commands {}
#[derive(Clone, Debug)]
struct Draw {
    pipeline: Arc<PipelineData>,
    bind: Bind,
    buffer: Arc<BufferData>,
    slice: Range<usize>,
    vertices: Range<u32>,
    indices: Option<(Arc<BufferData>, Range<usize>, wgpu::IndexFormat, i32)>,
}
#[derive(Debug)]
struct PassData {
    target: Texture,
    clear: wgpu::Color,
    draws: Vec<Draw>,
}
#[derive(Debug)]
struct Pass {
    native: Arc<Native>,
    encoder: Arc<Mutex<EncoderState>>,
    data: Option<PassData>,
    pipeline: Option<Arc<PipelineData>>,
    bind: Option<Bind>,
    vertex: Option<(Arc<BufferData>, Range<usize>)>,
    index: Option<(Arc<BufferData>, Range<usize>, wgpu::IndexFormat)>,
}
impl Drop for Pass {
    fn drop(&mut self) {
        let mut encoder = self.encoder.lock().unwrap();
        encoder.active = false;
        if !std::thread::panicking() {
            encoder.passes.push(self.data.take().unwrap());
        }
    }
}
impl Pass {
    fn record(
        &mut self,
        vertices: Range<u32>,
        instances: Range<u32>,
        indexed: bool,
        base_vertex: i32,
    ) {
        assert!(
            instances == (0..1),
            "TRUEOS wgpu: instancing is unsupported"
        );
        let pipeline = self
            .pipeline
            .as_ref()
            .expect("TRUEOS wgpu: missing pipeline")
            .clone();
        let bind = self
            .bind
            .as_ref()
            .expect("TRUEOS wgpu: missing camera binding")
            .clone();
        let (buffer, slice) = self
            .vertex
            .as_ref()
            .expect("TRUEOS wgpu: missing vertex buffer");
        assert!(
            vertices.start <= vertices.end && (vertices.end - vertices.start) % 3 == 0,
            "TRUEOS wgpu: incomplete triangle list"
        );
        let indices = if indexed {
            let (b, range, format) = self
                .index
                .as_ref()
                .expect("TRUEOS wgpu: missing index buffer");
            Some((b.clone(), range.clone(), *format, base_vertex))
        } else {
            assert!(
                usize::try_from(vertices.end)
                    .unwrap()
                    .checked_mul(32)
                    .is_some_and(|end| end <= slice.len()),
                "TRUEOS wgpu: vertex range exceeds buffer"
            );
            None
        };
        assert!(
            self.data.as_ref().unwrap().draws.len() < 64,
            "TRUEOS wgpu: too many draws in one pass"
        );
        self.data.as_mut().unwrap().draws.push(Draw {
            pipeline,
            bind,
            buffer: buffer.clone(),
            slice: slice.clone(),
            vertices,
            indices,
        });
    }
}
fn frame_vertices_fit(current: usize, next: usize) -> bool {
    current
        .checked_add(next)
        .is_some_and(|count| count <= MAX_FRAME_VERTICES)
}

fn owned(native: &Arc<Native>, other: &Arc<Native>) {
    assert!(
        Arc::ptr_eq(native, other),
        "TRUEOS wgpu: resource belongs to another device"
    );
}
fn limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_buffer_size = MAX_BUFFER_BYTES;
    limits.max_texture_dimension_1d = 0;
    limits.max_texture_dimension_2d = 4096;
    limits.max_texture_dimension_3d = 0;
    limits.max_texture_array_layers = 1;
    limits.max_bind_groups = 1;
    limits.max_bind_groups_plus_vertex_buffers = 2;
    limits.max_bindings_per_bind_group = 1;
    limits.max_dynamic_uniform_buffers_per_pipeline_layout = 0;
    limits.max_dynamic_storage_buffers_per_pipeline_layout = 0;
    limits.max_sampled_textures_per_shader_stage = 0;
    limits.max_samplers_per_shader_stage = 0;
    limits.max_storage_buffers_per_shader_stage = 0;
    limits.max_storage_buffers_in_vertex_stage = 0;
    limits.max_storage_buffers_in_fragment_stage = 0;
    limits.max_storage_textures_per_shader_stage = 0;
    limits.max_storage_textures_in_vertex_stage = 0;
    limits.max_storage_textures_in_fragment_stage = 0;
    limits.max_uniform_buffers_per_shader_stage = 1;
    limits.max_uniform_buffer_binding_size = 80;
    limits.max_storage_buffer_binding_size = 0;
    limits.max_vertex_buffers = 1;
    limits.max_vertex_attributes = 2;
    limits.max_vertex_buffer_array_stride = 32;
    limits.max_inter_stage_shader_variables = 1;
    limits.max_color_attachments = 1;
    limits.max_color_attachment_bytes_per_sample = 4;
    limits.max_compute_workgroup_storage_size = 0;
    limits.max_compute_invocations_per_workgroup = 0;
    limits.max_compute_workgroup_size_x = 0;
    limits.max_compute_workgroup_size_y = 0;
    limits.max_compute_workgroup_size_z = 0;
    limits.max_compute_workgroups_per_dimension = 0;
    limits
}
fn info() -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: "TRUEOS authenticated native renderer".into(),
        vendor: 0x8086,
        device: 0,
        device_type: wgpu::DeviceType::IntegratedGpu,
        driver: "TRUEOS".into(),
        driver_info: "bounded custom backend; native PCI device ID is not exposed".into(),
        ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Noop)
    }
}
fn checked_slice(buffer: &Arc<BufferData>, offset: u64, size: Option<u64>) -> Range<usize> {
    assert!(
        !*buffer.destroyed.lock().unwrap(),
        "TRUEOS wgpu: destroyed buffer"
    );
    let end = size.map_or(buffer.size, |size| {
        offset
            .checked_add(size)
            .expect("TRUEOS wgpu: buffer range overflow")
    });
    assert!(
        offset <= end && end <= buffer.size,
        "TRUEOS wgpu: buffer range exceeds allocation"
    );
    usize::try_from(offset).unwrap()..usize::try_from(end).unwrap()
}
fn unorm(value: f64) -> u32 {
    (value.clamp(0., 1.) * 255.).round() as u32
}
fn clear_word(color: wgpu::Color) -> u32 {
    unorm(color.r) | (unorm(color.g) << 8) | (unorm(color.b) << 16) | (unorm(color.a) << 24)
}

struct Scratch {
    native: Arc<Native>,
    vertex: vgpu::Buffer,
    index: vgpu::Buffer,
}
impl Drop for Scratch {
    fn drop(&mut self) {
        #[cfg(target_os = "trueos")]
        {
            let _ = self.native.device.unwrap().destroy_buffer(self.vertex);
            let _ = self.native.device.unwrap().destroy_buffer(self.index);
        }
    }
}
#[cfg(target_os = "trueos")]
fn submit_pass(native_gpu: &Arc<Native>, pass: PassData) -> Result<(), Error> {
    owned(native_gpu, &pass.target.native);
    assert!(
        !*pass.target.destroyed.lock().unwrap(),
        "TRUEOS wgpu: destroyed frame target"
    );
    let TextureKind::Frame(frame) = &pass.target.kind else {
        panic!("TRUEOS wgpu: color target is not a frame");
    };
    let surface = frame
        .lock()
        .unwrap()
        .take()
        .expect("TRUEOS wgpu: frame already submitted or discarded");
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut camera: Option<Vec<u8>> = None;
    let mut pipeline: Option<Arc<PipelineData>> = None;
    for draw in pass.draws {
        assert!(
            frame_vertices_fit(indices.len(), draw.vertices.len()),
            "TRUEOS wgpu: frame exceeds admitted geometry limit"
        );
        owned(native_gpu, &draw.pipeline.native);
        assert!(
            !*draw.buffer.destroyed.lock().unwrap() && !*draw.bind.camera.destroyed.lock().unwrap(),
            "TRUEOS wgpu: destroyed draw resource"
        );
        let current_camera =
            draw.bind.camera.bytes.lock().unwrap()[draw.bind.range.clone()].to_vec();
        if let Some(camera) = &camera {
            assert_eq!(
                camera, &current_camera,
                "TRUEOS wgpu: one pass requires one camera"
            );
        } else {
            camera = Some(current_camera);
        }
        if let Some(pipeline) = &pipeline {
            assert!(
                Arc::ptr_eq(pipeline, &draw.pipeline),
                "TRUEOS wgpu: one pass requires one pipeline"
            );
        } else {
            pipeline = Some(draw.pipeline.clone());
        }
        let buffer = draw.buffer.bytes.lock().unwrap();
        let vertex_bytes = &buffer[draw.slice.clone()];
        let first = vertices.len() / 32;
        if let Some((index, range, format, base)) = draw.indices {
            let stride = if format == wgpu::IndexFormat::Uint16 {
                2
            } else {
                4
            };
            assert!(
                (draw.vertices.end as usize)
                    .checked_mul(stride)
                    .is_some_and(|end| end <= range.len()),
                "TRUEOS wgpu: index range exceeds buffer"
            );
            assert!(
                !*index.destroyed.lock().unwrap(),
                "TRUEOS wgpu: destroyed index buffer"
            );
            let bytes = index.bytes.lock().unwrap();
            let bytes = &bytes[range];
            let stride = if format == wgpu::IndexFormat::Uint16 {
                2
            } else {
                4
            };
            for index in draw.vertices {
                let at = usize::try_from(index)
                    .unwrap()
                    .checked_mul(stride)
                    .expect("TRUEOS wgpu: index overflow");
                assert!(
                    at.checked_add(stride).is_some_and(|end| end <= bytes.len()),
                    "TRUEOS wgpu: index range exceeds buffer"
                );
                let value = if stride == 2 {
                    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as u32
                } else {
                    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
                };
                let value = i64::from(value) + i64::from(base);
                assert!(
                    value >= 0
                        && usize::try_from(value)
                            .unwrap()
                            .checked_mul(32)
                            .and_then(|at| at.checked_add(32))
                            .is_some_and(|end| end <= vertex_bytes.len()),
                    "TRUEOS wgpu: indexed vertex exceeds buffer"
                );
                let at = usize::try_from(value).unwrap() * 32;
                vertices.extend_from_slice(&vertex_bytes[at..at + 32]);
                indices.push(u32::try_from(indices.len()).unwrap());
            }
        } else {
            let start = draw.vertices.start as usize * 32;
            let end = draw.vertices.end as usize * 32;
            vertices.extend_from_slice(&vertex_bytes[start..end]);
            for index in 0..(end - start) / 32 {
                indices.push(u32::try_from(first + index).unwrap());
            }
        }
        assert!(
            indices.len() <= MAX_FRAME_VERTICES,
            "TRUEOS wgpu: frame exceeds admitted geometry limit"
        );
    }
    let clear = clear_word(pass.clear);
    let point = if vertices.is_empty() {
        native(
            "clear submit",
            native_gpu
                .device
                .unwrap()
                .submit_ui4_clear(native_gpu.queue.unwrap(), surface, clear),
        )?
    } else {
        let mut upload = camera.expect("TRUEOS wgpu: missing camera");
        assert_eq!(upload.len(), 80);
        upload.extend_from_slice(&vertices);
        let vb = native(
            "vertex allocation",
            native_gpu.device.unwrap().create_buffer(
                upload.len(),
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_COPY_DST,
            ),
        )?;
        let ib = match native(
            "index allocation",
            native_gpu.device.unwrap().create_buffer(
                indices.len() * 4,
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_COPY_DST,
            ),
        ) {
            Ok(ib) => ib,
            Err(error) => {
                let _ = native_gpu.device.unwrap().destroy_buffer(vb);
                return Err(error);
            }
        };
        let scratch = Scratch {
            native: native_gpu.clone(),
            vertex: vb,
            index: ib,
        };
        let index_bytes: Vec<u8> = indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let written = native(
            "vertex upload",
            native_gpu.device.unwrap().write_buffer(vb, 0, &upload),
        )?;
        if written != upload.len() {
            return Err(Error {
                code: vgpu::ERR_IO,
                operation: "short vertex upload",
            });
        }
        let written = native(
            "index upload",
            native_gpu.device.unwrap().write_buffer(ib, 0, &index_bytes),
        )?;
        if written != index_bytes.len() {
            return Err(Error {
                code: vgpu::ERR_IO,
                operation: "short index upload",
            });
        }
        let draw = vgpu::IndexedDraw {
            vertex_offset: 80,
            index_count: indices.len() as u32,
            clear_rgba8_srgb: clear,
            topology: 0,
            texture_reserved: vgpu::INDEXED_DRAW_DRAWABLE_DEPTH
                | vgpu::INDEXED_DRAW_DEPTH_TEST
                | vgpu::INDEXED_DRAW_DEPTH_WRITE
                | vgpu::INDEXED_DRAW_CLEAR_DEPTH
                | (3 << vgpu::INDEXED_DRAW_DEPTH_COMPARE_SHIFT),
            ..Default::default()
        };
        for vertex in vertices.chunks_exact(32) {
            assert_eq!(
                f32::from_le_bytes(vertex[28..32].try_into().unwrap()),
                1.,
                "TRUEOS wgpu: only opaque vertex colors are supported"
            );
        }
        let point = native(
            "indexed submit",
            native_gpu.device.unwrap().submit_ui4_indexed(
                native_gpu.queue.unwrap(),
                surface,
                pipeline.unwrap().pipeline.unwrap(),
                vb,
                ib,
                draw,
            ),
        )?;
        if let Err(error) = native(
            "completion",
            native_gpu
                .device
                .unwrap()
                .wait(native_gpu.queue.unwrap(), point.value),
        ) {
            std::mem::forget(scratch);
            return Err(error);
        }
        return Ok(());
    };
    native(
        "completion",
        native_gpu
            .device
            .unwrap()
            .wait(native_gpu.queue.unwrap(), point.value),
    )?;
    Ok(())
}

impl DeviceInterface for Device {
    fn features(&self) -> wgpu::Features {
        wgpu::Features::empty()
    }
    fn limits(&self) -> wgpu::Limits {
        limits()
    }
    fn adapter_info(&self) -> wgpu::AdapterInfo {
        info()
    }
    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        shader_bound_checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        assert!(
            matches!(&desc.source, wgpu::ShaderSource::Wgsl(source) if source.as_ref() == self.0.package.wgsl),
            "TRUEOS wgpu: unadmitted shader source"
        );
        DispatchShaderModule::custom(Shader(self.0.clone()))
    }
    unsafe fn create_shader_module_passthrough(
        &self,
        desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        panic!("TRUEOS wgpu: create_shader_module_passthrough is unsupported")
    }
    fn create_bind_group_layout(
        &self,
        desc: &wgpu::BindGroupLayoutDescriptor<'_>,
    ) -> DispatchBindGroupLayout {
        assert!(
            desc.entries.len() == 1
                && desc.entries[0].binding == 0
                && desc.entries[0].visibility == wgpu::ShaderStages::VERTEX
                && desc.entries[0].count.is_none()
                && matches!(desc.entries[0].ty, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size } if min_binding_size.is_none_or(|size| size.get() == 80)),
            "TRUEOS wgpu: unsupported binding layout"
        );
        DispatchBindGroupLayout::custom(Layout(self.0.clone()))
    }
    fn create_bind_group(&self, desc: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        let layout = desc
            .layout
            .as_custom::<Layout>()
            .expect("TRUEOS wgpu: foreign binding layout");
        owned(&self.0, &layout.0);
        assert!(
            desc.entries.len() == 1 && desc.entries[0].binding == 0,
            "TRUEOS wgpu: expected camera binding zero"
        );
        let wgpu::BindingResource::Buffer(binding) = &desc.entries[0].resource else {
            panic!("TRUEOS wgpu: expected uniform buffer");
        };
        let buffer = binding
            .buffer
            .as_custom::<Buffer>()
            .expect("TRUEOS wgpu: foreign camera buffer");
        owned(&self.0, &buffer.0.native);
        assert!(
            buffer.0.usage.contains(wgpu::BufferUsages::UNIFORM) && binding.offset % 256 == 0,
            "TRUEOS wgpu: invalid uniform binding"
        );
        let range = checked_slice(
            &buffer.0,
            binding.offset,
            binding.size.map(|size| size.get()),
        );
        assert_eq!(range.len(), 80, "TRUEOS wgpu: camera must contain 80 bytes");
        DispatchBindGroup::custom(Bind {
            native: self.0.clone(),
            camera: buffer.0.clone(),
            range,
        })
    }
    fn create_pipeline_layout(
        &self,
        desc: &wgpu::PipelineLayoutDescriptor<'_>,
    ) -> DispatchPipelineLayout {
        panic!("TRUEOS wgpu: create_pipeline_layout is unsupported")
    }
    fn create_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        assert!(
            desc.layout.is_none() && desc.cache.is_none() && desc.multiview_mask.is_none(),
            "TRUEOS wgpu: unsupported pipeline layout"
        );
        let shader = desc
            .vertex
            .module
            .as_custom::<Shader>()
            .expect("TRUEOS wgpu: foreign shader");
        owned(&self.0, &shader.0);
        assert!(
            desc.vertex.entry_point == Some("vs_main")
                && desc.vertex.compilation_options.constants.is_empty()
                && desc.vertex.buffers.len() == 1,
            "TRUEOS wgpu: unsupported vertex stage"
        );
        let buffer = desc.vertex.buffers[0]
            .as_ref()
            .expect("TRUEOS wgpu: missing vertex layout");
        assert!(
            buffer.array_stride == 32
                && buffer.step_mode == wgpu::VertexStepMode::Vertex
                && buffer.attributes.len() == 2
                && buffer.attributes[0]
                    == wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0
                    }
                && buffer.attributes[1]
                    == wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 1
                    },
            "TRUEOS wgpu: unsupported vertex layout"
        );
        let fragment = desc
            .fragment
            .as_ref()
            .expect("TRUEOS wgpu: missing fragment stage");
        let fragment_shader = fragment
            .module
            .as_custom::<Shader>()
            .expect("TRUEOS wgpu: foreign fragment shader");
        owned(&self.0, &fragment_shader.0);
        assert!(
            fragment.entry_point == Some("fs_main")
                && fragment.compilation_options.constants.is_empty()
                && fragment.targets.len() == 1
                && fragment.targets[0]
                    .as_ref()
                    .is_some_and(|target| target.format == wgpu::TextureFormat::Rgba8Unorm
                        && target.blend.is_none()
                        && target.write_mask == wgpu::ColorWrites::ALL),
            "TRUEOS wgpu: unsupported fragment stage"
        );
        assert!(
            desc.primitive == wgpu::PrimitiveState::default()
                && desc.multisample == wgpu::MultisampleState::default(),
            "TRUEOS wgpu: unsupported primitive or multisampling state"
        );
        let depth = desc
            .depth_stencil
            .as_ref()
            .expect("TRUEOS wgpu: missing depth state");
        assert!(
            depth.format == wgpu::TextureFormat::Depth32Float
                && depth.depth_write_enabled == Some(true)
                && depth.depth_compare == Some(wgpu::CompareFunction::LessEqual)
                && depth.stencil == Default::default()
                && depth.bias == Default::default(),
            "TRUEOS wgpu: unsupported depth state"
        );
        let (shader, pipeline) = admit_pipeline(&self.0).unwrap_or_else(|error| panic!("{error}"));
        DispatchRenderPipeline::custom(Pipeline(Arc::new(PipelineData {
            native: self.0.clone(),
            shader: Some(shader),
            pipeline: Some(pipeline),
        })))
    }
    fn create_mesh_pipeline(
        &self,
        desc: &wgpu::MeshPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        panic!("TRUEOS wgpu: create_mesh_pipeline is unsupported")
    }
    fn create_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> DispatchComputePipeline {
        panic!("TRUEOS wgpu: create_compute_pipeline is unsupported")
    }
    unsafe fn create_pipeline_cache(
        &self,
        desc: &wgpu::PipelineCacheDescriptor<'_>,
    ) -> DispatchPipelineCache {
        panic!("TRUEOS wgpu: create_pipeline_cache is unsupported")
    }
    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        let supported = wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::INDEX
            | wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST;
        assert!(
            desc.size > 0
                && desc.size <= MAX_BUFFER_BYTES
                && desc.size % 4 == 0
                && !desc.mapped_at_creation
                && supported.contains(desc.usage)
                && !desc.usage.is_empty(),
            "TRUEOS wgpu: unsupported buffer descriptor"
        );
        let mut live_bytes = self.0.buffer_bytes.lock().unwrap();
        let next = live_bytes
            .checked_add(desc.size)
            .filter(|value| *value <= 128 * 1024 * 1024)
            .expect("TRUEOS wgpu: buffer memory budget exceeded");
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(desc.size as usize)
            .expect("TRUEOS wgpu: buffer allocation failed");
        bytes.resize(desc.size as usize, 0);
        *live_bytes = next;
        drop(live_bytes);
        DispatchBuffer::custom(Buffer(Arc::new(BufferData {
            native: self.0.clone(),
            bytes: Mutex::new(bytes),
            usage: desc.usage,
            size: desc.size,
            destroyed: Mutex::new(false),
        })))
    }
    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        assert!(
            desc.format == wgpu::TextureFormat::Depth32Float
                && desc.dimension == wgpu::TextureDimension::D2
                && desc.mip_level_count == 1
                && desc.sample_count == 1
                && desc.size.depth_or_array_layers == 1
                && desc.size.width > 0
                && desc.size.height > 0
                && desc.size.width <= 4096
                && desc.size.height <= 4096
                && desc.usage == wgpu::TextureUsages::RENDER_ATTACHMENT
                && desc.view_formats.is_empty(),
            "TRUEOS wgpu: only a single Depth32Float attachment is supported"
        );
        DispatchTexture::custom(Texture {
            native: self.0.clone(),
            size: desc.size,
            kind: TextureKind::Depth,
            destroyed: Arc::new(Mutex::new(false)),
        })
    }
    fn create_external_texture(
        &self,
        desc: &wgpu::ExternalTextureDescriptor<'_>,
        planes: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        panic!("TRUEOS wgpu: create_external_texture is unsupported")
    }
    fn create_blas(
        &self,
        desc: &wgpu::CreateBlasDescriptor<'_>,
        sizes: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        panic!("TRUEOS wgpu: create_blas is unsupported")
    }
    fn create_tlas(&self, desc: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        panic!("TRUEOS wgpu: create_tlas is unsupported")
    }
    fn create_sampler(&self, desc: &wgpu::SamplerDescriptor<'_>) -> DispatchSampler {
        panic!("TRUEOS wgpu: create_sampler is unsupported")
    }
    fn create_query_set(&self, desc: &wgpu::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        panic!("TRUEOS wgpu: create_query_set is unsupported")
    }
    fn create_command_encoder(
        &self,
        desc: &wgpu::CommandEncoderDescriptor<'_>,
    ) -> DispatchCommandEncoder {
        DispatchCommandEncoder::custom(Encoder {
            native: self.0.clone(),
            state: Arc::new(Mutex::new(EncoderState::default())),
        })
    }
    fn create_render_bundle_encoder(
        &self,
        desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        panic!("TRUEOS wgpu: create_render_bundle_encoder is unsupported")
    }
    fn set_device_lost_callback(&self, device_lost_callback: BoxDeviceLostCallback) {
        panic!("TRUEOS wgpu: set_device_lost_callback is unsupported")
    }
    fn on_uncaptured_error(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        panic!("TRUEOS wgpu: on_uncaptured_error is unsupported")
    }
    fn push_error_scope(&self, filter: wgpu::ErrorFilter) -> u32 {
        panic!("TRUEOS wgpu: push_error_scope is unsupported")
    }
    fn pop_error_scope(&self, index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        panic!("TRUEOS wgpu: pop_error_scope is unsupported")
    }
    unsafe fn start_graphics_debugger_capture(&self) {
        panic!("TRUEOS wgpu: start_graphics_debugger_capture is unsupported")
    }
    unsafe fn stop_graphics_debugger_capture(&self) {
        panic!("TRUEOS wgpu: stop_graphics_debugger_capture is unsupported")
    }
    fn poll(
        &self,
        poll_type: wgpu::wgt::PollType<u64>,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        if self.0.error.lock().unwrap().is_some() {
            Err(wgpu::PollError::Timeout)
        } else {
            Ok(wgpu::PollStatus::QueueEmpty)
        }
    }
    fn get_internal_counters(&self) -> wgpu::InternalCounters {
        Default::default()
    }
    fn generate_allocator_report(&self) -> Option<wgpu::AllocatorReport> {
        None
    }
    fn destroy(&self) {
        panic!("TRUEOS wgpu: destroy is unsupported")
    }
}

impl QueueInterface for Queue {
    fn write_buffer(&self, buffer: &DispatchBuffer, offset: wgpu::BufferAddress, data: &[u8]) {
        let _queue = self.0.submit_lock.lock().unwrap();
        let buffer = buffer
            .as_custom::<Buffer>()
            .expect("TRUEOS wgpu: foreign buffer");
        owned(&self.0, &buffer.0.native);
        assert!(
            buffer.0.usage.contains(wgpu::BufferUsages::COPY_DST)
                && offset % 4 == 0
                && data.len() % 4 == 0,
            "TRUEOS wgpu: invalid buffer upload"
        );
        let range = checked_slice(&buffer.0, offset, Some(data.len() as u64));
        buffer.0.bytes.lock().unwrap()[range].copy_from_slice(data);
    }
    fn create_staging_buffer(&self, size: wgpu::BufferSize) -> Option<DispatchQueueWriteBuffer> {
        panic!("TRUEOS wgpu: create_staging_buffer is unsupported")
    }
    fn validate_write_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        size: wgpu::BufferSize,
    ) -> Option<()> {
        panic!("TRUEOS wgpu: validate_write_buffer is unsupported")
    }
    fn write_staging_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        staging_buffer: DispatchQueueWriteBuffer,
    ) {
        panic!("TRUEOS wgpu: write_staging_buffer is unsupported")
    }
    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        panic!("TRUEOS wgpu: write_texture is unsupported")
    }
    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let buffers: Vec<_> = command_buffers.collect();
        let _submit = self.0.submit_lock.lock().unwrap();
        assert!(
            self.0.error.lock().unwrap().is_none(),
            "TRUEOS wgpu: device has a failed submission"
        );
        let mut passes = Vec::new();
        for buffer in buffers {
            let buffer = buffer
                .as_custom::<Commands>()
                .expect("TRUEOS wgpu: foreign command buffer");
            owned(&self.0, &buffer.native);
            passes.extend(
                buffer
                    .passes
                    .lock()
                    .unwrap()
                    .take()
                    .expect("TRUEOS wgpu: command buffer already submitted"),
            );
        }
        assert!(
            passes.len() == 1,
            "TRUEOS wgpu: submit requires exactly one render pass"
        );
        if let Err(error) = submit_pass(&self.0, passes.pop().unwrap()) {
            self.0.error.lock().unwrap().get_or_insert(error);
        }
        let mut serial = self.0.serial.lock().unwrap();
        *serial += 1;
        *serial
    }
    fn get_timestamp_period(&self) -> f32 {
        0.
    }
    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        assert!(
            self.0.error.lock().unwrap().is_none(),
            "TRUEOS wgpu: submission failed"
        );
        callback();
    }
    fn compact_blas(&self, blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        panic!("TRUEOS wgpu: compact_blas is unsupported")
    }
    fn present(&self, detail: &DispatchSurfaceOutputDetail) {
        panic!("TRUEOS wgpu: present is unsupported")
    }
}

impl BufferInterface for Buffer {
    fn map_async(
        &self,
        mode: wgpu::MapMode,
        range: Range<wgpu::BufferAddress>,
        callback: BufferMapCallback,
    ) {
        callback(Err(wgpu::BufferAsyncError));
    }
    fn get_mapped_range(
        &self,
        sub_range: Range<wgpu::BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, wgpu::MapRangeError> {
        panic!("TRUEOS wgpu: get_mapped_range is unsupported")
    }
    fn unmap(&self) {
        panic!("TRUEOS wgpu: unmap is unsupported")
    }
    fn destroy(&self) {
        *self.0.destroyed.lock().unwrap() = true;
    }
    fn size(&self) -> wgpu::BufferAddress {
        self.0.size
    }
    fn usage(&self) -> wgpu::BufferUsages {
        self.0.usage
    }
}

impl TextureInterface for Texture {
    fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        assert!(
            !*self.destroyed.lock().unwrap(),
            "TRUEOS wgpu: destroyed texture"
        );
        assert!(
            desc.format.is_none_or(|format| format == self.format())
                && desc
                    .dimension
                    .is_none_or(|dimension| dimension == wgpu::TextureViewDimension::D2)
                && desc.base_mip_level == 0
                && desc.mip_level_count.is_none_or(|count| count == 1)
                && desc.base_array_layer == 0
                && desc.array_layer_count.is_none_or(|count| count == 1)
                && desc
                    .usage
                    .is_none_or(|usage| usage == wgpu::TextureUsages::RENDER_ATTACHMENT)
                && desc.aspect == wgpu::TextureAspect::All
                && desc.swizzle == wgpu::TextureComponentSwizzle::default(),
            "TRUEOS wgpu: unsupported texture view"
        );
        DispatchTextureView::custom(View(self.clone()))
    }
    fn destroy(&self) {
        *self.destroyed.lock().unwrap() = true;
        if let TextureKind::Frame(frame) = &self.kind {
            frame.lock().unwrap().take();
        }
    }
    fn size(&self) -> wgpu::Extent3d {
        self.size
    }
    fn mip_level_count(&self) -> u32 {
        1
    }
    fn sample_count(&self) -> u32 {
        1
    }
    fn dimension(&self) -> wgpu::TextureDimension {
        wgpu::TextureDimension::D2
    }
    fn format(&self) -> wgpu::TextureFormat {
        if matches!(self.kind, TextureKind::Depth) {
            wgpu::TextureFormat::Depth32Float
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }
    }
    fn usage(&self) -> wgpu::TextureUsages {
        wgpu::TextureUsages::RENDER_ATTACHMENT
    }
}

impl CommandEncoderInterface for Encoder {
    fn copy_buffer_to_buffer(
        &self,
        source: &DispatchBuffer,
        source_offset: wgpu::BufferAddress,
        destination: &DispatchBuffer,
        destination_offset: wgpu::BufferAddress,
        copy_size: Option<wgpu::BufferAddress>,
    ) {
        panic!("TRUEOS wgpu: copy_buffer_to_buffer is unsupported")
    }
    fn copy_buffer_to_texture(
        &self,
        source: wgpu::TexelCopyBufferInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        panic!("TRUEOS wgpu: copy_buffer_to_texture is unsupported")
    }
    fn copy_texture_to_buffer(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyBufferInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        panic!("TRUEOS wgpu: copy_texture_to_buffer is unsupported")
    }
    fn copy_texture_to_texture(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        panic!("TRUEOS wgpu: copy_texture_to_texture is unsupported")
    }
    fn begin_compute_pass(&self, desc: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        panic!("TRUEOS wgpu: begin_compute_pass is unsupported")
    }
    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        let mut state = self.state.lock().unwrap();
        assert!(
            !state.active && !state.finished && state.passes.is_empty(),
            "TRUEOS wgpu: encoder requires one inactive pass"
        );
        assert!(
            desc.color_attachments.len() == 1
                && desc.timestamp_writes.is_none()
                && desc.occlusion_query_set.is_none()
                && desc.multiview_mask.is_none(),
            "TRUEOS wgpu: unsupported render pass"
        );
        let color = desc.color_attachments[0]
            .as_ref()
            .expect("TRUEOS wgpu: missing color attachment");
        let view = color
            .view
            .as_custom::<View>()
            .expect("TRUEOS wgpu: foreign color view");
        owned(&self.native, &view.0.native);
        assert!(
            matches!(view.0.kind, TextureKind::Frame(_))
                && color.resolve_target.is_none()
                && color.depth_slice.is_none()
                && color.ops.store == wgpu::StoreOp::Store,
            "TRUEOS wgpu: unsupported color attachment"
        );
        let wgpu::LoadOp::Clear(clear) = color.ops.load else {
            panic!("TRUEOS wgpu: color load is unsupported");
        };
        assert!(
            [clear.r, clear.g, clear.b, clear.a]
                .into_iter()
                .all(f64::is_finite)
                && clear.a == 1.,
            "TRUEOS wgpu: nonfinite clear color"
        );
        let depth = desc
            .depth_stencil_attachment
            .as_ref()
            .expect("TRUEOS wgpu: missing depth attachment");
        let depth_view = depth
            .view
            .as_custom::<View>()
            .expect("TRUEOS wgpu: foreign depth view");
        owned(&self.native, &depth_view.0.native);
        assert!(
            matches!(depth_view.0.kind, TextureKind::Depth)
                && !*depth_view.0.destroyed.lock().unwrap()
                && depth_view.0.size == view.0.size
                && depth
                    .depth_ops
                    .is_some_and(|ops| ops.load == wgpu::LoadOp::Clear(1.))
                && depth.stencil_ops.is_none(),
            "TRUEOS wgpu: unsupported depth attachment"
        );
        state.active = true;
        DispatchRenderPass::custom(Pass {
            native: self.native.clone(),
            encoder: self.state.clone(),
            data: Some(PassData {
                target: view.0.clone(),
                clear,
                draws: Vec::new(),
            }),
            pipeline: None,
            bind: None,
            vertex: None,
            index: None,
        })
    }
    fn finish(&mut self) -> DispatchCommandBuffer {
        let mut state = self.state.lock().unwrap();
        assert!(
            !state.active && !state.finished,
            "TRUEOS wgpu: encoder active or finished"
        );
        state.finished = true;
        DispatchCommandBuffer::custom(Commands {
            native: self.native.clone(),
            passes: Mutex::new(Some(std::mem::take(&mut state.passes))),
        })
    }
    fn clear_texture(
        &self,
        texture: &DispatchTexture,
        subresource_range: &wgpu::ImageSubresourceRange,
    ) {
        panic!("TRUEOS wgpu: clear_texture is unsupported")
    }
    fn clear_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        panic!("TRUEOS wgpu: clear_buffer is unsupported")
    }
    fn insert_debug_marker(&self, label: &str) {}
    fn push_debug_group(&self, label: &str) {}
    fn pop_debug_group(&self) {}
    fn write_timestamp(&self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS wgpu: write_timestamp is unsupported")
    }
    fn resolve_query_set(
        &self,
        query_set: &DispatchQuerySet,
        first_query: u32,
        query_count: u32,
        destination: &DispatchBuffer,
        destination_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS wgpu: resolve_query_set is unsupported")
    }
    fn mark_acceleration_structures_built<'a>(
        &self,
        blas: &mut dyn Iterator<Item = &'a Blas>,
        tlas: &mut dyn Iterator<Item = &'a Tlas>,
    ) {
        panic!("TRUEOS wgpu: mark_acceleration_structures_built is unsupported")
    }
    fn build_acceleration_structures<'a>(
        &self,
        blas: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        tlas: &mut dyn Iterator<Item = &'a wgpu::Tlas>,
    ) {
        panic!("TRUEOS wgpu: build_acceleration_structures is unsupported")
    }
    fn transition_resources<'a>(
        &mut self,
        buffer_transitions: &mut dyn Iterator<Item = wgpu::BufferTransition<&'a DispatchBuffer>>,
        texture_transitions: &mut dyn Iterator<Item = wgpu::TextureTransition<&'a DispatchTexture>>,
    ) {
        panic!("TRUEOS wgpu: transition_resources is unsupported")
    }
}

impl RenderPassInterface for Pass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        let pipeline = pipeline
            .as_custom::<Pipeline>()
            .expect("TRUEOS wgpu: foreign pipeline");
        owned(&self.native, &pipeline.0.native);
        self.pipeline = Some(pipeline.0.clone());
    }
    fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: Option<&DispatchBindGroup>,
        offsets: &[wgpu::DynamicOffset],
    ) {
        assert!(
            index == 0 && offsets.is_empty(),
            "TRUEOS wgpu: dynamic bindings unsupported"
        );
        self.bind = bind_group.map(|bind| {
            let bind = bind
                .as_custom::<Bind>()
                .expect("TRUEOS wgpu: foreign bind group");
            owned(&self.native, &bind.native);
            bind.clone()
        });
    }
    fn set_index_buffer(
        &mut self,
        buffer: &DispatchBuffer,
        index_format: wgpu::IndexFormat,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        let buffer = buffer
            .as_custom::<Buffer>()
            .expect("TRUEOS wgpu: foreign index buffer");
        owned(&self.native, &buffer.0.native);
        let stride = if index_format == wgpu::IndexFormat::Uint16 {
            2
        } else {
            4
        };
        assert!(
            buffer.0.usage.contains(wgpu::BufferUsages::INDEX) && offset % stride == 0,
            "TRUEOS wgpu: invalid index binding"
        );
        let range = checked_slice(&buffer.0, offset, size);
        assert!(range.len() % stride as usize == 0);
        self.index = Some((buffer.0.clone(), range, index_format));
    }
    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: Option<&DispatchBuffer>,
        offset: wgpu::BufferAddress,
        size: Option<wgpu::BufferAddress>,
    ) {
        assert!(
            slot == 0 && offset % 32 == 0,
            "TRUEOS wgpu: unsupported vertex binding"
        );
        self.vertex = buffer.map(|buffer| {
            let buffer = buffer
                .as_custom::<Buffer>()
                .expect("TRUEOS wgpu: foreign vertex buffer");
            owned(&self.native, &buffer.0.native);
            assert!(buffer.0.usage.contains(wgpu::BufferUsages::VERTEX));
            let range = checked_slice(&buffer.0, offset, size);
            assert!(range.len() % 32 == 0, "TRUEOS wgpu: incomplete vertex");
            (buffer.0.clone(), range)
        });
    }
    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        panic!("TRUEOS wgpu: set_immediates is unsupported")
    }
    fn set_blend_constant(&mut self, color: wgpu::Color) {
        panic!("TRUEOS wgpu: set_blend_constant is unsupported")
    }
    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        panic!("TRUEOS wgpu: set_scissor_rect is unsupported")
    }
    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        panic!("TRUEOS wgpu: set_viewport is unsupported")
    }
    fn set_stencil_reference(&mut self, reference: u32) {
        panic!("TRUEOS wgpu: set_stencil_reference is unsupported")
    }
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.record(vertices, instances, false, 0);
    }
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.record(indices, instances, true, base_vertex);
    }
    fn draw_mesh_tasks(&mut self, group_count_x: u32, group_count_y: u32, group_count_z: u32) {
        panic!("TRUEOS wgpu: draw_mesh_tasks is unsupported")
    }
    fn draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS wgpu: draw_indirect is unsupported")
    }
    fn draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS wgpu: draw_indexed_indirect is unsupported")
    }
    fn draw_mesh_tasks_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
    ) {
        panic!("TRUEOS wgpu: draw_mesh_tasks_indirect is unsupported")
    }
    fn multi_draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_indirect is unsupported")
    }
    fn multi_draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_indexed_indirect is unsupported")
    }
    fn multi_draw_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_indirect_count is unsupported")
    }
    fn multi_draw_mesh_tasks_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_mesh_tasks_indirect is unsupported")
    }
    fn multi_draw_indexed_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_indexed_indirect_count is unsupported")
    }
    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: wgpu::BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: wgpu::BufferAddress,
        max_count: u32,
    ) {
        panic!("TRUEOS wgpu: multi_draw_mesh_tasks_indirect_count is unsupported")
    }
    fn insert_debug_marker(&mut self, label: &str) {}
    fn push_debug_group(&mut self, group_label: &str) {}
    fn pop_debug_group(&mut self) {}
    fn write_timestamp(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS wgpu: write_timestamp is unsupported")
    }
    fn begin_occlusion_query(&mut self, query_index: u32) {
        panic!("TRUEOS wgpu: begin_occlusion_query is unsupported")
    }
    fn end_occlusion_query(&mut self) {
        panic!("TRUEOS wgpu: end_occlusion_query is unsupported")
    }
    fn begin_pipeline_statistics_query(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        panic!("TRUEOS wgpu: begin_pipeline_statistics_query is unsupported")
    }
    fn end_pipeline_statistics_query(&mut self) {
        panic!("TRUEOS wgpu: end_pipeline_statistics_query is unsupported")
    }
    fn execute_bundles(&mut self, render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        panic!("TRUEOS wgpu: execute_bundles is unsupported")
    }
}

impl ShaderModuleInterface for Shader {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        Box::pin(std::future::ready(wgpu::CompilationInfo {
            messages: Vec::new(),
        }))
    }
}
impl BindGroupLayoutInterface for Layout {}
impl BindGroupInterface for Bind {}
impl TextureViewInterface for View {}
impl RenderPipelineInterface for Pipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        assert_eq!(index, 0, "TRUEOS wgpu: only camera group zero is supported");
        DispatchBindGroupLayout::custom(Layout(self.0.native.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "trueos"))]
    fn cpu_context() -> Context {
        let native = Arc::new(Native {
            device: None,
            queue: None,
            package: ShaderPackage::new(VOXY_WGSL, vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64),
            error: Mutex::new(None),
            serial: Mutex::new(0),
            buffer_bytes: Mutex::new(0),
            submit_lock: Mutex::new(()),
        });
        Context {
            device: wgpu::Device::from_custom(Device(native.clone())),
            queue: wgpu::Queue::from_custom(Queue(native.clone())),
            native,
        }
    }

    #[cfg(not(target_os = "trueos"))]
    fn camera_layout(context: &Context) -> wgpu::BindGroupLayout {
        context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
    }

    #[cfg(not(target_os = "trueos"))]
    fn camera(context: &Context, size: u64) -> wgpu::Buffer {
        context.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    #[test]
    fn geometry_budget_is_checked_before_copying() {
        assert!(frame_vertices_fit(600_000, 180_000));
        assert!(!frame_vertices_fit(600_000, 180_001));
        assert!(!frame_vertices_fit(usize::MAX, 1));
    }

    #[test]
    fn admitted_source_matches_kernel_identity() {
        assert_eq!(
            fnv(VOXY_WGSL.as_bytes()),
            vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn camera_binding_and_upload_use_public_wgpu_api() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let layout = camera_layout(&context);
        let _binding = context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                }],
            });
        context.queue().write_buffer(&camera, 0, &[5; 80]);
        assert_eq!(
            *camera
                .as_custom::<Buffer>()
                .unwrap()
                .0
                .bytes
                .lock()
                .unwrap(),
            [5; 80]
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "camera must contain 80 bytes")]
    fn wrong_uniform_size_is_rejected() {
        let context = cpu_context();
        let camera = camera(&context, 84);
        let layout = camera_layout(&context);
        let _ = context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                }],
            });
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "resource belongs to another device")]
    fn foreign_resource_is_rejected() {
        let first = cpu_context();
        let second = cpu_context();
        let camera = camera(&first, 80);
        second.queue().write_buffer(&camera, 0, &[0; 80]);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "buffer range exceeds allocation")]
    fn out_of_bounds_upload_is_rejected() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        context.queue().write_buffer(&camera, 76, &[0; 8]);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "destroyed buffer")]
    fn destroyed_buffer_cannot_be_uploaded() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        camera.destroy();
        context.queue().write_buffer(&camera, 0, &[0; 80]);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "unsupported vertex layout")]
    fn wrong_vertex_stride_is_rejected_before_native_admission() {
        let context = cpu_context();
        let shader = context
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(VOXY_WGSL.into()),
            });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let _ = context
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                    compilation_options: Default::default(),
                },
                fragment: None,
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "unadmitted shader source")]
    fn shader_module_requires_exact_source_bytes() {
        let context = cpu_context();
        let _ = context
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl("@compute fn unrelated() {}".into()),
            });
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn native_submission_error_remains_visible() {
        let context = cpu_context();
        *context.native.error.lock().unwrap() = Some(Error {
            code: vgpu::ERR_DEVICE_LOST,
            operation: "completion",
        });
        assert_eq!(context.wait().unwrap_err().code, vgpu::ERR_DEVICE_LOST);
        assert_eq!(context.wait().unwrap_err().code, vgpu::ERR_DEVICE_LOST);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn adapter_info_does_not_guess_the_native_device_id() {
        let context = cpu_context();
        let info = context.device().adapter_info();
        assert_eq!(info.device, 0);
        assert_eq!(info.vendor, 0x8086);
        assert!(
            info.driver_info
                .contains("native PCI device ID is not exposed")
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "begin_compute_pass is unsupported")]
    fn unsupported_compute_is_rejected() {
        let context = cpu_context();
        let mut encoder = context.device().create_command_encoder(&Default::default());
        encoder.begin_compute_pass(&Default::default());
    }
    #[test]
    fn clear_matches_linear_attachment_encoding() {
        assert_eq!(clear_word(wgpu::Color::BLACK), 0xff000000);
        assert_eq!(
            clear_word(wgpu::Color {
                r: 0.5,
                g: 0.,
                b: 1.,
                a: 1.
            }),
            0xffff0080
        );
    }
    #[test]
    fn wrong_source_cannot_bootstrap_native_device() {
        let error = Context::open_with_package(ShaderPackage::new(
            "untrusted",
            vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64,
        ))
        .unwrap_err();
        assert_eq!(error.code, vgpu::ERR_UNSUPPORTED);
    }
}

#[cfg(not(target_os = "trueos"))]
fn submit_pass(native: &Arc<Native>, pass: PassData) -> Result<(), Error> {
    Err(Error {
        code: vgpu::ERR_NO_DEVICE,
        operation: "submit",
    })
}
#[cfg(not(target_os = "trueos"))]
fn admit_pipeline(
    native: &Arc<Native>,
) -> Result<(vgpu::ShaderModule, vgpu::RenderPipeline), Error> {
    Err(Error {
        code: vgpu::ERR_NO_DEVICE,
        operation: "pipeline admission",
    })
}
#[cfg(target_os = "trueos")]
fn admit_pipeline(gpu: &Arc<Native>) -> Result<(vgpu::ShaderModule, vgpu::RenderPipeline), Error> {
    let device = gpu.device.unwrap();
    let shader = native(
        "shader admission",
        device.create_shader_module(gpu.package.digest),
    )?;
    match native(
        "pipeline admission",
        device.create_render_pipeline(shader, 32, 0),
    ) {
        Ok(pipeline) => Ok((shader, pipeline)),
        Err(error) => {
            let _ = device.destroy_shader_module(shader);
            Err(error)
        }
    }
}
