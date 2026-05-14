#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct CrateProbe {
    name: String,
    count: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    trueos::globalog::log_with_level(trueos::globalog::level::INFO, "crate test hello\n");

    match run_probe() {
        Ok(()) => trueos::globalog::log_with_level(
            trueos::globalog::level::INFO,
            "test_some_crates bp: serde/json/regex/anyhow ok\n",
        ),
        Err(_) => trueos::globalog::log_with_level(
            trueos::globalog::level::INFO,
            "test_some_crates bp: crate probe failed\n",
        ),
    }
}

fn run_probe() -> anyhow::Result<()> {
    let probe = CrateProbe {
        name: String::from("TRUEOS blueprint"),
        count: 4,
    };

    let json = serde_json::to_string(&probe)?;
    let decoded: CrateProbe = serde_json::from_str(&json)?;
    let _regex_type_is_linkable = core::mem::size_of::<Regex>();

    anyhow::ensure!(decoded.count == 4 && decoded.name == "TRUEOS blueprint");
    Ok(())
}
