#![feature(allocator_api)]

//! Warcraft III's Windows XP compatibility process.
//!
//! Windows policy lives here. The kernel-facing boundary is the generic
//! [`trueos::x86`] address-space/context API; no WC3 operation is exposed by
//! the kernel.

#[cfg(test)]
#[macro_use]
#[path = "test.rs"]
mod test;

#[allow(non_snake_case)]
pub mod ThisToThat;
pub mod assets;
pub mod checkpoint;
pub mod child_loader;
pub mod imports;
pub mod pe32;
pub mod process;
pub mod seh;
pub mod session;
pub mod thunk32;

pub const LAUNCHER_PATH: &str = "/common/Warcraft III/Warcraft III.exe";
pub const EXPECTED_SHA256: [u8; 32] = [
    0x5a, 0x8c, 0xca, 0x72, 0x7c, 0x71, 0x9a, 0xe0, 0x54, 0xad, 0xf8, 0xd1, 0x55, 0x23, 0xa8, 0xe3,
    0x09, 0x97, 0x45, 0x22, 0x5e, 0x2f, 0x4f, 0x98, 0x85, 0xf8, 0x87, 0x74, 0xaa, 0x6f, 0x36, 0xd9,
];

// The API crate exports abort(), whose kernel termination dependencies must
// also resolve when these pure host tests are linked outside TRUEOS.
#[cfg(test)]
mod host_test_abi {
    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_blueprint_shutdown(_bytes: *const u8, _len: usize) -> i32 {
        std::process::exit(1)
    }
}
