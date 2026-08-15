#![no_std]

mod pe_bytes;
mod weave;

use trueos::vsys;

fn main() {
    let _ = vsys::log_record(
        2,
        "weave-boot-probe",
        "IMPORTANT stage=launch action=begin mode=host-helpers-noop",
    );

    match weave::run(pe_bytes::WINDOWS_EXE) {
        Ok(exit_code) => {
            if exit_code == 0 {
                let _ = vsys::log_record(
                    2,
                    "weave-boot-probe",
                    "IMPORTANT stage=launch action=return exit_code=0",
                );
            } else {
                let _ = vsys::log_record(
                    1,
                    "weave-boot-probe",
                    "IMPORTANT stage=launch action=return result=nonzero-exit",
                );
            }
        }
        Err(error) => {
            let _ = error;
            let _ = vsys::log_record(
                1,
                "weave-boot-probe",
                "IMPORTANT stage=launch action=return result=loader-failed",
            );
        }
    }
}
