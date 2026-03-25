#![cfg_attr(not(feature = "host-std"), no_std)]
#![cfg_attr(not(feature = "host-std"), no_main)]

use trueos::vsys;

fn app_main(args: &[&str]) {
    vsys::log_info_with_args("hello world from TRUEOS app template", args);
}

trueos::portal!(app_main);
