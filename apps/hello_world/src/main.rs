#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn main() {
    trueos::globalog::log_with_level(
        trueos::globalog::level::INFO,
        "hello_world bp: hello from no_std\n",
    );
}
