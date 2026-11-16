//! Speech-only emoji removal. The caller retains the original text for Lily.
use alloc::{borrow::Cow, string::String};

use crate::{FrontendError, emoji_ranges::EMOJI_RANGES};

fn emoji_base(character: char) -> bool {
    let value = character as u32;
    EMOJI_RANGES
        .binary_search_by(|&(start, end)| {
            if end < value {
                core::cmp::Ordering::Less
            } else if start > value {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

// Return a whole emoji component, including presentation, skin tone and flag
// tags. ASCII digits, '#' and '*' are removed only when used as keycaps.
fn component_len(text: &str) -> usize {
    let mut characters = text.char_indices();
    let Some((_, first)) = characters.next() else { return 0 };
    let mut end = first.len_utf8();
    if matches!(first, '0'..='9' | '#' | '*') {
        if text[end..].starts_with('\u{fe0f}') {
            end += '\u{fe0f}'.len_utf8();
        }
        return if text[end..].starts_with('\u{20e3}') {
            end + '\u{20e3}'.len_utf8()
        } else {
            0
        };
    }
    if !emoji_base(first) {
        return 0;
    }
    for (offset, character) in characters {
        if matches!(character, '\u{fe0e}' | '\u{fe0f}' | '\u{1f3fb}'..='\u{1f3ff}' | '\u{e0020}'..='\u{e007f}') {
            end = offset + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn sequence_len(text: &str) -> usize {
    let mut end = component_len(text);
    if end == 0 {
        return 0;
    }
    while text[end..].starts_with('\u{200d}') {
        let next = end + '\u{200d}'.len_utf8();
        let component = component_len(&text[next..]);
        if component == 0 {
            break;
        }
        end = next + component;
    }
    end
}

/// Omit emoji sequences from speech, leaving word boundaries and all other
/// Unicode intact. Plain text is borrowed; unsupported non-emoji text still
/// reaches the normal English frontend validation.
pub fn speech_without_emoji(text: &str) -> Result<Cow<'_, str>, FrontendError> {
    let mut result: Option<String> = None;
    let mut offset = 0;
    while offset < text.len() {
        let skipped = sequence_len(&text[offset..]);
        if skipped != 0 {
            if result.is_none() {
                let mut output = String::new();
                output.try_reserve_exact(text.len()).map_err(|_| FrontendError::Allocation)?;
                output.push_str(&text[..offset]);
                result = Some(output);
            }
            let output = result.as_mut().unwrap();
            if !output.is_empty() && !output.ends_with(char::is_whitespace) {
                output.push(' ');
            }
            offset += skipped;
        } else {
            let character = text[offset..].chars().next().unwrap();
            if let Some(output) = result.as_mut() {
                output.push(character);
            }
            offset += character.len_utf8();
        }
    }
    match result {
        None => Ok(Cow::Borrowed(text)),
        Some(mut output) => {
            output.truncate(output.trim_end().len());
            let leading = output.len() - output.trim_start().len();
            output.drain(..leading);
            Ok(Cow::Owned(output))
        }
    }
}

/// A nonempty emoji-only request completes silently, without an audio clip.
pub fn is_emoji_only(text: &str) -> bool {
    let mut offset = 0;
    let mut found = false;
    while offset < text.len() {
        let skipped = sequence_len(&text[offset..]);
        if skipped != 0 {
            found = true;
            offset += skipped;
        } else {
            let character = text[offset..].chars().next().unwrap();
            if !character.is_whitespace() {
                return false;
            }
            offset += character.len_utf8();
        }
    }
    found
}
