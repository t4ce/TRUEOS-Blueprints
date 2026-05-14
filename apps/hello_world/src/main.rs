#![no_std]

use trueos::logl::{self, level};

fn main() {
    logl::log(level::INFO, "hello_world bp: hello from no_std\n");
}
