//! Library surface for `player-scope`.
//!
//! The library exposes the command parser, dispatch trait, and Ratatui UI
//! runner so playback, playlist, recording, and editor behavior can be wired
//! behind the UI without coupling backend logic to drawing.

pub mod control;
pub mod ui;
