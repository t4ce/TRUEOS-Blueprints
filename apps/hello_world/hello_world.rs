#![no_std]
#![no_main]

use trueos::vsys;

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    vsys::log_info("hello_world bp: hello from no_std\n");
}
