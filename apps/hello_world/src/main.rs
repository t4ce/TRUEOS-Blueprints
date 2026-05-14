#![no_std]
#![no_main]

use trueos::globalog::{self, level};

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    globalog::log_with_level(level::INFO, "hello_world bp: hello from no_std\n");
}
