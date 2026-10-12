//! WHATWG character references used by the translated tokenizer.
use crate::entities_data::NAMED;

/// Input begins immediately after `&`; the returned length excludes `&`.
pub(crate) fn decode(input: &str, in_attribute: bool) -> Option<(String, usize)> {
    if input.starts_with('#') {
        return numeric(input);
    }
    let limit = input
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == b';')
        .take(33)
        .count();
    for end in (1..=limit).rev() {
        let candidate = &input[..end];
        if let Ok(index) = NAMED.binary_search_by_key(&candidate, |(name, _)| *name) {
            if in_attribute
                && !candidate.ends_with(';')
                && input
                    .as_bytes()
                    .get(end)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'=')
            {
                return None;
            }
            return Some((NAMED[index].1.into(), end));
        }
    }
    None
}
fn numeric(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let (radix, mut position) = if matches!(bytes.get(1), Some(b'x' | b'X')) {
        (16, 2)
    } else {
        (10, 1)
    };
    let start = position;
    let mut value = 0u32;
    while let Some(digit) = bytes
        .get(position)
        .and_then(|c| (*c as char).to_digit(radix))
    {
        value = value.saturating_mul(radix).saturating_add(digit);
        position += 1;
    }
    if position == start {
        return None;
    }
    if bytes.get(position) == Some(&b';') {
        position += 1;
    }
    value = match value {
        0 | 0xD800..=0xDFFF | 0x110000..=u32::MAX => 0xFFFD,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        other => other,
    };
    Some((
        char::from_u32(value).unwrap_or('\u{FFFD}').to_string(),
        position,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_and_ambiguous_ampersands() {
        assert_eq!(
            decode("NotEqualTilde;", false),
            Some(("\u{2242}\u{338}".into(), 14))
        );
        assert_eq!(decode("ampx", false), Some(("&".into(), 3)));
        assert_eq!(decode("ampx", true), None);
        assert_eq!(decode("amp=", true), None);
        assert_eq!(decode("#x80;", false), Some(("€".into(), 5)));
        assert_eq!(decode("#0;", false), Some(("�".into(), 3)));
        assert_eq!(decode("#x110000;", false), Some(("�".into(), 9)));
    }
}
