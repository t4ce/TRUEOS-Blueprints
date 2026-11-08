use trueos::{
    async_fs,
    vgpu::{
        self, Buffer, Device, Queue, RetainedCamera, RetainedFrameSubmit, RetainedFrameSubmitV2,
        RetainedMaterialParameters, RetainedMesh, RetainedMeshDescriptor, RetainedTransformSeed,
        TimelinePoint, Ui4Surface,
    },
    vmedia::{self, ImageFormat, RetainedTexture},
};

const CLEAR: u32 = u32::from_le_bytes([128, 128, 128, 0]);
const VERTEX_STRIDE: usize = 48;

/// One shared palette and one resident mesh, replaced only when the roster
/// changes. Camera movement submits transforms without uploading geometry.
pub struct Renderer {
    device: Device,
    palette: RetainedTexture,
    geometry: Option<Geometry>,
}

impl Renderer {
    pub fn new(device: Device, palette_bmp: &[u8]) -> Result<Self, i32> {
        let palette = async_fs::block_on(vmedia::decode_retained(
            device,
            ImageFormat::Bmp,
            palette_bmp,
        ))?;
        Ok(Self {
            device,
            palette,
            geometry: None,
        })
    }

    pub fn replace(&mut self, vertices: &[u8], indices: &[u8], radius: f32) -> Result<(), i32> {
        if vertices.is_empty() && indices.is_empty() {
            self.geometry = None;
            return Ok(());
        }
        if vertices.is_empty()
            || indices.is_empty()
            || vertices.len() % VERTEX_STRIDE != 0
            || indices.len() % 4 != 0
            || !radius.is_finite()
            || radius < 0.0
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let vertex_count =
            u32::try_from(vertices.len() / VERTEX_STRIDE).map_err(|_| vgpu::ERR_UNSUPPORTED)?;
        let index_count = u32::try_from(indices.len() / 4).map_err(|_| vgpu::ERR_UNSUPPORTED)?;
        let vertex_buffer = upload(self.device, vertices, vgpu::BUFFER_USAGE_VERTEX)?;
        let index_buffer = match upload(self.device, indices, vgpu::BUFFER_USAGE_INDEX) {
            Ok(buffer) => buffer,
            Err(error) => {
                let _ = self.device.destroy_buffer(vertex_buffer);
                return Err(error);
            }
        };
        let mesh = match self.device.create_retained_mesh(
            vertex_buffer,
            index_buffer,
            RetainedMeshDescriptor {
                vertex_count,
                index_count,
                vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV_TANGENT,
                topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                ..RetainedMeshDescriptor::default()
            },
        ) {
            Ok(mesh) => mesh,
            Err(error) => {
                let _ = self.device.destroy_buffer(index_buffer);
                let _ = self.device.destroy_buffer(vertex_buffer);
                return Err(error);
            }
        };
        // Keep the current scene intact if allocating its replacement fails.
        self.geometry = Some(Geometry {
            device: self.device,
            mesh,
            vertex_buffer,
            index_buffer,
            radius,
        });
        Ok(())
    }

    pub fn render(
        &self,
        queue: Queue,
        surface: Ui4Surface,
        camera: RetainedCamera,
    ) -> Result<TimelinePoint, i32> {
        let Some(geometry) = &self.geometry else {
            return self.device.submit_ui4_clear(queue, surface, CLEAR);
        };
        let mut frame = RetainedFrameSubmit {
            camera,
            clear_rgba8_srgb: CLEAR,
            seed_count: 1,
            ..RetainedFrameSubmit::default()
        };
        frame.material.textures[vgpu::RETAINED_MATERIAL_BASE_COLOR] = self.palette.id().raw();
        frame.seeds[0] = RetainedTransformSeed {
            scale: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            local_radius: geometry.radius,
            ..RetainedTransformSeed::default()
        };
        self.device.submit_retained_frame_v2(
            queue,
            surface,
            geometry.mesh,
            geometry.vertex_buffer,
            geometry.index_buffer,
            RetainedFrameSubmitV2 {
                frame,
                material_parameters: RetainedMaterialParameters {
                    metallic_factor: 0.0,
                    roughness_factor: 1.0,
                    flags: vgpu::RETAINED_MATERIAL_FLAG_NEAREST,
                    ..RetainedMaterialParameters::default()
                },
            },
        )
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Release the mesh before its palette, whose carrier it shares.
        self.geometry = None;
    }
}

struct Geometry {
    device: Device,
    mesh: RetainedMesh,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    radius: f32,
}

impl Drop for Geometry {
    fn drop(&mut self) {
        let _ = self.device.destroy_retained_mesh(self.mesh);
        let _ = self.device.destroy_buffer(self.index_buffer);
        let _ = self.device.destroy_buffer(self.vertex_buffer);
    }
}

fn upload(device: Device, bytes: &[u8], usage: u32) -> Result<Buffer, i32> {
    let buffer = device.create_buffer(bytes.len(), usage | vgpu::BUFFER_USAGE_MAP_WRITE)?;
    match device.write_buffer(buffer, 0, bytes) {
        Ok(written) if written == bytes.len() => Ok(buffer),
        result => {
            let _ = device.destroy_buffer(buffer);
            Err(result.err().unwrap_or(vgpu::ERR_IO))
        }
    }
}
