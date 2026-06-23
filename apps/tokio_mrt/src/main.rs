#![no_std]

use trueos::{
    clock, 
    logl::{self, level},
    rng, t, vshell,
};

fn main() {
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

    let _ = t::runtime::current_thread()
        .build()
        .map(|runtime| runtime.block_on(t::fs::write("/hello_world.txt", b"hello world")));

}
