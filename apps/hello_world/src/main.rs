#![no_std]

use trueos::logl::{self, level};

fn main() {
    logl::log(level::TRACE, format_args!("Hello World: TRACE"));
    logl::log(level::INFO, format_args!("Hello World: INFO"));
    logl::log(level::WARN, format_args!("Hello World: WARN"));
    logl::log(level::ERROR, format_args!("Hello World: ERROR"));
}
