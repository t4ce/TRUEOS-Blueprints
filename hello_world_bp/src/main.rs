#![cfg_attr(not(feature = "host-std"), no_std)]
#![cfg_attr(not(feature = "host-std"), no_main)]

use trueos::{ui2, vsys};

fn open_window() {
    let Some(window) = ui2::OwnedWindow::create(
        "Hello World BP",
        ui2::Rect {
            x: 96,
            y: 96,
            width: 480,
            height: 160,
        },
    ) else {
        vsys::log_error("hello world bp: ui2 window create failed\n");
        return;
    };

    let _ = window.id().set_title("Hello World BP");
    let _ = window.leak();
    vsys::log_info("hello world bp: ui2 window ready\n");
}

fn app_main(args: &[&str]) {
    open_window();
    vsys::log_info_with_args("hello world from TRUEOS app template", args);
}

trueos::portal!(app_main);
