#![no_std]

mod pe_bytes;
mod weave;

use trueos::vsys;

fn main() {
    vsys::write_out(b"hello_medium: launching embedded PE32+ through TRUEOS Weave\n");

    match weave::run(pe_bytes::WINDOWS_EXE) {
        Ok(exit_code) => {
            if exit_code == 0 {
                vsys::write_out(b"hello_medium: Windows PE returned exit_code=0\n");
            } else {
                vsys::write_err(b"hello_medium: Windows PE returned nonzero exit code\n");
            }
        }
        Err(error) => {
            let _ = error;
            vsys::write_err(b"hello_medium: embedded Windows PE launch failed\n");
        }
    }
}
