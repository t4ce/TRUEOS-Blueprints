extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRange {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogAmount {
    Lines,
    Chars,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub mod range {
    use super::LogRange;

    pub const START: LogRange = LogRange::Start;
    pub const BEGIN: LogRange = LogRange::Start;
    pub const FRONT: LogRange = LogRange::Start;
    pub const END: LogRange = LogRange::End;
    pub const BACK: LogRange = LogRange::End;
    pub const REAR: LogRange = LogRange::End;
}

pub mod amount {
    use super::LogAmount;

    pub const LINES: LogAmount = LogAmount::Lines;
    pub const CHARS: LogAmount = LogAmount::Chars;
}

pub mod level {
    use super::Level;

    pub const TRACE: Level = Level::Trace;
    pub const DEBUG: Level = Level::Debug;
    pub const INFO: Level = Level::Info;
    pub const WARN: Level = Level::Warn;
    pub const ERROR: Level = Level::Error;
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    const fn stream(self) -> u32 {
        match self {
            Self::Error => 2,
            Self::Trace | Self::Debug | Self::Info | Self::Warn => 1,
        }
    }
}

pub trait LogMessage {
    fn write_to(self, out: &mut String);
}

impl LogMessage for fmt::Arguments<'_> {
    fn write_to(self, out: &mut String) {
        let _ = out.write_fmt(self);
    }
}

impl LogMessage for &str {
    fn write_to(self, out: &mut String) {
        out.push_str(self);
    }
}

impl LogMessage for &String {
    fn write_to(self, out: &mut String) {
        out.push_str(self.as_str());
    }
}

impl LogMessage for String {
    fn write_to(self, out: &mut String) {
        out.push_str(self.as_str());
    }
}

pub fn log(message: impl LogMessage) {
    emit(None, None, message);
}

pub fn log_with_level(level: Level, message: impl LogMessage) {
    emit(None, Some(level), message);
}

pub fn log_with_concept_level(concept: &str, level: Level, message: impl LogMessage) {
    emit(Some(concept), Some(level), message);
}

pub fn log_excerpt(src: &str, range: LogRange, amount: LogAmount, count: usize) {
    let excerpt = match amount {
        LogAmount::Lines => excerpt_lines(src, range, count),
        LogAmount::Chars => excerpt_chars(src, range, count),
    };
    log(excerpt.as_str());
}

fn emit(concept: Option<&str>, level: Option<Level>, message: impl LogMessage) {
    let mut line = String::new();
    if let (Some(concept), Some(level)) = (concept, level) {
        let _ = write!(&mut line, "[{}:{}] ", concept, level.as_str());
    } else if let Some(level) = level {
        let _ = write!(&mut line, "[blueprint:{}] ", level.as_str());
    }

    message.write_to(&mut line);
    if !line.ends_with('\n') {
        line.push('\n');
    }

    let stream = level.unwrap_or(Level::Info).stream();
    crate::platform::write_stream(stream, line.as_bytes());
}

fn excerpt_chars(src: &str, range: LogRange, count: usize) -> String {
    if count == 0 || src.is_empty() {
        return String::new();
    }

    match range {
        LogRange::Start => src.chars().take(count).collect(),
        LogRange::End => src
            .chars()
            .rev()
            .take(count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    }
}

fn excerpt_lines(src: &str, range: LogRange, count: usize) -> String {
    if count == 0 || src.is_empty() {
        return String::new();
    }

    let lines: Vec<&str> = src.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let slice = match range {
        LogRange::Start => &lines[..core::cmp::min(count, lines.len())],
        LogRange::End => &lines[lines.len().saturating_sub(count)..],
    };

    let mut out = String::new();
    for (idx, line) in slice.iter().enumerate() {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}
