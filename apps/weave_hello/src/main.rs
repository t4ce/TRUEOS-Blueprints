#![no_std]

mod pe_bytes;
mod weave;

use trueos::logl::{self, level};

fn main() {
    logl::log(
        level::INFO,
        "weave_hello: launching embedded PE32+ through TRUEOS Weave",
    );

    match weave::run(pe_bytes::WINDOWS_EXE) {
        Ok(exit_code) => {
            logl::log(
                level::INFO,
                format_args!("weave_hello: Windows PE returned exit_code={exit_code}"),
            );
        }
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("weave_hello: launch failed: {error}"),
            );
        }
    }
}
