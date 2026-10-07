#![no_std]
pub fn resident_level() -> tracing_core::LevelFilter { tracing_core::LevelFilter::current() }
