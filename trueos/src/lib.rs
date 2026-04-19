#![cfg_attr(not(feature = "host-std"), no_std)]

pub use trueos_sys as sys;

pub mod vcabi {
    pub use trueos_sys::vcabi::*;
}

#[cfg(feature = "host-std")]
#[path = "vclock_host.rs"]
pub mod vclock;
#[cfg(not(feature = "host-std"))]
pub mod vclock;

#[cfg(feature = "host-std")]
#[path = "vfetch_host.rs"]
pub mod vfetch;
#[cfg(not(feature = "host-std"))]
pub mod vfetch;

#[cfg(feature = "host-std")]
#[path = "venv_host.rs"]
pub mod venv;
#[cfg(not(feature = "host-std"))]
pub mod venv;

#[cfg(feature = "host-std")]
#[path = "vfs_host.rs"]
pub mod vfs;
#[cfg(not(feature = "host-std"))]
pub mod vfs;

#[cfg(feature = "host-std")]
#[path = "vgfx_host.rs"]
pub mod vgfx;
#[cfg(not(feature = "host-std"))]
pub mod vgfx;

#[cfg(feature = "host-std")]
#[path = "vinput_host.rs"]
pub mod vinput;
#[cfg(not(feature = "host-std"))]
pub mod vinput;
pub mod vnet;
#[cfg(feature = "host-std")]
#[path = "runtime_host.rs"]
pub mod runtime;
#[cfg(not(feature = "host-std"))]
pub mod runtime;

#[cfg(feature = "host-std")]
#[path = "vshell_host.rs"]
pub mod vshell;
#[cfg(not(feature = "host-std"))]
pub mod vshell;

#[cfg(feature = "host-std")]
#[path = "vsys_host.rs"]
pub mod vsys;
#[cfg(not(feature = "host-std"))]
pub mod vsys;

#[cfg(feature = "host-std")]
#[path = "ui2_host.rs"]
pub mod ui2;
#[cfg(not(feature = "host-std"))]
pub mod ui2;

pub mod vled {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Rgb8 {
        pub r: u8,
        pub g: u8,
        pub b: u8,
    }

    impl Rgb8 {
        pub const fn new(r: u8, g: u8, b: u8) -> Self {
            Self { r, g, b }
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum Effect {
        Solid,
        Breathing,
        Rainbow,
        Off,
    }
}

pub mod prelude {
    pub use crate::venv;
    pub use crate::vclock;
    pub use crate::vfetch;
    pub use crate::vfs;
    pub use crate::vgfx;
    pub use crate::vinput;
    pub use crate::vnet;
    pub use crate::vshell;
    pub use crate::vsys;
}

#[cfg(feature = "host-std")]
#[macro_export]
macro_rules! portal {
    ($main:path) => {
        fn main() {
            let __trueos_host_args = $crate::runtime::host_args();
            let __trueos_host_arg_refs = __trueos_host_args
                .iter()
                .map(|arg| arg.as_str())
                .collect::<std::vec::Vec<_>>();
            $main(__trueos_host_arg_refs.as_slice())
        }
    };
    ($body:block) => {
        fn main() {
            $body
        }
    };
}

#[cfg(all(not(feature = "host-std"), not(feature = "linked-portal")))]
#[macro_export]
macro_rules! portal {
    ($main:path) => {
        #[global_allocator]
        static TRUEOS_GLOBAL_ALLOCATOR: $crate::runtime::TrueosAllocator =
            $crate::runtime::TrueosAllocator;

        #[panic_handler]
        fn trueos_panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
            $crate::runtime::panic_handler(info)
        }

        mod __trueos_app_entry {
            use super::*;

            #[unsafe(export_name = "main")]
            pub extern "C" fn __trueos_abi_main(
                argc: usize,
                argv: *const *const core::ffi::c_char,
            ) {
                let args = unsafe { $crate::runtime::args_from_abi(argc, argv) };
                $main(args)
            }
        }
    };
    ($body:block) => {
        #[global_allocator]
        static TRUEOS_GLOBAL_ALLOCATOR: $crate::runtime::TrueosAllocator =
            $crate::runtime::TrueosAllocator;

        #[panic_handler]
        fn trueos_panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
            $crate::runtime::panic_handler(info)
        }

        mod __trueos_app_entry {
            #[unsafe(export_name = "main")]
            pub extern "C" fn __trueos_abi_main(
                _argc: usize,
                _argv: *const *const core::ffi::c_char,
            ) {
                $body
            }
        }
    };
}

#[cfg(all(not(feature = "host-std"), feature = "linked-portal"))]
#[macro_export]
macro_rules! portal {
    ($main:path) => {
        mod __trueos_app_entry {
            use super::*;

            #[unsafe(export_name = "main")]
            pub extern "C" fn __trueos_abi_main(
                argc: usize,
                argv: *const *const core::ffi::c_char,
            ) {
                let args = unsafe { $crate::runtime::args_from_abi(argc, argv) };
                $main(args)
            }
        }
    };
    ($body:block) => {
        mod __trueos_app_entry {
            #[unsafe(export_name = "main")]
            pub extern "C" fn __trueos_abi_main(
                _argc: usize,
                _argv: *const *const core::ffi::c_char,
            ) {
                $body
            }
        }
    };
}
