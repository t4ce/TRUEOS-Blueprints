#![no_std]
#![no_main]

use trueos::vsys;

fn main(args: &[&str]) {
    vsys::log_info_with_args("hello world from TRUEOS app template", args);
}

trueos::portal!(main);
