//! TRUEOS backing for SceneDB Pod pages and liveness words.

use std::any::Any;
use std::ops::Range;

use crate::page::{LayoutError, PageBacking, PageBackingAllocator};
use trueos::vgpu::{
    BufferSlice, Device, VVideoMem, BUFFER_USAGE_MAP_READ, BUFFER_USAGE_MAP_WRITE,
    BUFFER_USAGE_STORAGE,
};

#[derive(Copy, Clone)]
pub struct TrueosVVideoAllocator {
    device: Device,
}

impl TrueosVVideoAllocator {
    pub const fn new(device: Device) -> Self {
        Self { device }
    }
}

pub struct TrueosVVideoBacking {
    memory: VVideoMem,
}

impl TrueosVVideoBacking {
    pub fn slice(&self, range: Range<usize>) -> Result<BufferSlice, i32> {
        let bytes = range
            .end
            .checked_sub(range.start)
            .ok_or(trueos::vgpu::ERR_UNSUPPORTED)?;
        self.memory.slice(range.start, bytes)
    }

    pub const fn memory(&self) -> &VVideoMem {
        &self.memory
    }
}

unsafe impl PageBacking for TrueosVVideoBacking {
    fn as_mut_ptr(&self) -> *mut u8 {
        self.memory.as_ptr() as *mut u8
    }

    fn len(&self) -> usize {
        self.memory.len()
    }

    fn flush(&self, range: Range<usize>) -> Result<(), ()> {
        self.memory
            .flush(range.start, range.end.saturating_sub(range.start))
            .map_err(|_| ())
    }

    fn invalidate(&self, range: Range<usize>) -> Result<(), ()> {
        self.memory
            .invalidate(range.start, range.end.saturating_sub(range.start))
            .map_err(|_| ())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PageBackingAllocator for TrueosVVideoAllocator {
    fn allocate_zeroed(
        &self,
        bytes: usize,
        align: usize,
    ) -> Result<Box<dyn PageBacking>, LayoutError> {
        if align > 4096 {
            return Err(LayoutError::BackingAllocation);
        }
        let memory = self
            .device
            .allocate_vvideo_mem(
                bytes,
                BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
            )
            .map_err(|_| LayoutError::BackingAllocation)?;
        Ok(Box::new(TrueosVVideoBacking { memory }))
    }
}
