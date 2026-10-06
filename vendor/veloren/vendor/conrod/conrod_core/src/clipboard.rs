//! Plain-text clipboard shared within the process, without OS integration.
//!
//! Contents last until replaced or until the process exits.

use std::sync::Mutex;

static CONTENTS: Mutex<String> = Mutex::new(String::new());

/// Returns a copy of the current clipboard text (initially empty).
pub fn read() -> String {
    CONTENTS.lock().unwrap_or_else(|error| error.into_inner()).clone()
}

/// Replaces the clipboard text.
pub fn write(text: String) {
    *CONTENTS.lock().unwrap_or_else(|error| error.into_inner()) = text;
}
