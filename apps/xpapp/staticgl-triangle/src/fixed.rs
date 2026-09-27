//! Fixed GL state and guest arrays uploaded for native GPU VS/PS execution.
use trueos::vgpu::{self, Buffer, Device, Queue, RenderPipeline, ShaderModule, TimelinePoint, Ui4Surface};
use super::textured::{bytes_of_slice, ensure_buffer};
pub struct FixedRenderer {
    device: Device, shader: ShaderModule, pipeline: RenderPipeline,
    vertices: Option<(Buffer, usize)>, indices: Option<(Buffer, usize)>, texture: Option<(Buffer, usize)>,
}
impl FixedRenderer {
    pub fn new(device: Device) -> Result<Self, i32> {
        let shader=device.create_shader_module(vgpu::SHADER_PACKAGE_WC3_FIXED_FNV1A64)?;
        let pipeline=match device.create_render_pipeline(shader,64,0) {
            Ok(p)=>p, Err(e)=> { let _=device.destroy_shader_module(shader); return Err(e); }
        };
        Ok(Self {device,shader,pipeline,vertices:None,indices:None,texture:None})
    }
    pub fn draw(&mut self, queue: Queue, surface: Ui4Surface, vertices: &[[f32;16]], indices: &[u32],
        state: &[f32;384], rgba: &[u8], width:u32, height:u32, flags:u32) -> Result<TimelinePoint,i32> {
        if !vgpu::indexed_draw_flags_valid(flags) || vertices.is_empty() || indices.len()%3!=0
            || indices.is_empty() || indices.iter().any(|i| *i as usize>=vertices.len())
            || vertices.iter().flatten().chain(state.iter()).any(|v| !v.is_finite())
            || width==0 || height==0 || width.checked_mul(4).is_none()
            || (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4))!=Some(rgba.len()) {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let vb=bytes_of_slice(vertices); let ib=bytes_of_slice(indices);
        let vertex=ensure_buffer(self.device,&mut self.vertices,vb.len()+vgpu::WC3_FIXED_STATE_BYTES,
            vgpu::BUFFER_USAGE_MAP_WRITE|vgpu::BUFFER_USAGE_VERTEX)?;
        let index=ensure_buffer(self.device,&mut self.indices,ib.len(),vgpu::BUFFER_USAGE_MAP_WRITE|vgpu::BUFFER_USAGE_INDEX)?;
        let texture=ensure_buffer(self.device,&mut self.texture,rgba.len(),vgpu::BUFFER_USAGE_MAP_WRITE)?;
        for (buffer,offset,data) in [(vertex,0,bytes_of_slice(state)),(vertex,vgpu::WC3_FIXED_STATE_BYTES,vb),
            (index,0,ib),(texture,0,rgba)] {
            if self.device.write_buffer(buffer,offset,data)?!=data.len() { return Err(vgpu::ERR_IO); }
        }
        self.device.submit_ui4_indexed(queue,surface,self.pipeline,vertex,index,vgpu::IndexedDraw {
            vertex_offset:vgpu::WC3_FIXED_STATE_BYTES as u64,index_count:indices.len() as u32,
            topology:vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,sampled_texture:texture.raw(),texture_width:width,
            texture_height:height,texture_pitch:width*4,
            sampler_flags:vgpu::SAMPLER_ADDRESS_U_REPEAT|vgpu::SAMPLER_ADDRESS_V_REPEAT,
            texture_reserved:flags,..Default::default()
        })
    }
    pub fn destroy(mut self)->Result<(),i32> {
        for buffer in [self.vertices.take(),self.indices.take(),self.texture.take()].into_iter().flatten() { self.device.destroy_buffer(buffer.0)?; }
        self.device.destroy_render_pipeline(self.pipeline)?;
        self.device.destroy_shader_module(self.shader)
    }
}
