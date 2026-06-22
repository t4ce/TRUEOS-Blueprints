#![no_std]

use trueos::{
    clock, hid,
    logl::{self, level},
    rng, t, vshell,
};

fn main() {
    logl::log(level::TRACE, format_args!("Hello World: TRACE"));
    logl::log(level::INFO, format_args!("Hello World: INFO"));
    logl::log(level::WARN, format_args!("Hello World: WARN"));
    logl::log(level::ERROR, format_args!("Hello World: ERROR"));

    match clock::utc_date_time() {
        Some(now) => logl::log(level::INFO, format_args!("Hello World: time {}", now)),
        None => logl::log(
            level::INFO,
            format_args!(
                "Hello World: wall time unavailable monotonic_ms={}",
                clock::monotonic_millis()
            ),
        ),
    }

    let mice = hid::hid_hut_mice();

    for mouse in &mice {
        logl::log(
            level::INFO,
            format_args!(
                "Hello World: cursor mouse controller={} slot={} ep={} buttons={} norm=({:.4},{:.4})",
                mouse.controller_id,
                mouse.slot_id,
                mouse.ep_target,
                mouse.buttons_down,
                mouse.x,
                mouse.y
            ),
        );
    }

    let _ = t::runtime::current_thread()
        .build()
        .map(|runtime| runtime.block_on(t::fs::write("/hello_world.txt", b"hello world")));

    logl::log(
        level::INFO,
        format_args!(
            "Hello World: rng bool={} u8={} u16={} u32={}",
            rng::boolean(),
            rng::u8(),
            rng::u16(),
            rng::u32()
        ),
    );
    logl::log(
        level::INFO,
        format_args!(
            "Hello World: rng u64={} u128={} usize={}",
            rng::u64(),
            rng::u128(),
            rng::usize()
        ),
    );
    logl::log(
        level::INFO,
        format_args!(
            "Hello World: rng i8={} i16={} i32={} i64={}",
            rng::i8(),
            rng::i16(),
            rng::i32(),
            rng::i64()
        ),
    );
    logl::log(
        level::INFO,
        format_args!(
            "Hello World: rng i128={} isize={} f32={} f64={}",
            rng::i128(),
            rng::isize(),
            rng::f32(),
            rng::f64()
        ),
    );

    let mut shell_input = [0u8; 64];
    loop {
        let read = vshell::read_blocking(&mut shell_input);
        let input = &shell_input[..read];
        vshell::write(b"Hello World: ");
        vshell::write(input);
        if !input.ends_with(b"\n") {
            vshell::write(b"\n");
        }
    }
}
