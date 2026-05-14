#![no_std]
#![no_main]

use trueos::logl::{self, level};

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    logl::log(level::INFO, "hello_world bp: hello from no_std\n");
}
