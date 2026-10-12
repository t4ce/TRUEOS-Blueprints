//! Read-only OS display assets, copied from the kernel’s single image registry.
//! Embedded images are encoded JPEG/PNG. Firmware BGRT is optional RGBA8.

use crate::image_source;
pub use crate::image_source::{FORMAT_JPEG, FORMAT_PNG, FORMAT_RGBA8, Info};
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asset {
    Logo,
    IntelGraphics,
    LiveUpdate0way,
    LiveUpdateNoway,
    LiveUpdateNway,
}

/// Complete directly embedded image inventory; excludes firmware and derived views.
pub const EMBEDDED: [Asset; 5] = [
    Asset::Logo,
    Asset::IntelGraphics,
    Asset::LiveUpdate0way,
    Asset::LiveUpdateNoway,
    Asset::LiveUpdateNway,
];

impl Asset {
    pub const fn source(self) -> &'static str {
        match self {
            Self::Logo => "kernel:logo",
            Self::IntelGraphics => "kernel:intel-graphics",
            Self::LiveUpdate0way => "kernel:live-update-0way",
            Self::LiveUpdateNoway => "kernel:live-update-noway",
            Self::LiveUpdateNway => "kernel:live-update-nway",
        }
    }
}

/// Caller-owned bytes; no pointer or mutable lease into kernel asset storage.
pub struct Data {
    pub info: Info,
    pub bytes: Box<[u8]>,
}

fn read_source(source: &str) -> Result<Data, i32> {
    let (info, bytes) = image_source::read(source)?;
    Ok(Data {
        info,
        bytes: bytes.into_boxed_slice(),
    })
}

pub fn read(asset: Asset) -> Result<Data, i32> {
    read_source(asset.source())
}

/// Firmware logo is opt-in and may be absent on this machine.
pub fn bgrt() -> Result<Option<Data>, i32> {
    match read_source("kernel:bgrt") {
        Ok(data) => Ok(Some(data)),
        Err(-3) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Convenience JPEG buffer for image decoders.
pub fn logo_jpeg() -> Result<Vec<u8>, i32> {
    let data = read(Asset::Logo)?;
    if data.info.format != FORMAT_JPEG {
        return Err(-3);
    }
    Ok(data.bytes.into_vec())
}
