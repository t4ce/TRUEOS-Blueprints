use link3d::{
    roster::{Keyboard, Mouse, Tablet},
    scene::{
        MAX_INSTANCES, MAX_KEYBOARDS, MAX_MICE, MAX_TABLETS, RowInstances, SceneGeometry,
        palette_bmp,
    },
};
use trueos::{
    async_fs,
    vgpu::{
        self, Buffer, Device, Queue, RetainedCamera, RetainedDrawRange, RetainedFrameSubmit,
        RetainedFrameSubmitV2, RetainedFrameSubmitV3, RetainedMaterialParameters, RetainedMesh,
        RetainedMeshDescriptor, TimelinePoint, Ui4Surface,
    },
    vmedia::{self, ImageFormat, RetainedTexture},
};

const CLEAR: u32 = u32::from_le_bytes([128, 128, 128, 0]);

/// One resident mesh holding shared keyboard and mouse geometry. Roster changes replace only the
/// 64-byte instance rows and their color palette; camera changes upload neither.
pub struct Renderer {
    device: Device,
    geometry: Geometry,
    seeds: Buffer,
    count: u32,
    palette: Option<RetainedTexture>,
    properties: RetainedTexture,
    keyboard_index_count: u32,
    draws: [RetainedDrawRange; 4],
    draw_count: u32,
}

impl Renderer {
    pub fn new(
        device: Device,
        scene: &SceneGeometry,
        radius: f32,
        properties: &[u8],
    ) -> Result<Self, i32> {
        let geometry = Geometry::new(device, &scene.vertices, &scene.indices, radius)?;
        // Mesh creation establishes the carrier before retained texture decode.
        let properties = async_fs::block_on(vmedia::decode_retained(
            device,
            ImageFormat::Bmp,
            properties,
        ))?;
        let seeds = device.create_buffer(
            MAX_INSTANCES * 64,
            vgpu::BUFFER_USAGE_MAP_READ | vgpu::BUFFER_USAGE_MAP_WRITE,
        )?;
        Ok(Self {
            device,
            geometry,
            seeds,
            count: 0,
            palette: None,
            properties,
            keyboard_index_count: scene.keyboard_index_count,
            draws: [RetainedDrawRange::default(); 4],
            draw_count: 0,
        })
    }

    pub fn update_instances(
        &mut self,
        row: &RowInstances,
        keyboards: &[Keyboard],
        mice: &[Mouse],
        tablets: &[Tablet],
    ) -> Result<(), i32> {
        if row.count == 0 {
            self.count = 0;
            return Ok(());
        }
        if row.count as usize != keyboards.len() + mice.len() + tablets.len()
            || row.seeds.len() != (keyboards.len() + mice.len() + tablets.len()) * 64
            || keyboards.len() > MAX_KEYBOARDS
            || mice.len() > MAX_MICE
            || tablets.len() > MAX_TABLETS
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let palette = async_fs::block_on(vmedia::decode_retained(
            self.device,
            ImageFormat::Bmp,
            &palette_bmp(keyboards, mice, tablets),
        ))?;
        let written = self.device.write_buffer(self.seeds, 0, &row.seeds)?;
        if written != row.seeds.len() {
            return Err(vgpu::ERR_IO);
        }
        self.palette = Some(palette);
        self.count = row.count;
        self.draws = [RetainedDrawRange::default(); 4];
        self.draw_count = 0;
        // The retained renderer requires every submitted group to be nonempty.
        if !keyboards.is_empty() {
            self.draws[0] = RetainedDrawRange {
                first_index: 0,
                index_count: self.keyboard_index_count,
            };
            self.draw_count += 1;
        }
        if !mice.is_empty() || !tablets.is_empty() {
            self.draws[self.draw_count as usize] = RetainedDrawRange {
                first_index: self.keyboard_index_count,
                index_count: self.geometry.index_count - self.keyboard_index_count,
            };
            self.draw_count += 1;
        }
        Ok(())
    }

    pub fn render(
        &self,
        queue: Queue,
        surface: Ui4Surface,
        camera: RetainedCamera,
    ) -> Result<TimelinePoint, i32> {
        if self.count == 0 {
            return self.device.submit_ui4_clear(queue, surface, CLEAR);
        }
        let mut frame = RetainedFrameSubmit {
            camera,
            clear_rgba8_srgb: CLEAR,
            ..RetainedFrameSubmit::default()
        };
        frame.material.textures[vgpu::RETAINED_MATERIAL_BASE_COLOR] = self
            .palette
            .as_ref()
            .ok_or(vgpu::ERR_UNSUPPORTED)?
            .id()
            .raw();
        frame.material.textures[vgpu::RETAINED_MATERIAL_METALLIC_ROUGHNESS] =
            self.properties.id().raw();
        self.device.submit_retained_frame_v3(
            queue,
            surface,
            self.geometry.mesh,
            self.geometry.vertex_buffer,
            self.geometry.index_buffer,
            RetainedFrameSubmitV3 {
                frame: RetainedFrameSubmitV2 {
                    frame,
                    material_parameters: RetainedMaterialParameters {
                        metallic_factor: 0.5,
                        roughness_factor: 1.0,
                        flags: vgpu::RETAINED_MATERIAL_FLAG_NEAREST,
                        ..RetainedMaterialParameters::default()
                    },
                },
                seed_buffer: self.seeds.raw(),
                seed_count: self.count,
                draw_count: self.draw_count,
                draws: self.draws,
                ..RetainedFrameSubmitV3::default()
            },
        )
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.seeds);
    }
}

struct Geometry {
    device: Device,
    mesh: RetainedMesh,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u32,
}
impl Geometry {
    fn new(device: Device, vertices: &[u8], indices: &[u8], radius: f32) -> Result<Self, i32> {
        if vertices.is_empty()
            || indices.is_empty()
            || vertices.len() % 48 != 0
            || indices.len() % 4 != 0
            || !radius.is_finite()
            || radius <= 0.0
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let vertex_count = u32::try_from(vertices.len() / 48).map_err(|_| vgpu::ERR_UNSUPPORTED)?;
        let index_count = u32::try_from(indices.len() / 4).map_err(|_| vgpu::ERR_UNSUPPORTED)?;
        let vertex_buffer = upload(device, vertices, vgpu::BUFFER_USAGE_VERTEX)?;
        let index_buffer = match upload(device, indices, vgpu::BUFFER_USAGE_INDEX) {
            Ok(buffer) => buffer,
            Err(error) => {
                let _ = device.destroy_buffer(vertex_buffer);
                return Err(error);
            }
        };
        let mesh = match device.create_retained_mesh(
            vertex_buffer,
            index_buffer,
            RetainedMeshDescriptor {
                vertex_count,
                index_count,
                vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV_TANGENT_INSTANCE_PALETTE,
                topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                ..RetainedMeshDescriptor::default()
            },
        ) {
            Ok(mesh) => mesh,
            Err(error) => {
                let _ = device.destroy_buffer(index_buffer);
                let _ = device.destroy_buffer(vertex_buffer);
                return Err(error);
            }
        };
        Ok(Self {
            device,
            mesh,
            vertex_buffer,
            index_buffer,
            index_count,
        })
    }
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
