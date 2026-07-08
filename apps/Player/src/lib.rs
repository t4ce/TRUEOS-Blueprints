//! Library surface for `player-scope`.
//!
//! The library exposes the command parser, dispatch trait, and Ratatui UI
//! runner so playback, playlist, recording, and editor behavior can be wired
//! behind the UI without coupling backend logic to drawing.

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod audio;
pub mod control;
pub mod playback;
pub mod ui;
