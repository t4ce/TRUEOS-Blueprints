#!/usr/bin/env python3
"""Run the production fixed renderer against a recording vGPU device."""
from pathlib import Path
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'apps/xpapp/staticgl-triangle/src/fixed.rs').read_text()
source = source.replace('//! Fixed GL state and guest arrays uploaded for native GPU VS/PS execution.', '')
source = source.replace('use trueos::vgpu::', 'use crate::vgpu::')
stub = r'''
#![allow(dead_code)]
extern crate alloc;
mod vgpu {
use std::sync::Mutex;
pub static EVENTS: Mutex<Vec<(char,u64)>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[derive(Clone,Copy)] pub struct Buffer(u64);
impl Buffer { pub fn raw(self)->u64{self.0} }
#[derive(Clone,Copy)] pub struct Device;
#[derive(Clone,Copy)] pub struct Queue;
#[derive(Clone,Copy)] pub struct RenderPipeline;
#[derive(Clone,Copy)] pub struct ShaderModule;
#[derive(Clone,Copy)] pub struct TimelinePoint;
#[derive(Clone,Copy)] pub struct Ui4Surface;
pub const SHADER_PACKAGE_WC3_FIXED_FNV1A64:u64=1;
pub const WC3_FIXED_STATE_BYTES:usize=1536;
pub const BUFFER_USAGE_MAP_WRITE:u32=1;
pub const BUFFER_USAGE_VERTEX:u32=2;
pub const BUFFER_USAGE_INDEX:u32=4;
pub const PRIMITIVE_TOPOLOGY_TRIANGLE_LIST:u32=1;
pub const SAMPLER_ADDRESS_U_REPEAT:u32=1;
pub const SAMPLER_ADDRESS_V_REPEAT:u32=2;
pub const ERR_UNSUPPORTED:i32=-1;
pub const ERR_IO:i32=-2;
pub fn indexed_draw_flags_valid(_:u32)->bool{true}
#[derive(Default)] pub struct IndexedDraw {
pub vertex_offset:u64,pub index_count:u32,pub topology:u32,pub sampled_texture:u64,
pub texture_width:u32,pub texture_height:u32,pub texture_pitch:u32,pub sampler_flags:u32,
pub texture_reserved:u32,pub reserved:u32,
}
impl Device {
pub fn create_shader_module(self,_:u64)->Result<ShaderModule,i32>{Ok(ShaderModule)}
pub fn create_render_pipeline(self,_:ShaderModule,_:u32,_:u32)->Result<RenderPipeline,i32>{Ok(RenderPipeline)}
pub fn destroy_shader_module(self,_:ShaderModule)->Result<(),i32>{Ok(())}
pub fn destroy_render_pipeline(self,_:RenderPipeline)->Result<(),i32>{Ok(())}
pub fn create_buffer(self,_:usize,_:u32)->Result<Buffer,i32>{let id=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);EVENTS.lock().unwrap().push(('c',id));Ok(Buffer(id))}
pub fn destroy_buffer(self,b:Buffer)->Result<(),i32>{EVENTS.lock().unwrap().push(('d',b.0));Ok(())}
pub fn write_buffer(self,b:Buffer,_:usize,data:&[u8])->Result<usize,i32>{EVENTS.lock().unwrap().push(('w',b.0));Ok(data.len())}
pub fn submit_ui4_indexed(self,_:Queue,_:Ui4Surface,_:RenderPipeline,_:Buffer,_:Buffer,_:IndexedDraw)->Result<TimelinePoint,i32>{Ok(TimelinePoint)}
}
}
mod textured {
use crate::vgpu::*;
pub fn bytes_of_slice<T>(v:&[T])->&[u8]{unsafe{std::slice::from_raw_parts(v.as_ptr().cast(),std::mem::size_of_val(v))}}
pub fn ensure_buffer(d:Device,s:&mut Option<(Buffer,usize)>,n:usize,u:u32)->Result<Buffer,i32>{
if let Some((b,c))=*s {if c>=n{return Ok(b)} d.destroy_buffer(b)?;}
let b=d.create_buffer(n,u)?;*s=Some((b,n));Ok(b)
}
}
mod fixed {
'''
tests = r'''
#[test] fn reuse_mutation_shape_eviction_and_cleanup() {
    use crate::vgpu::{Device,EVENTS};
    let mut r=FixedRenderer::new(Device).unwrap();
    let a=r.sampled_texture(&[1;8],2,1).unwrap().raw();
    let b=r.sampled_texture(&[2;8],2,1).unwrap().raw();
    assert_ne!(a,b);
    assert_eq!(r.sampled_texture(&[1;8],2,1).unwrap().raw(),a);
    assert_eq!(EVENTS.lock().unwrap().iter().filter(|&&(e,id)|e=='w'&&id==a).count(),1);
    assert_ne!(r.sampled_texture(&[1;8],1,2).unwrap().raw(),a);
    for n in 0..TEXTURE_CACHE_ENTRIES {r.sampled_texture(&[n as u8;4],1,1).unwrap();}
    assert!(EVENTS.lock().unwrap().contains(&('d',a)));
    assert!(r.textures.len()<=TEXTURE_CACHE_ENTRIES);
    // Exercise byte-budget eviction independently of the entry bound.
    r.sampled_texture(&vec![7;TEXTURE_CACHE_BYTES],2048,2048).unwrap();
    assert_eq!(r.textures.len(),1);
    assert_eq!(r.texture_bytes,TEXTURE_CACHE_BYTES);
    r.destroy().unwrap();
    let events=EVENTS.lock().unwrap();
    for &(_,id) in events.iter().filter(|&&(e,_)|e=='c') {
        assert_eq!(events.iter().filter(|&&(e,i)|e=='d'&&i==id).count(),1);
    }
}
}
'''
with tempfile.TemporaryDirectory(prefix='fixed-cache-') as directory:
    path=Path(directory)
    (path/'test.rs').write_text(stub+source+tests)
    subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'test')],check=True)
    subprocess.run([str(path/'test')],check=True)
