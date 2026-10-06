#![allow(unused_variables)]
#![cfg_attr(not(target_os = "trueos"), allow(dead_code))]
use std::{
    fmt,
    ops::Range,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use v::vgpu;
use wgpu::custom::*;
use wgpu::{Blas, Tlas};

const VOXY_WGSL: &str = include_str!("voxy_headless.wgsl");
const VOXY_TEXTURED_WGSL: &str = include_str!("voxy_headless_textured.wgsl");
const MAX_BUFFER_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FRAME_VERTICES: usize = 780_000;
const CAMERA_BYTES: usize = 80;
const VERTEX_STRIDE: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct ShaderPackage {
    pub wgsl: &'static str,
    pub digest: u64,
}
impl ShaderPackage {
    pub const fn new(wgsl: &'static str, digest: u64) -> Self {
        Self { wgsl, digest }
    }
    fn textured(self) -> bool {
        self.digest == vgpu::SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64
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
    pass_upload: Mutex<Option<PassUpload<vgpu::Buffer>>>,
}
impl Drop for Native {
    fn drop(&mut self) {
        #[cfg(target_os = "trueos")]
        if let Some(device) = self.device {
            if let Some(upload) = self
                .pass_upload
                .get_mut()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                release_pass_upload(&device, upload, self.error.get_mut().unwrap().is_some());
            }
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
        let source = if package.digest == vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64 {
            Some(VOXY_WGSL)
        } else if package.textured() {
            Some(VOXY_TEXTURED_WGSL)
        } else {
            None
        };
        if source != Some(package.wgsl) || fnv(package.wgsl.as_bytes()) != package.digest {
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
                pass_upload: Mutex::new(None),
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
struct Layout {
    native: Arc<Native>,
    textured: bool,
}
#[derive(Clone, Debug)]
struct Bind {
    native: Arc<Native>,
    camera: Arc<BufferData>,
    range: Range<usize>,
    atlas: Option<Arc<SampledTextureData>>,
}
#[derive(Clone, Debug)]
struct Sampler(Arc<Native>);
#[derive(Clone, Debug)]
struct Buffer(Arc<BufferData>);
#[derive(Debug)]
struct BufferData {
    native: Arc<Native>,
    bytes: Mutex<Vec<u8>>,
    usage: wgpu::BufferUsages,
    size: u64,
    destroyed: Mutex<bool>,
    revision: AtomicU64,
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
    Sampled(Arc<SampledTextureData>),
}
#[derive(Debug)]
struct AtlasPixels {
    bytes: Vec<u8>,
    revision: u64,
}
#[derive(Debug)]
struct SampledTextureData {
    native: Arc<Native>,
    pixels: Mutex<AtlasPixels>,
    upload: Mutex<Option<PixelUpload<vgpu::Buffer>>>,
    unknown_completion: AtomicBool,
    size: wgpu::Extent3d,
    destroyed: Arc<Mutex<bool>>,
}
impl Drop for SampledTextureData {
    fn drop(&mut self) {
        *self.native.buffer_bytes.lock().unwrap() -=
            u64::from(self.size.width) * u64::from(self.size.height) * 4;
        #[cfg(target_os = "trueos")]
        if !self.unknown_completion.load(Ordering::Acquire)
            && self.native.error.lock().unwrap().is_none()
        {
            if let Some(upload) = self.upload.get_mut().unwrap().take() {
                // Every submit is synchronous. The last reference can disappear
                // only after retirement; ambiguous failures retain the handle.
                let _ = self.native.device.unwrap().destroy_buffer(upload.buffer);
            }
        }
    }
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
    limits.max_bindings_per_bind_group = 3;
    limits.max_dynamic_uniform_buffers_per_pipeline_layout = 0;
    limits.max_dynamic_storage_buffers_per_pipeline_layout = 0;
    limits.max_sampled_textures_per_shader_stage = 1;
    limits.max_samplers_per_shader_stage = 1;
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

trait UploadDevice {
    type Buffer: Copy;
    fn create(&self, bytes: usize, usage: u32) -> Result<Self::Buffer, i32>;
    fn write_at(&self, buffer: Self::Buffer, offset: usize, bytes: &[u8]) -> Result<usize, i32>;
    fn write(&self, buffer: Self::Buffer, bytes: &[u8]) -> Result<usize, i32> {
        self.write_at(buffer, 0, bytes)
    }
    fn destroy(&self, buffer: Self::Buffer);
}

#[derive(Debug)]
struct PixelUpload<B> {
    buffer: B,
    revision: u64,
}

fn upload_pixels<D: UploadDevice>(
    device: &D,
    upload: &mut Option<PixelUpload<D::Buffer>>,
    revision: u64,
    pixels: &[u8],
) -> Result<D::Buffer, Error> {
    if let Some(upload) = upload.as_ref().filter(|upload| upload.revision == revision) {
        return Ok(upload.buffer);
    }
    let fresh = upload.is_none();
    let buffer = match upload.as_ref() {
        Some(upload) => upload.buffer,
        // The retained sampled-buffer contract intentionally uses MAP_WRITE
        // alone. wgpu COPY_DST is enforced on the logical texture, above CABI.
        None => native(
            "atlas allocation",
            device.create(pixels.len(), vgpu::BUFFER_USAGE_MAP_WRITE),
        )?,
    };
    let result = native("atlas upload", device.write(buffer, pixels)).and_then(|written| {
        if written == pixels.len() {
            Ok(())
        } else {
            Err(Error {
                code: vgpu::ERR_IO,
                operation: "atlas upload",
            })
        }
    });
    if let Err(error) = result {
        if fresh {
            device.destroy(buffer);
        }
        return Err(error);
    }
    *upload = Some(PixelUpload { buffer, revision });
    Ok(buffer)
}

fn binding_layout_entry(entry: &wgpu::BindGroupLayoutEntry, binding: u32) -> bool {
    if entry.binding != binding || entry.count.is_some() {
        return false;
    }
    match binding {
        0 => {
            entry.visibility == wgpu::ShaderStages::VERTEX
                && matches!(entry.ty, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size } if min_binding_size.is_none_or(|size| size.get() == 80))
        }
        1 => {
            entry.visibility == wgpu::ShaderStages::FRAGMENT
                && matches!(
                    entry.ty,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false
                    }
                )
        }
        2 => {
            entry.visibility == wgpu::ShaderStages::FRAGMENT
                && matches!(
                    entry.ty,
                    wgpu::BindingType::Sampler(
                        wgpu::SamplerBindingType::Filtering
                            | wgpu::SamplerBindingType::NonFiltering
                    )
                )
        }
        _ => false,
    }
}

fn binding_layout(native: &Arc<Native>) -> Layout {
    Layout {
        native: native.clone(),
        textured: native.package.textured(),
    }
}

fn write_atlas(atlas: &SampledTextureData, data: &[u8], layout: wgpu::TexelCopyBufferLayout) {
    let row = atlas.size.width as usize * 4;
    let height = atlas.size.height as usize;
    let pitch = layout.bytes_per_row.map_or_else(
        || {
            assert_eq!(
                height, 1,
                "TRUEOS wgpu: multi-row atlas upload needs bytes_per_row"
            );
            row
        },
        |pitch| pitch as usize,
    );
    assert!(
        pitch >= row
            && pitch % 4 == 0
            && layout
                .rows_per_image
                .is_none_or(|rows| rows >= atlas.size.height),
        "TRUEOS wgpu: invalid atlas row layout"
    );
    let offset = usize::try_from(layout.offset).expect("TRUEOS wgpu: atlas offset overflow");
    let end = (height - 1)
        .checked_mul(pitch)
        .and_then(|tail| offset.checked_add(tail))
        .and_then(|tail| tail.checked_add(row));
    assert!(
        end.is_some_and(|end| end <= data.len()),
        "TRUEOS wgpu: atlas upload exceeds data"
    );
    // Validate all rows before mutating any pixels or invalidating a revision.
    for y in 0..height {
        assert!(
            data[offset + y * pitch..offset + y * pitch + row]
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 255),
            "TRUEOS wgpu: only opaque atlas texels are supported"
        );
    }
    let mut pixels = atlas.pixels.lock().unwrap();
    if pixels.revision != 0
        && (0..height).all(|y| {
            pixels.bytes[y * row..(y + 1) * row]
                == data[offset + y * pitch..offset + y * pitch + row]
        })
    {
        return;
    }
    let next = pixels
        .revision
        .checked_add(1)
        .expect("TRUEOS wgpu: atlas revision overflow");
    for y in 0..height {
        pixels.bytes[y * row..(y + 1) * row]
            .copy_from_slice(&data[offset + y * pitch..offset + y * pitch + row]);
    }
    pixels.revision = next;
}

#[cfg(target_os = "trueos")]
impl UploadDevice for vgpu::Device {
    type Buffer = vgpu::Buffer;
    fn create(&self, bytes: usize, usage: u32) -> Result<Self::Buffer, i32> {
        self.create_buffer(bytes, usage)
    }
    fn write_at(&self, buffer: Self::Buffer, offset: usize, bytes: &[u8]) -> Result<usize, i32> {
        self.write_buffer(buffer, offset, bytes)
    }
    fn destroy(&self, buffer: Self::Buffer) {
        let _ = self.destroy_buffer(buffer);
    }
}

#[derive(Clone, Debug)]
struct CameraStamp {
    source: Weak<BufferData>,
    revision: u64,
    range: Range<usize>,
}
impl CameraStamp {
    fn matches(&self, other: &Self) -> bool {
        self.source.ptr_eq(&other.source)
            && self.revision == other.revision
            && self.range == other.range
    }
}
#[derive(Clone, Debug)]
struct IndexStamp {
    source: Weak<BufferData>,
    revision: u64,
    range: Range<usize>,
    format: wgpu::IndexFormat,
    base: i32,
}
impl IndexStamp {
    fn matches(&self, other: &Self) -> bool {
        self.source.ptr_eq(&other.source)
            && self.revision == other.revision
            && self.range == other.range
            && self.format == other.format
            && self.base == other.base
    }
}
#[derive(Clone, Debug)]
struct DrawStamp {
    source: Weak<BufferData>,
    revision: u64,
    slice: Range<usize>,
    vertices: Range<u32>,
    index: Option<IndexStamp>,
    destination: usize,
}
impl DrawStamp {
    fn matches(&self, other: &Self) -> bool {
        self.source.ptr_eq(&other.source)
            && self.revision == other.revision
            && self.slice == other.slice
            && self.vertices == other.vertices
            && self.destination == other.destination
            && match (&self.index, &other.index) {
                (None, None) => true,
                (Some(a), Some(b)) => a.matches(b),
                _ => false,
            }
    }
}
struct DrawSegment<'a> {
    draw: &'a Draw,
    stamp: DrawStamp,
}
struct PassSources<'a> {
    camera: Option<CameraStamp>,
    camera_bytes: [u8; CAMERA_BYTES],
    segments: Vec<DrawSegment<'a>>,
    vertex_count: usize,
    pipeline: Option<Arc<PipelineData>>,
    atlas: Option<Arc<SampledTextureData>>,
}

// Weak source identities retain allocation identities without keeping logical
// buffers (which own Native) alive through Native's own upload cache.
#[derive(Debug)]
struct PassUpload<B> {
    vertex: B,
    index: B,
    capacity: usize,
    camera: Option<CameraStamp>,
    segments: Vec<DrawStamp>,
    unknown_completion: bool,
    retirements: u64,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct GeometryWrites {
    camera_bytes: usize,
    first_draw_bytes: usize,
    other_draw_bytes: usize,
}

fn release_pass_upload<D: UploadDevice>(
    device: &D,
    upload: PassUpload<D::Buffer>,
    failed: bool,
) -> bool {
    if failed || upload.unknown_completion {
        return false;
    }
    device.destroy(upload.vertex);
    device.destroy(upload.index);
    true
}

fn write_native_range<D: UploadDevice>(
    device: &D,
    buffer: D::Buffer,
    offset: usize,
    bytes: &[u8],
    operation: &'static str,
) -> Result<(), Error> {
    if bytes.is_empty() {
        return Ok(());
    }
    let written = native(operation, device.write_at(buffer, offset, bytes))?;
    if written != bytes.len() {
        return Err(Error {
            code: vgpu::ERR_IO,
            operation,
        });
    }
    Ok(())
}

fn create_pass_upload<D: UploadDevice>(
    device: &D,
    capacity: usize,
) -> Result<PassUpload<D::Buffer>, Error> {
    assert!(
        capacity > 0 && capacity <= MAX_FRAME_VERTICES,
        "TRUEOS wgpu: invalid geometry capacity"
    );
    let vertex_bytes = CAMERA_BYTES + capacity * VERTEX_STRIDE;
    let usage = vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_COPY_DST;
    let vertex = native(
        "vertex allocation",
        device.create(vertex_bytes, usage | vgpu::BUFFER_USAGE_VERTEX),
    )?;
    let index = match native(
        "index allocation",
        device.create(capacity * 4, usage | vgpu::BUFFER_USAGE_INDEX),
    ) {
        Ok(index) => index,
        Err(error) => {
            device.destroy(vertex);
            return Err(error);
        }
    };
    let initialized = (|| {
        let mut indices = Vec::new();
        indices.try_reserve_exact(capacity * 4).map_err(|_| Error {
            code: vgpu::ERR_OUT_OF_MEMORY,
            operation: "identity indices",
        })?;
        for index in 0..capacity as u32 {
            indices.extend_from_slice(&index.to_le_bytes());
        }
        write_native_range(device, index, 0, &indices, "index upload")
    })();
    if let Err(error) = initialized {
        device.destroy(vertex);
        device.destroy(index);
        return Err(error);
    }
    Ok(PassUpload {
        vertex,
        index,
        capacity,
        camera: None,
        segments: Vec::new(),
        unknown_completion: false,
        retirements: 0,
    })
}

fn pass_sources<'a>(native_gpu: &Arc<Native>, draws: &'a [Draw]) -> PassSources<'a> {
    let mut sources = PassSources {
        camera: None,
        camera_bytes: [0; CAMERA_BYTES],
        segments: Vec::new(),
        vertex_count: 0,
        pipeline: None,
        atlas: None,
    };
    for draw in draws {
        assert!(
            frame_vertices_fit(sources.vertex_count, draw.vertices.len()),
            "TRUEOS wgpu: frame exceeds admitted geometry limit"
        );
        owned(native_gpu, &draw.pipeline.native);
        owned(native_gpu, &draw.bind.native);
        owned(native_gpu, &draw.buffer.native);
        owned(native_gpu, &draw.bind.camera.native);
        assert!(
            !*draw.buffer.destroyed.lock().unwrap() && !*draw.bind.camera.destroyed.lock().unwrap(),
            "TRUEOS wgpu: destroyed draw resource"
        );
        assert!(
            draw.vertices.start <= draw.vertices.end && draw.vertices.len() % 3 == 0,
            "TRUEOS wgpu: incomplete triangle list"
        );
        let camera = CameraStamp {
            source: Arc::downgrade(&draw.bind.camera),
            revision: draw.bind.camera.revision.load(Ordering::Acquire),
            range: draw.bind.range.clone(),
        };
        if !sources
            .camera
            .as_ref()
            .is_some_and(|prior| prior.matches(&camera))
        {
            let bytes: [u8; CAMERA_BYTES] = draw.bind.camera.bytes.lock().unwrap()
                [draw.bind.range.clone()]
            .try_into()
            .expect("TRUEOS wgpu: camera must contain 80 bytes");
            if sources.camera.is_some() {
                assert_eq!(
                    sources.camera_bytes, bytes,
                    "TRUEOS wgpu: one pass requires one camera"
                );
            } else {
                sources.camera_bytes = bytes;
                sources.camera = Some(camera);
            }
        }
        if let Some(pipeline) = &sources.pipeline {
            assert!(
                Arc::ptr_eq(pipeline, &draw.pipeline),
                "TRUEOS wgpu: one pass requires one pipeline"
            );
        } else {
            sources.pipeline = Some(draw.pipeline.clone());
        }
        if native_gpu.package.textured() {
            let atlas = draw
                .bind
                .atlas
                .as_ref()
                .expect("TRUEOS wgpu: missing atlas binding");
            owned(native_gpu, &atlas.native);
            assert!(
                !*atlas.destroyed.lock().unwrap(),
                "TRUEOS wgpu: destroyed atlas"
            );
            assert!(
                atlas.pixels.lock().unwrap().revision != 0,
                "TRUEOS wgpu: atlas requires a complete opaque upload before drawing"
            );
            if let Some(prior) = &sources.atlas {
                assert!(
                    Arc::ptr_eq(prior, atlas),
                    "TRUEOS wgpu: one pass requires one atlas"
                );
            } else {
                sources.atlas = Some(atlas.clone());
            }
        } else {
            assert!(
                draw.bind.atlas.is_none(),
                "TRUEOS wgpu: color package does not sample textures"
            );
        }
        let index = if let Some((buffer, range, format, base)) = &draw.indices {
            owned(native_gpu, &buffer.native);
            assert!(
                !*buffer.destroyed.lock().unwrap(),
                "TRUEOS wgpu: destroyed index buffer"
            );
            let stride = if *format == wgpu::IndexFormat::Uint16 {
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
            Some(IndexStamp {
                source: Arc::downgrade(buffer),
                revision: buffer.revision.load(Ordering::Acquire),
                range: range.clone(),
                format: *format,
                base: *base,
            })
        } else {
            assert!(
                (draw.vertices.end as usize)
                    .checked_mul(VERTEX_STRIDE)
                    .is_some_and(|end| end <= draw.slice.len()),
                "TRUEOS wgpu: vertex range exceeds buffer"
            );
            None
        };
        sources.segments.push(DrawSegment {
            draw,
            stamp: DrawStamp {
                source: Arc::downgrade(&draw.buffer),
                revision: draw.buffer.revision.load(Ordering::Acquire),
                slice: draw.slice.clone(),
                vertices: draw.vertices.clone(),
                index,
                destination: CAMERA_BYTES + sources.vertex_count * VERTEX_STRIDE,
            },
        });
        sources.vertex_count += draw.vertices.len();
    }
    sources
}

fn materialize_segment(draw: &Draw) -> Vec<u8> {
    // Snapshot index bytes before locking the vertex data: WebGPU permits a
    // single logical buffer with both INDEX and VERTEX usages.
    let indexed = draw.indices.as_ref().map(|(buffer, range, format, base)| {
        let stride = if *format == wgpu::IndexFormat::Uint16 {
            2
        } else {
            4
        };
        let begin = range.start + draw.vertices.start as usize * stride;
        let end = range.start + draw.vertices.end as usize * stride;
        (
            buffer.bytes.lock().unwrap()[begin..end].to_vec(),
            stride,
            *base,
        )
    });
    let buffer = draw.buffer.bytes.lock().unwrap();
    let vertices = &buffer[draw.slice.clone()];
    let bytes = if let Some((indices, stride, base)) = indexed {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(draw.vertices.len() * VERTEX_STRIDE)
            .expect("TRUEOS wgpu: vertex allocation failed");
        for word in indices.chunks_exact(stride) {
            let index = if stride == 2 {
                u16::from_le_bytes(word.try_into().unwrap()) as u32
            } else {
                u32::from_le_bytes(word.try_into().unwrap())
            };
            let index = i64::from(index) + i64::from(base);
            assert!(
                index >= 0
                    && usize::try_from(index)
                        .unwrap()
                        .checked_mul(VERTEX_STRIDE)
                        .and_then(|at| at.checked_add(VERTEX_STRIDE))
                        .is_some_and(|end| end <= vertices.len()),
                "TRUEOS wgpu: indexed vertex exceeds buffer"
            );
            let at = index as usize * VERTEX_STRIDE;
            bytes.extend_from_slice(&vertices[at..at + VERTEX_STRIDE]);
        }
        bytes
    } else {
        vertices[draw.vertices.start as usize * VERTEX_STRIDE
            ..draw.vertices.end as usize * VERTEX_STRIDE]
            .to_vec()
    };
    for vertex in bytes.chunks_exact(VERTEX_STRIDE) {
        assert!(
            vertex
                .chunks_exact(4)
                .all(|word| f32::from_le_bytes(word.try_into().unwrap()).is_finite()),
            "TRUEOS wgpu: non-finite vertex"
        );
        assert_eq!(
            f32::from_le_bytes(vertex[28..32].try_into().unwrap()),
            1.,
            "TRUEOS wgpu: only opaque vertex colors are supported"
        );
    }
    bytes
}

fn update_pass_upload<D: UploadDevice>(
    device: &D,
    upload: &mut PassUpload<D::Buffer>,
    sources: &PassSources<'_>,
) -> Result<GeometryWrites, Error> {
    assert!(
        sources.vertex_count <= upload.capacity,
        "TRUEOS wgpu: frame exceeds native buffer capacity"
    );
    let mut writes = GeometryWrites::default();
    if let Some(camera) = &sources.camera {
        if !upload
            .camera
            .as_ref()
            .is_some_and(|prior| prior.matches(camera))
        {
            write_native_range(
                device,
                upload.vertex,
                0,
                &sources.camera_bytes,
                "camera upload",
            )?;
            writes.camera_bytes = CAMERA_BYTES;
        }
    }
    for (index, segment) in sources.segments.iter().enumerate() {
        if !upload
            .segments
            .get(index)
            .is_some_and(|prior| prior.matches(&segment.stamp))
        {
            let bytes = materialize_segment(segment.draw);
            write_native_range(
                device,
                upload.vertex,
                segment.stamp.destination,
                &bytes,
                "vertex upload",
            )?;
            if index == 0 {
                writes.first_draw_bytes += bytes.len();
            } else {
                writes.other_draw_bytes += bytes.len();
            }
        }
    }
    // Commit metadata only after every range write succeeds. A partial write
    // must never make a future pass skip the corresponding source data.
    upload.camera = sources.camera.clone();
    upload.segments = sources
        .segments
        .iter()
        .map(|segment| segment.stamp.clone())
        .collect();
    Ok(writes)
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
    let sources = pass_sources(native_gpu, &pass.draws);
    let device = native_gpu.device.unwrap();
    let queue = native_gpu.queue.unwrap();
    let clear = clear_word(pass.clear);
    if sources.vertex_count == 0 {
        let point = native(
            "clear submit",
            device.submit_ui4_clear(queue, surface, clear),
        )?;
        return native("completion", device.wait(queue, point.value));
    }
    let sampled_texture = if let Some(atlas) = &sources.atlas {
        let pixels = atlas.pixels.lock().unwrap();
        match upload_pixels(
            &device,
            &mut atlas.upload.lock().unwrap(),
            pixels.revision,
            &pixels.bytes,
        ) {
            Ok(buffer) => buffer.raw(),
            Err(error) => {
                atlas.unknown_completion.store(true, Ordering::Release);
                return Err(error);
            }
        }
    } else {
        0
    };
    let mut retained = native_gpu.pass_upload.lock().unwrap();
    let created = retained.is_none();
    if retained.is_none() {
        *retained = Some(create_pass_upload(&device, MAX_FRAME_VERTICES)?);
    }
    let upload = retained.as_mut().unwrap();
    let writes = update_pass_upload(&device, upload, &sources)?;
    let draw = vgpu::IndexedDraw {
        vertex_offset: CAMERA_BYTES as u64,
        index_count: sources.vertex_count as u32,
        clear_rgba8_srgb: clear,
        topology: 0,
        sampled_texture,
        texture_width: sources.atlas.as_ref().map_or(0, |atlas| atlas.size.width),
        texture_height: sources.atlas.as_ref().map_or(0, |atlas| atlas.size.height),
        texture_pitch: sources
            .atlas
            .as_ref()
            .map_or(0, |atlas| atlas.size.width * 4),
        sampler_flags: 0,
        texture_reserved: vgpu::INDEXED_DRAW_DRAWABLE_DEPTH
            | vgpu::INDEXED_DRAW_DEPTH_TEST
            | vgpu::INDEXED_DRAW_DEPTH_WRITE
            | vgpu::INDEXED_DRAW_CLEAR_DEPTH
            | (3 << vgpu::INDEXED_DRAW_DEPTH_COMPARE_SHIFT),
        ..Default::default()
    };
    // Keep both native handles until the exact synchronous completion. Mark
    // ambiguity before entering CABI, so unwinding cannot release live storage.
    upload.unknown_completion = true;
    if let Some(atlas) = &sources.atlas {
        atlas.unknown_completion.store(true, Ordering::Release);
    }
    let point = native(
        "indexed submit",
        device.submit_ui4_indexed(
            queue,
            surface,
            sources.pipeline.as_ref().unwrap().pipeline.unwrap(),
            upload.vertex,
            upload.index,
            draw,
        ),
    )?;
    native("completion", device.wait(queue, point.value))?;
    upload.unknown_completion = false;
    if let Some(atlas) = &sources.atlas {
        atlas.unknown_completion.store(false, Ordering::Release);
    }
    upload.retirements += 1;
    if upload.retirements <= 3 || upload.retirements % 128 == 0 {
        let _ = v::vsys::log_record(
            v::vsys::LOG_LEVEL_IMPORTANT,
            "apps::voxygen",
            &format!(
                "voxy-wgpu: phase=adapter-retired frame={} native_vb={} native_ib={} vertices={} camera_upload_bytes={} draw0_upload_bytes={} other_draw_upload_bytes={} index_upload_bytes={} storage=persistent-composite\n",
                upload.retirements,
                upload.vertex.raw(),
                upload.index.raw(),
                sources.vertex_count,
                writes.camera_bytes,
                writes.first_draw_bytes,
                writes.other_draw_bytes,
                if created { MAX_FRAME_VERTICES * 4 } else { 0 },
            ),
        );
    }
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
        let count = if self.0.package.textured() { 3 } else { 1 };
        assert!(
            desc.entries.len() == count
                && (0..count as u32).all(|binding| {
                    desc.entries
                        .iter()
                        .find(|entry| entry.binding == binding)
                        .is_some_and(|entry| binding_layout_entry(entry, binding))
                }),
            "TRUEOS wgpu: unsupported binding layout"
        );
        DispatchBindGroupLayout::custom(binding_layout(&self.0))
    }
    fn create_bind_group(&self, desc: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        let layout = desc
            .layout
            .as_custom::<Layout>()
            .expect("TRUEOS wgpu: foreign binding layout");
        owned(&self.0, &layout.native);
        let count = if layout.textured { 3 } else { 1 };
        assert!(
            desc.entries.len() == count
                && (0..count as u32).all(|binding| {
                    desc.entries
                        .iter()
                        .filter(|entry| entry.binding == binding)
                        .count()
                        == 1
                }),
            "TRUEOS wgpu: expected admitted group-zero bindings"
        );
        let entry = |binding| {
            &desc
                .entries
                .iter()
                .find(|entry| entry.binding == binding)
                .unwrap()
                .resource
        };
        let wgpu::BindingResource::Buffer(binding) = entry(0) else {
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
        let atlas = if layout.textured {
            let wgpu::BindingResource::TextureView(view) = entry(1) else {
                panic!("TRUEOS wgpu: expected sampled atlas view");
            };
            let view = view
                .as_custom::<View>()
                .expect("TRUEOS wgpu: foreign atlas view");
            owned(&self.0, &view.0.native);
            assert!(
                !*view.0.destroyed.lock().unwrap(),
                "TRUEOS wgpu: destroyed atlas"
            );
            let TextureKind::Sampled(atlas) = &view.0.kind else {
                panic!("TRUEOS wgpu: expected sampled RGBA8 atlas");
            };
            let wgpu::BindingResource::Sampler(sampler) = entry(2) else {
                panic!("TRUEOS wgpu: expected atlas sampler");
            };
            let sampler = sampler
                .as_custom::<Sampler>()
                .expect("TRUEOS wgpu: foreign sampler");
            owned(&self.0, &sampler.0);
            Some(atlas.clone())
        } else {
            None
        };
        DispatchBindGroup::custom(Bind {
            native: self.0.clone(),
            camera: buffer.0.clone(),
            range,
            atlas,
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
            revision: AtomicU64::new(0),
        })))
    }
    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        let depth = desc.format == wgpu::TextureFormat::Depth32Float
            && desc.usage == wgpu::TextureUsages::RENDER_ATTACHMENT;
        let sampled = desc.format == wgpu::TextureFormat::Rgba8Unorm
            && desc.usage == (wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING);
        assert!(
            (depth || sampled)
                && desc.dimension == wgpu::TextureDimension::D2
                && desc.mip_level_count == 1
                && desc.sample_count == 1
                && desc.size.depth_or_array_layers == 1
                && desc.size.width > 0
                && desc.size.height > 0
                && desc.size.width <= 4096
                && desc.size.height <= 4096
                && desc.view_formats.is_empty(),
            "TRUEOS wgpu: unsupported single-level texture descriptor"
        );
        let destroyed = Arc::new(Mutex::new(false));
        let kind = if sampled {
            let size = u64::from(desc.size.width) * u64::from(desc.size.height) * 4;
            assert!(
                size <= MAX_BUFFER_BYTES,
                "TRUEOS wgpu: atlas exceeds native upload limit"
            );
            let mut live_bytes = self.0.buffer_bytes.lock().unwrap();
            let next = live_bytes
                .checked_add(size)
                .filter(|value| *value <= 128 * 1024 * 1024)
                .expect("TRUEOS wgpu: texture memory budget exceeded");
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size as usize)
                .expect("TRUEOS wgpu: atlas allocation failed");
            bytes.resize(size as usize, 0);
            *live_bytes = next;
            TextureKind::Sampled(Arc::new(SampledTextureData {
                native: self.0.clone(),
                pixels: Mutex::new(AtlasPixels { bytes, revision: 0 }),
                upload: Mutex::new(None),
                unknown_completion: AtomicBool::new(false),
                size: desc.size,
                destroyed: destroyed.clone(),
            }))
        } else {
            TextureKind::Depth
        };
        DispatchTexture::custom(Texture {
            native: self.0.clone(),
            size: desc.size,
            kind,
            destroyed,
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
        assert!(
            desc.address_mode_u == wgpu::AddressMode::ClampToEdge
                && desc.address_mode_v == wgpu::AddressMode::ClampToEdge
                && desc.address_mode_w == wgpu::AddressMode::ClampToEdge
                && desc.mag_filter == wgpu::FilterMode::Nearest
                && desc.min_filter == wgpu::FilterMode::Nearest
                && desc.mipmap_filter == wgpu::MipmapFilterMode::Nearest
                && desc.lod_min_clamp == 0.
                && desc.lod_max_clamp == 32.
                && desc.compare.is_none()
                && desc.anisotropy_clamp == 1
                && desc.border_color.is_none(),
            "TRUEOS wgpu: only the default nearest clamp sampler is supported"
        );
        DispatchSampler::custom(Sampler(self.0.clone()))
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
        let mut bytes = buffer.0.bytes.lock().unwrap();
        if bytes[range.clone()] != *data {
            let revision = buffer
                .0
                .revision
                .load(Ordering::Relaxed)
                .checked_add(1)
                .expect("TRUEOS wgpu: buffer revision overflow");
            bytes[range].copy_from_slice(data);
            buffer.0.revision.store(revision, Ordering::Release);
        }
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
        let _queue = self.0.submit_lock.lock().unwrap();
        let target = texture
            .texture
            .as_custom::<Texture>()
            .expect("TRUEOS wgpu: foreign texture");
        owned(&self.0, &target.native);
        assert!(
            !*target.destroyed.lock().unwrap(),
            "TRUEOS wgpu: destroyed atlas"
        );
        let TextureKind::Sampled(atlas) = &target.kind else {
            panic!("TRUEOS wgpu: texture is not a COPY_DST atlas");
        };
        assert!(
            texture.mip_level == 0
                && texture.origin == wgpu::Origin3d::ZERO
                && texture.aspect == wgpu::TextureAspect::All
                && size == target.size,
            "TRUEOS wgpu: only a complete atlas upload is supported"
        );
        write_atlas(atlas, data, data_layout);
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
                && desc.usage.is_none_or(|usage| usage
                    == if matches!(self.kind, TextureKind::Sampled(_)) {
                        wgpu::TextureUsages::TEXTURE_BINDING
                    } else {
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                    })
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
        if matches!(self.kind, TextureKind::Sampled(_)) {
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        }
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
impl SamplerInterface for Sampler {}
impl RenderPipelineInterface for Pipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        assert_eq!(index, 0, "TRUEOS wgpu: only group zero is supported");
        DispatchBindGroupLayout::custom(binding_layout(&self.0.native))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct UploadBroker {
        buffers: RefCell<Vec<Option<(u32, Vec<u8>)>>>,
        short_write: Cell<bool>,
        writes: Cell<usize>,
        ranges: RefCell<Vec<(usize, usize, usize)>>,
    }

    impl UploadDevice for UploadBroker {
        type Buffer = usize;
        fn create(&self, bytes: usize, usage: u32) -> Result<usize, i32> {
            let mut buffers = self.buffers.borrow_mut();
            let handle = buffers.len();
            buffers.push(Some((usage, vec![0; bytes])));
            Ok(handle)
        }
        fn write_at(&self, buffer: usize, offset: usize, bytes: &[u8]) -> Result<usize, i32> {
            let mut buffers = self.buffers.borrow_mut();
            let (usage, storage) = buffers[buffer].as_mut().unwrap();
            if *usage & vgpu::BUFFER_USAGE_MAP_WRITE == 0 {
                return Err(vgpu::ERR_PERMISSION);
            }
            self.writes.set(self.writes.get() + 1);
            self.ranges.borrow_mut().push((buffer, offset, bytes.len()));
            if self.short_write.get() {
                return Ok(bytes.len().saturating_sub(4));
            }
            let end = offset
                .checked_add(bytes.len())
                .filter(|end| *end <= storage.len())
                .ok_or(vgpu::ERR_UNSUPPORTED)?;
            storage[offset..end].copy_from_slice(bytes);
            Ok(bytes.len())
        }
        fn destroy(&self, buffer: usize) {
            self.buffers.borrow_mut()[buffer] = None;
        }
    }

    #[test]
    fn native_submission_uploads_satisfy_the_broker_write_contract() {
        let broker = UploadBroker::default();
        let old_usage = vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_COPY_DST;
        let denied = broker.create(4, old_usage).unwrap();
        assert_eq!(broker.write(denied, &[0; 4]), Err(vgpu::ERR_PERMISSION));
        broker.destroy(denied);

        let camera_and_vertices = vec![0x5a; 80 + 3 * 32];
        let indices: Vec<u8> = [0u32, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let retained = create_pass_upload(&broker, 3).unwrap();
        let vertex = retained.vertex;
        let index = retained.index;
        write_native_range(&broker, vertex, 0, &camera_and_vertices, "vertex upload").unwrap();
        let buffers = broker.buffers.borrow();
        for (handle, role, expected) in [
            (vertex, vgpu::BUFFER_USAGE_VERTEX, &camera_and_vertices),
            (index, vgpu::BUFFER_USAGE_INDEX, &indices),
        ] {
            let (usage, uploaded) = buffers[handle].as_ref().unwrap();
            assert_ne!(usage & role, 0);
            assert_eq!(uploaded, expected);
        }
    }

    #[test]
    fn incomplete_native_upload_is_rejected_and_released() {
        let broker = UploadBroker::default();
        broker.short_write.set(true);
        let error = create_pass_upload(&broker, 3).unwrap_err();
        assert_eq!(error.code, vgpu::ERR_IO);
        assert_eq!(error.operation, "index upload");
        assert!(broker.buffers.borrow().iter().all(Option::is_none));
    }

    #[test]
    fn retained_atlas_upload_preserves_handle_and_skips_unchanged_revision() {
        let broker = UploadBroker::default();
        let mut upload = None;
        let first = upload_pixels(&broker, &mut upload, 1, &[4, 5, 6, 255]).unwrap();
        assert_eq!(
            broker.buffers.borrow()[first].as_ref().unwrap().0,
            vgpu::BUFFER_USAGE_MAP_WRITE
        );
        assert_eq!(
            upload_pixels(&broker, &mut upload, 1, &[4, 5, 6, 255]).unwrap(),
            first
        );
        assert_eq!(broker.writes.get(), 1);
        assert_eq!(
            upload_pixels(&broker, &mut upload, 2, &[7, 8, 9, 255]).unwrap(),
            first
        );
        assert_eq!(broker.writes.get(), 2);
        assert_eq!(broker.buffers.borrow().len(), 1);
        assert_eq!(
            broker.buffers.borrow()[first].as_ref().unwrap().1,
            [7, 8, 9, 255]
        );
    }

    #[test]
    fn failed_atlas_rewrite_keeps_owned_buffer_and_old_revision() {
        let broker = UploadBroker::default();
        let mut upload = None;
        let buffer = upload_pixels(&broker, &mut upload, 1, &[4, 5, 6, 255]).unwrap();
        broker.short_write.set(true);
        assert_eq!(
            upload_pixels(&broker, &mut upload, 2, &[7, 8, 9, 255])
                .unwrap_err()
                .code,
            vgpu::ERR_IO
        );
        assert_eq!(upload.as_ref().unwrap().revision, 1);
        assert!(broker.buffers.borrow()[buffer].is_some());
    }

    #[test]
    fn new_atlas_short_upload_releases_unsubmitted_storage() {
        let broker = UploadBroker::default();
        broker.short_write.set(true);
        let mut upload = None;
        assert_eq!(
            upload_pixels(&broker, &mut upload, 1, &[4, 5, 6, 255])
                .unwrap_err()
                .code,
            vgpu::ERR_IO
        );
        assert!(upload.is_none());
        assert!(broker.buffers.borrow().iter().all(Option::is_none));
    }

    #[cfg(not(target_os = "trueos"))]
    fn atlas(context: &Context) -> wgpu::Texture {
        context.device().create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    #[cfg(not(target_os = "trueos"))]
    fn write_pixels(context: &Context, atlas: &wgpu::Texture, data: &[u8], pitch: u32) {
        context.queue().write_texture(
            atlas.as_image_copy(),
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(2),
            },
            atlas.size(),
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn atlas_upload_copies_rows_ignores_padding_and_tracks_actual_changes() {
        let context = cpu_context();
        let atlas = atlas(&context);
        let data = [
            1, 2, 3, 255, 4, 5, 6, 255, 99, 99, 99, 99, 7, 8, 9, 255, 10, 11, 12, 255,
        ];
        write_pixels(&context, &atlas, &data, 12);
        let TextureKind::Sampled(sampled) = &atlas.as_custom::<Texture>().unwrap().kind else {
            unreachable!()
        };
        assert_eq!(
            sampled.pixels.lock().unwrap().bytes,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        assert_eq!(sampled.pixels.lock().unwrap().revision, 1);
        write_pixels(&context, &atlas, &data, 12);
        assert_eq!(sampled.pixels.lock().unwrap().revision, 1);
        assert_eq!(*context.native.buffer_bytes.lock().unwrap(), 16);
        assert!(!sampled.unknown_completion.load(Ordering::Acquire));
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn textured_layout_reflection_and_public_wgpu_bindings_agree() {
        let context = cpu_context_with_package(ShaderPackage::new(
            VOXY_TEXTURED_WGSL,
            vgpu::SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64,
        ));
        // CPU dispatch fixture: no native pipeline is admitted or executed.
        let pipeline = Pipeline(Arc::new(PipelineData {
            native: context.native.clone(),
            shader: None,
            pipeline: None,
        }));
        let reflected = pipeline.get_bind_group_layout(0);
        assert!(reflected.as_custom::<Layout>().unwrap().textured);
        let layout = context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let camera = camera(&context, 80);
        let atlas = atlas(&context);
        let view = atlas.create_view(&Default::default());
        let sampler = context.device().create_sampler(&Default::default());
        let bind = context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
        assert!(bind.as_custom::<Bind>().unwrap().atlas.is_some());
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "atlas upload exceeds data")]
    fn short_atlas_rows_are_rejected() {
        let context = cpu_context();
        let atlas = atlas(&context);
        write_pixels(&context, &atlas, &[255; 15], 8);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "only opaque atlas texels")]
    fn transparent_atlas_is_rejected_before_any_write() {
        let context = cpu_context();
        let atlas = atlas(&context);
        write_pixels(&context, &atlas, &[0; 16], 8);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "resource belongs to another device")]
    fn foreign_atlas_upload_is_rejected() {
        let first = cpu_context();
        let second = cpu_context();
        write_pixels(&second, &atlas(&first), &[255; 16], 8);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "destroyed atlas")]
    fn destroyed_atlas_upload_is_rejected() {
        let context = cpu_context();
        let atlas = atlas(&context);
        atlas.destroy();
        write_pixels(&context, &atlas, &[255; 16], 8);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    #[should_panic(expected = "nearest clamp sampler")]
    fn linear_sampler_is_rejected() {
        let context = cpu_context();
        let _ = context.device().create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "trueos"))]
    fn cpu_context() -> Context {
        cpu_context_with_package(ShaderPackage::new(
            VOXY_WGSL,
            vgpu::SHADER_PACKAGE_VOXY_HEADLESS_FNV1A64,
        ))
    }

    #[cfg(not(target_os = "trueos"))]
    fn cpu_context_with_package(package: ShaderPackage) -> Context {
        let native = Arc::new(Native {
            device: None,
            queue: None,
            package,
            error: Mutex::new(None),
            serial: Mutex::new(0),
            buffer_bytes: Mutex::new(0),
            submit_lock: Mutex::new(()),
            pass_upload: Mutex::new(None),
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

    #[cfg(not(target_os = "trueos"))]
    fn geometry(context: &Context, count: usize, marker: f32) -> wgpu::Buffer {
        let buffer = context.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (count * VERTEX_STRIDE) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes: Vec<u8> = (0..count)
            .flat_map(|index| [marker + index as f32, 0., 0., 1., 1., 1., 1., 1.])
            .flat_map(f32::to_le_bytes)
            .collect();
        context.queue().write_buffer(&buffer, 0, &bytes);
        buffer
    }

    #[cfg(not(target_os = "trueos"))]
    fn cpu_pipeline(context: &Context) -> Arc<PipelineData> {
        Arc::new(PipelineData {
            native: context.native.clone(),
            shader: None,
            pipeline: None,
        })
    }

    #[cfg(not(target_os = "trueos"))]
    fn geometry_draw(
        context: &Context,
        camera: &wgpu::Buffer,
        pipeline: &Arc<PipelineData>,
        buffer: &wgpu::Buffer,
        vertices: Range<u32>,
    ) -> Draw {
        let buffer = buffer.as_custom::<Buffer>().unwrap().0.clone();
        Draw {
            pipeline: pipeline.clone(),
            bind: Bind {
                native: context.native.clone(),
                camera: camera.as_custom::<Buffer>().unwrap().0.clone(),
                range: 0..80,
                atlas: None,
            },
            slice: 0..buffer.size as usize,
            buffer,
            vertices,
            indices: None,
        }
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn persistent_geometry_sends_only_camera_or_changed_overlay_between_frames() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let terrain = geometry(&context, 6, 10.);
        let overlay = geometry(&context, 3, 20.);
        let pipeline = cpu_pipeline(&context);
        let draws = [
            geometry_draw(&context, &camera, &pipeline, &terrain, 0..6),
            geometry_draw(&context, &camera, &pipeline, &overlay, 0..3),
        ];
        let broker = UploadBroker::default();
        let mut upload = create_pass_upload(&broker, 12).unwrap();
        assert_eq!(
            broker.buffers.borrow()[upload.index].as_ref().unwrap().1,
            (0..12u32).flat_map(u32::to_le_bytes).collect::<Vec<_>>()
        );
        broker.ranges.borrow_mut().clear();
        let writes =
            update_pass_upload(&broker, &mut upload, &pass_sources(&context.native, &draws))
                .unwrap();
        assert_eq!(
            writes,
            GeometryWrites {
                camera_bytes: 80,
                first_draw_bytes: 192,
                other_draw_bytes: 96
            }
        );
        assert_eq!(
            *broker.ranges.borrow(),
            [
                (upload.vertex, 0, 80),
                (upload.vertex, 80, 192),
                (upload.vertex, 272, 96)
            ]
        );
        broker.ranges.borrow_mut().clear();
        assert_eq!(
            update_pass_upload(&broker, &mut upload, &pass_sources(&context.native, &draws))
                .unwrap(),
            GeometryWrites::default()
        );
        assert!(broker.ranges.borrow().is_empty());

        context.queue().write_buffer(&camera, 0, &[1; 80]);
        let writes =
            update_pass_upload(&broker, &mut upload, &pass_sources(&context.native, &draws))
                .unwrap();
        assert_eq!(
            writes,
            GeometryWrites {
                camera_bytes: 80,
                ..Default::default()
            }
        );
        assert_eq!(*broker.ranges.borrow(), [(upload.vertex, 0, 80)]);
        broker.ranges.borrow_mut().clear();
        context
            .queue()
            .write_buffer(&overlay, 0, &99f32.to_le_bytes());
        let writes =
            update_pass_upload(&broker, &mut upload, &pass_sources(&context.native, &draws))
                .unwrap();
        assert_eq!(
            writes,
            GeometryWrites {
                other_draw_bytes: 96,
                ..Default::default()
            }
        );
        assert_eq!(*broker.ranges.borrow(), [(upload.vertex, 272, 96)]);
        let buffers = broker.buffers.borrow();
        assert_eq!(
            &buffers[upload.vertex].as_ref().unwrap().1[80..272],
            &terrain
                .as_custom::<Buffer>()
                .unwrap()
                .0
                .bytes
                .lock()
                .unwrap()[..]
        );
        assert_eq!(
            buffers.len(),
            2,
            "native geometry handles must stay unchanged"
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn changing_layout_moves_only_affected_segments_and_reuses_identity_indices() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let terrain = geometry(&context, 6, 10.);
        let overlay = geometry(&context, 3, 20.);
        let pipeline = cpu_pipeline(&context);
        let mut draws = vec![
            geometry_draw(&context, &camera, &pipeline, &terrain, 0..3),
            geometry_draw(&context, &camera, &pipeline, &overlay, 0..3),
        ];
        let broker = UploadBroker::default();
        let mut upload = create_pass_upload(&broker, 12).unwrap();
        update_pass_upload(&broker, &mut upload, &pass_sources(&context.native, &draws)).unwrap();
        broker.ranges.borrow_mut().clear();
        draws[0].vertices = 0..6;
        let sources = pass_sources(&context.native, &draws);
        assert_eq!(sources.vertex_count, 9);
        let writes = update_pass_upload(&broker, &mut upload, &sources).unwrap();
        assert_eq!(
            writes,
            GeometryWrites {
                first_draw_bytes: 192,
                other_draw_bytes: 96,
                ..Default::default()
            }
        );
        assert_eq!(
            *broker.ranges.borrow(),
            [(upload.vertex, 80, 192), (upload.vertex, 272, 96)]
        );
        broker.ranges.borrow_mut().clear();
        draws.truncate(1);
        let sources = pass_sources(&context.native, &draws);
        assert_eq!(sources.vertex_count, 6);
        assert_eq!(
            update_pass_upload(&broker, &mut upload, &sources).unwrap(),
            GeometryWrites::default()
        );
        assert!(broker.ranges.borrow().is_empty());
        assert_eq!(upload.segments.len(), 1);
        assert_eq!(broker.buffers.borrow().len(), 2);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn indexed_sources_preserve_base_vertex_and_refresh_on_index_revision() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let geometry = geometry(&context, 4, 10.);
        let pipeline = cpu_pipeline(&context);
        let indices = context.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 8,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes: Vec<u8> = [0u16, 1, 2, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        context.queue().write_buffer(&indices, 0, &bytes);
        let mut draw = geometry_draw(&context, &camera, &pipeline, &geometry, 0..3);
        draw.indices = Some((
            indices.as_custom::<Buffer>().unwrap().0.clone(),
            0..8,
            wgpu::IndexFormat::Uint16,
            1,
        ));
        let broker = UploadBroker::default();
        let mut upload = create_pass_upload(&broker, 6).unwrap();
        update_pass_upload(
            &broker,
            &mut upload,
            &pass_sources(&context.native, std::slice::from_ref(&draw)),
        )
        .unwrap();
        assert_eq!(
            &broker.buffers.borrow()[upload.vertex].as_ref().unwrap().1[80..176],
            &geometry
                .as_custom::<Buffer>()
                .unwrap()
                .0
                .bytes
                .lock()
                .unwrap()[32..128]
        );
        broker.ranges.borrow_mut().clear();
        let bytes: Vec<u8> = [2u16, 1, 0, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        context.queue().write_buffer(&indices, 0, &bytes);
        let writes = update_pass_upload(
            &broker,
            &mut upload,
            &pass_sources(&context.native, std::slice::from_ref(&draw)),
        )
        .unwrap();
        assert_eq!(
            writes,
            GeometryWrites {
                first_draw_bytes: 96,
                ..Default::default()
            }
        );
        assert_eq!(*broker.ranges.borrow(), [(upload.vertex, 80, 96)]);
        let expected: Vec<u8> = [3usize, 2, 1]
            .into_iter()
            .flat_map(|index| {
                geometry
                    .as_custom::<Buffer>()
                    .unwrap()
                    .0
                    .bytes
                    .lock()
                    .unwrap()[index * 32..index * 32 + 32]
                    .to_vec()
            })
            .collect();
        assert_eq!(
            &broker.buffers.borrow()[upload.vertex].as_ref().unwrap().1[80..176],
            &expected
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn same_logical_buffer_can_supply_vertices_and_indices() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let vertices = geometry(&context, 3, 10.);
        let combined = context.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 104,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::INDEX
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        context.queue().write_buffer(
            &combined,
            0,
            &vertices
                .as_custom::<Buffer>()
                .unwrap()
                .0
                .bytes
                .lock()
                .unwrap(),
        );
        let indices: Vec<u8> = [0u16, 1, 2, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        context.queue().write_buffer(&combined, 96, &indices);
        let pipeline = cpu_pipeline(&context);
        let mut draw = geometry_draw(&context, &camera, &pipeline, &combined, 0..3);
        draw.slice = 0..96;
        draw.indices = Some((
            combined.as_custom::<Buffer>().unwrap().0.clone(),
            96..104,
            wgpu::IndexFormat::Uint16,
            0,
        ));
        assert_eq!(
            materialize_segment(&draw),
            *vertices
                .as_custom::<Buffer>()
                .unwrap()
                .0
                .bytes
                .lock()
                .unwrap()
        );
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn cached_sources_do_not_keep_logical_buffers_alive() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let geometry = geometry(&context, 3, 10.);
        let weak = Arc::downgrade(&geometry.as_custom::<Buffer>().unwrap().0);
        let pipeline = cpu_pipeline(&context);
        let draw = geometry_draw(&context, &camera, &pipeline, &geometry, 0..3);
        let broker = UploadBroker::default();
        let mut upload = create_pass_upload(&broker, 6).unwrap();
        update_pass_upload(
            &broker,
            &mut upload,
            &pass_sources(&context.native, std::slice::from_ref(&draw)),
        )
        .unwrap();
        drop(draw);
        drop(geometry);
        assert!(weak.upgrade().is_none());
        assert!(upload.segments[0].source.upgrade().is_none());
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn partial_geometry_upload_never_commits_the_new_source_revision() {
        let context = cpu_context();
        let camera = camera(&context, 80);
        let geometry = geometry(&context, 3, 10.);
        let pipeline = cpu_pipeline(&context);
        let draw = geometry_draw(&context, &camera, &pipeline, &geometry, 0..3);
        let broker = UploadBroker::default();
        let mut upload = create_pass_upload(&broker, 6).unwrap();
        update_pass_upload(
            &broker,
            &mut upload,
            &pass_sources(&context.native, std::slice::from_ref(&draw)),
        )
        .unwrap();
        let prior_revision = upload.segments[0].revision;
        context
            .queue()
            .write_buffer(&geometry, 0, &99f32.to_le_bytes());
        broker.short_write.set(true);
        assert_eq!(
            update_pass_upload(
                &broker,
                &mut upload,
                &pass_sources(&context.native, std::slice::from_ref(&draw))
            )
            .unwrap_err()
            .code,
            vgpu::ERR_IO
        );
        assert_eq!(upload.segments[0].revision, prior_revision);
        assert!(!release_pass_upload(&broker, upload, true));
        assert!(broker.buffers.borrow().iter().all(Option::is_some));
    }

    #[test]
    fn native_geometry_is_released_only_after_known_idle_completion() {
        let broker = UploadBroker::default();
        let mut unknown = create_pass_upload(&broker, 6).unwrap();
        unknown.unknown_completion = true;
        let handles = [unknown.vertex, unknown.index];
        assert!(!release_pass_upload(&broker, unknown, false));
        assert!(
            handles
                .into_iter()
                .all(|handle| broker.buffers.borrow()[handle].is_some())
        );
        let idle = create_pass_upload(&broker, 6).unwrap();
        let handles = [idle.vertex, idle.index];
        assert!(release_pass_upload(&broker, idle, false));
        assert!(
            handles
                .into_iter()
                .all(|handle| broker.buffers.borrow()[handle].is_none())
        );
    }

    #[test]
    fn full_native_geometry_capacity_stays_inside_buffer_limit() {
        assert!(CAMERA_BYTES + MAX_FRAME_VERTICES * VERTEX_STRIDE <= MAX_BUFFER_BYTES as usize);
        assert!(MAX_FRAME_VERTICES * 4 <= MAX_BUFFER_BYTES as usize);
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
        assert_eq!(
            fnv(VOXY_TEXTURED_WGSL.as_bytes()),
            vgpu::SHADER_PACKAGE_VOXY_HEADLESS_TEXTURE_FNV1A64
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
