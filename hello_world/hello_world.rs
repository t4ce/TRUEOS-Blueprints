#![no_std]
#![no_main]

use core::panic::PanicInfo;
use trueos::{panic_abort, vsys, TrueosAllocator};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("hello_world bp: panic\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    vsys::log_info("hello_world bp: hello from no_std\n");
}
