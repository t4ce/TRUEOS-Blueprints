extern crate alloc;

use alloc::vec::Vec;

use crate::vfetch;

#[derive(Debug)]
pub struct BytesJob {
    op_id: u32,
}

impl BytesJob {
    #[inline]
    pub fn start(url: &[u8]) -> Result<Self, i32> {
        let op_id = vfetch::fetch_bytes(url)?;
        Ok(Self { op_id })
    }

    #[inline]
    pub fn wait(self, timeout_ms: u64) -> Result<Vec<u8>, i32> {
        let op_id = self.op_id;
        let rc = vfetch::fetch_bytes_wait(op_id, timeout_ms);
        if rc != 0 {
            let _ = vfetch::fetch_bytes_discard(op_id);
            return Err(rc);
        }

        match vfetch::fetch_bytes_read(op_id) {
            Ok(bytes) => {
                let _ = vfetch::fetch_bytes_discard(op_id);
                Ok(bytes)
            }
            Err(rc) => {
                let _ = vfetch::fetch_bytes_discard(op_id);
                Err(rc)
            }
        }
    }
}

#[inline]
pub fn fetch_bytes(url: &[u8], timeout_ms: u64) -> Result<Vec<u8>, i32> {
    BytesJob::start(url)?.wait(timeout_ms)
}
