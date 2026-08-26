extern crate alloc;

use alloc::vec::Vec;

use crate::event::RenderEvent;

const EVENT_COLUMNS: usize = 8;

/// Parse the exact integer matrix emitted by `10_trueos_adapter.js`.
///
/// Accepted shape:
/// `[[start,end,age,duration,midi,velocity,wave,pan], ...]`
///
/// Keeping this parser tiny avoids pulling serde/json into the first bare-metal
/// experiment. It deliberately rejects strings, floats, objects and trailing
/// garbage.
pub fn parse_event_rows(source: &str) -> Result<Vec<RenderEvent>, &'static str> {
    let rows = parse_integer_rows(source)?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let columns: [i64; EVENT_COLUMNS] =
            row.try_into().map_err(|_| "wrong event column count")?;
        events.push(row_to_event(columns)?);
    }
    Ok(events)
}

/// Parse an integer-only JSON matrix. This is shared by the legacy event
/// boundary and the v1 native command boundary; strings, floats, objects and
/// trailing data are deliberately rejected before they reach audio code.
pub fn parse_integer_rows(source: &str) -> Result<Vec<Vec<i64>>, &'static str> {
    let mut parser = Parser::new(source.as_bytes());
    parser.space();
    parser.byte(b'[')?;
    parser.space();

    let mut events = Vec::new();
    if parser.take(b']') {
        parser.finish()?;
        return Ok(events);
    }

    loop {
        parser.space();
        parser.byte(b'[')?;
        let mut columns = Vec::new();
        loop {
            parser.space();
            columns.push(parser.integer()?);
            parser.space();
            if parser.take(b',') {
                continue;
            }
            break;
        }

        parser.space();
        parser.byte(b']')?;
        events.push(columns);
        parser.space();

        if parser.take(b',') {
            continue;
        }
        parser.byte(b']')?;
        parser.finish()?;
        return Ok(events);
    }
}

fn row_to_event(row: [i64; EVENT_COLUMNS]) -> Result<RenderEvent, &'static str> {
    let [start, end, age, duration, midi, velocity, waveform, pan] = row;
    if start < 0 || end < start || age < 0 || duration <= 0 {
        return Err("invalid event span");
    }
    if midi < 0 || midi > 127 || velocity < 0 || velocity > 127 {
        return Err("invalid MIDI event");
    }
    if waveform < 0 || waveform > 4 || pan < i16::MIN as i64 || pan > i16::MAX as i64 {
        return Err("invalid voice controls");
    }

    Ok(RenderEvent {
        start_frame: u32::try_from(start).map_err(|_| "start frame overflow")?,
        end_frame: u32::try_from(end).map_err(|_| "end frame overflow")?,
        age_frames: u64::try_from(age).map_err(|_| "age overflow")?,
        duration_frames: u64::try_from(duration).map_err(|_| "duration overflow")?,
        midi_note: midi as u8,
        velocity: velocity as u8,
        waveform: waveform as u8,
        pan_q15: pan as i16,
    })
}

struct Parser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Parser<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn byte(&mut self, expected: u8) -> Result<(), &'static str> {
        if self.take(expected) {
            Ok(())
        } else {
            Err("unexpected JSON token")
        }
    }

    fn integer(&mut self) -> Result<i64, &'static str> {
        let negative = self.take(b'-');
        let first = self.peek().ok_or("missing integer")?;
        if !first.is_ascii_digit() {
            return Err("expected integer");
        }

        let mut value = 0i64;
        while let Some(digit) = self.peek() {
            if !digit.is_ascii_digit() {
                break;
            }
            value = value
                .checked_mul(10)
                .and_then(|x| x.checked_add(i64::from(digit - b'0')))
                .ok_or("integer overflow")?;
            self.cursor += 1;
        }

        if negative {
            value.checked_neg().ok_or("integer overflow")
        } else {
            Ok(value)
        }
    }

    fn finish(&mut self) -> Result<(), &'static str> {
        self.space();
        if self.cursor == self.bytes.len() {
            Ok(())
        } else {
            Err("trailing JSON data")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_event_rows;

    #[test]
    fn parses_event_matrix() {
        let rows = parse_event_rows("[[0,2400,0,2400,60,100,3,0]]").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].midi_note, 60);
        assert_eq!(rows[0].end_frame, 2400);
    }

    #[test]
    fn rejects_non_integer_json() {
        assert!(parse_event_rows("[[0,1.5,0,1,60,100,0,0]]").is_err());
    }
}
