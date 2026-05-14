pub use crate::globalog::{Level, LogAmount, LogMessage, LogRange, amount, level, range};

#[inline]
pub fn log(level: Level, message: impl LogMessage) {
    crate::globalog::log_with_level(level, message);
}

#[inline]
pub fn plain(message: impl LogMessage) {
    crate::globalog::log(message);
}

#[inline]
pub fn concept(concept: &str, level: Level, message: impl LogMessage) {
    crate::globalog::log_with_concept_level(concept, level, message);
}

#[inline]
pub fn excerpt(src: &str, range: LogRange, amount: LogAmount, count: usize) {
    crate::globalog::log_excerpt(src, range, amount, count);
}
