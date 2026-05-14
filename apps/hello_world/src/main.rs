#![no_std]
#![no_main]

use trueos::platform;

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    platform::log_info("hello_world bp: hello from no_std\n");
}
