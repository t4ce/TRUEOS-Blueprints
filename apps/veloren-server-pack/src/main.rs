// Build the upstream CLI unchanged while keeping the Blueprint crate itself
// isolated from Veloren's root workspace manifest.
// trueos-blueprint: TRUEOS std-backed tokio::runtime with trueos::net support.
include!(concat!(env!("VELOREN_SOURCE_ROOT"), "/server-cli/src/main.rs"));

// The packer detects a Rust main in this entry file and adds the Blueprint
// entry shim. The real main comes from the upstream include above.
#[cfg(any())]
fn main() {}
