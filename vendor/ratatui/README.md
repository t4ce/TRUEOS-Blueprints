# Vendored Ratatui sources

This directory contains the crates.io Ratatui packages used by TRUEOS Blueprint
applications. These are the versions used by the Codex Rust CLI in the adjacent
workspace and referenced by the Blueprint workspace dependencies.

| Package | Version |
| --- | --- |
| ratatui | 0.30.2 |
| ratatui-core | 0.1.2 |
| ratatui-widgets | 0.3.2 |
| ratatui-crossterm | 0.1.2 |
| ratatui-macros | 0.7.2 |
| ratatui-termion | 0.1.2 |
| ratatui-termwiz | 0.1.2 |
| ratatui-termina | 0.1.0 |

Sources: the corresponding published crates.io source archives, copied from the
local Cargo registry cache (`index.crates.io-1949cf8c6b5b557f`). Each package keeps
its upstream README, source, license when included in the published crate, and
published supporting files. The shared Ratatui MIT license is also at `LICENSE`
in this directory (the published `ratatui-termina` archive omits a license file). Source
repository: <https://github.com/ratatui/ratatui>.

The published, normalized `Cargo.toml` files are used so these packages do not
inherit metadata from the original Ratatui repository workspace. Inter-Ratatui
dependencies additionally use sibling paths, keeping the facade, widgets, core,
and enabled backend on these vendored sources. Original pre-publication manifests
remain as `Cargo.toml.orig` for reference only. Cargo cache marker and checksum
files were omitted; this is a path dependency tree, not a Cargo directory source.
No Rust implementation changes were made while restoring these packages.
