# ferris-says-nix

This Blueprint is the first Nix packaging smoke test for an existing GitHub
Rust package. It uses `rust-lang/ferris-says` at the exact revision recorded in
`Cargo.toml`, sends its normal `std::io::Write` output through TRUEOS stdout,
and exits after rendering the greeting.

The upstream package is licensed under MIT or Apache-2.0. This adapter does not
vendor or modify its source; Cargo resolves the pinned Git revision and the
Blueprint builder compiles it for `x86_64-unknown-trueos`.
