//! TRUEOS network piano snapshots formerly received by the kernel on UDP 9696.

use glicol::MidiEvent;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const CHANNEL: u8 = 0;
const KEYS: usize = 96;
const HEADER_BYTES: usize = 14;
const STATE_BYTES: usize = HEADER_BYTES + KEYS / 8;
const PRESSURE_BYTES: usize = STATE_BYTES + KEYS * 2;
const TEXT_BASE_NOTE: u8 = 36;
const DEFAULT_PRESSURE: i16 = 64;
const RELEASE_PRESSURE: i16 = 8;
const ATTACK_PRESSURE: i16 = 12;
const STATE_TIMEOUT: Duration = Duration::from_secs(2);

struct Frame {
    module: u8,
    base_note: u8,
    seq: Option<u16>,
    mask: u128,
    pressure: [i16; KEYS],
    text: bool,
}

impl Frame {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.starts_with(b"piano ") {
            return Self::text(data);
        }
        if !data.starts_with(b"TPNO") || data.get(7).copied()? as usize != KEYS {
            return None;
        }
        match (data[4], data.len()) {
            (1, PRESSURE_BYTES) | (2, STATE_BYTES) => {}
            _ => return None,
        }
        if data[6] >= 128 {
            return None;
        }
        let mut mask_bytes = [0; 16];
        mask_bytes[..KEYS / 8].copy_from_slice(&data[HEADER_BYTES..STATE_BYTES]);
        let mask = u128::from_le_bytes(mask_bytes);
        let mut pressure = default_pressure(mask);
        for (value, bytes) in pressure.iter_mut().zip(data[STATE_BYTES..].chunks_exact(2)) {
            *value = i16::from_le_bytes([bytes[0], bytes[1]]);
        }
        Some(Self {
            module: data[5],
            base_note: data[6],
            seq: Some(u16::from_le_bytes([data[8], data[9]])),
            mask,
            pressure,
            text: false,
        })
    }

    fn text(data: &[u8]) -> Option<Self> {
        let mut fields = core::str::from_utf8(data).ok()?.split_ascii_whitespace();
        if fields.next()? != "piano" {
            return None;
        }
        let mut seq = None;
        let mut mask = None;
        let mut deltas = None;
        for (key, value) in fields.filter_map(|field| field.split_once('=')) {
            match key {
                "seq" => seq = Some(u32::try_from(parse_unsigned(value)?).ok()? as u16),
                "mask" => mask = Some(parse_unsigned(value)?),
                "deltas" => deltas = Some(value),
                _ => {}
            }
        }
        let mask = mask?;
        let pressure = match deltas {
            None => default_pressure(mask),
            Some(deltas) => {
                let mut pressure = [0; KEYS];
                for (index, value) in deltas.split(',').enumerate() {
                    let raw = match hex_digits(value) {
                        Some(hex) => i32::from_str_radix(hex, 16).ok()?,
                        None => value.parse().ok()?,
                    };
                    *pressure.get_mut(index)? = raw.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
                }
                pressure
            }
        };
        Some(Self {
            module: 0,
            base_note: TEXT_BASE_NOTE,
            seq,
            mask,
            pressure,
            text: true,
        })
    }
}

fn hex_digits(value: &str) -> Option<&str> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
}

fn parse_unsigned(value: &str) -> Option<u128> {
    match hex_digits(value) {
        Some(hex) => u128::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

fn default_pressure(mask: u128) -> [i16; KEYS] {
    std::array::from_fn(|key| {
        if mask & (1 << key) != 0 {
            DEFAULT_PRESSURE
        } else {
            0
        }
    })
}

fn attack_velocity(pressure: i16, previous: i16) -> u8 {
    let pos = pressure.clamp(ATTACK_PRESSURE, 100) as u16 - ATTACK_PRESSURE as u16;
    let linear = pos * 255 / (100 - ATTACK_PRESSURE as u16);
    let curved = (linear + linear * linear / 255) / 2;
    let rise = pressure.saturating_sub(previous).clamp(0, 48) as u16;
    (1 + curved * 126 / 255 + rise * 10 / 48).min(127) as u8
}

struct Session {
    peer: SocketAddr,
    module: u8,
    base_note: u8,
    seq: Option<u16>,
    expires: Option<Instant>,
    held: u128,
    pressure: [i16; KEYS],
}

#[derive(Default)]
pub struct Piano {
    session: Option<Session>,
}

impl Piano {
    pub fn on_packet(
        &mut self,
        peer: SocketAddr,
        data: &[u8],
        now: Instant,
        mut send: impl FnMut(MidiEvent),
    ) -> bool {
        self.expire(now, &mut send);
        let Some(frame) = Frame::parse(data) else {
            return false;
        };
        if let Some(session) = &self.session {
            if session.peer != peer {
                return false;
            }
            if session.module != frame.module || session.base_note != frame.base_note {
                self.clear(&mut send);
            } else if let (Some(last), Some(next)) = (session.seq, frame.seq)
                && !(1..0x8000).contains(&next.wrapping_sub(last))
            {
                return false;
            }
        }
        let session = self.session.get_or_insert_with(|| Session {
            peer,
            module: frame.module,
            base_note: frame.base_note,
            seq: None,
            expires: None,
            held: 0,
            pressure: [0; KEYS],
        });
        let mut held = 0;
        for key in 0..KEYS.min(128 - usize::from(frame.base_note)) {
            let bit = 1 << key;
            let was_held = session.held & bit != 0;
            let pressure = frame.pressure[key];
            let minimum_pressure = if was_held {
                RELEASE_PRESSURE + 1
            } else {
                ATTACK_PRESSURE
            };
            let is_held = frame.mask & bit != 0 && pressure >= minimum_pressure;
            if is_held {
                held |= bit;
            }
            let note = frame.base_note + key as u8;
            match (was_held, is_held) {
                (false, true) => send(MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note,
                    velocity: attack_velocity(pressure, session.pressure[key]),
                }),
                (true, false) => send(MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note,
                }),
                _ => {}
            }
        }
        session.held = held;
        session.pressure = frame.pressure;
        session.seq = frame.seq.or(session.seq);
        // The standalone text sender only sends changes. A held key in that
        // format stays down until the next snapshot clears it.
        session.expires = (!frame.text || held == 0).then(|| now + STATE_TIMEOUT);
        true
    }

    pub fn expire(&mut self, now: Instant, send: impl FnMut(MidiEvent)) {
        if self
            .session
            .as_ref()
            .and_then(|session| session.expires)
            .is_some_and(|deadline| now >= deadline)
        {
            self.clear(send);
        }
    }

    pub fn clear(&mut self, mut send: impl FnMut(MidiEvent)) {
        if let Some(session) = self.session.take() {
            let mut held = session.held;
            while held != 0 {
                let key = held.trailing_zeros() as u8;
                held &= held - 1;
                send(MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note: session.base_note + key,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_state(seq: u16, mask: u8) -> [u8; STATE_BYTES] {
        let mut data = [0; STATE_BYTES];
        data[..4].copy_from_slice(b"TPNO");
        data[4] = 2;
        data[6] = 60;
        data[7] = KEYS as u8;
        data[8..10].copy_from_slice(&seq.to_le_bytes());
        data[HEADER_BYTES] = mask;
        data
    }

    #[test]
    fn binary_state_ignores_duplicates_and_reordering_across_wrap() {
        let peer = "127.0.0.1:5123".parse().unwrap();
        let now = Instant::now();
        let mut piano = Piano::default();
        let mut events = Vec::new();
        let mut send = |event| events.push(event);

        assert!(piano.on_packet(peer, &binary_state(65535, 1), now, &mut send));
        assert!(!piano.on_packet(peer, &binary_state(65535, 0), now, &mut send));
        assert!(piano.on_packet(peer, &binary_state(0, 2), now, &mut send));
        assert!(!piano.on_packet(peer, &binary_state(65534, 0), now, &mut send));
        piano.expire(now + STATE_TIMEOUT, send);
        assert_eq!(
            events,
            [
                MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note: 60,
                    velocity: 69
                },
                MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note: 60
                },
                MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note: 61,
                    velocity: 69
                },
                MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note: 61
                },
            ]
        );
    }

    #[test]
    fn binary_pressure_retains_hysteresis_and_accepts_restart_after_timeout() {
        let peer = "127.0.0.1:5123".parse().unwrap();
        let now = Instant::now();
        let mut piano = Piano::default();
        let mut events = Vec::new();
        let mut send = |event| events.push(event);
        let mut data = [0; PRESSURE_BYTES];
        let mut packet = |seq, pressure: i16| {
            data[..STATE_BYTES].copy_from_slice(&binary_state(seq, 1));
            data[4] = 1;
            data[STATE_BYTES..STATE_BYTES + 2].copy_from_slice(&pressure.to_le_bytes());
            data
        };

        assert!(piano.on_packet(peer, &packet(55, 11), now, &mut send));
        assert!(piano.on_packet(peer, &packet(56, 70), now, &mut send));
        assert!(piano.on_packet(peer, &packet(57, 9), now, &mut send));
        assert!(piano.on_packet(peer, &packet(58, 8), now, &mut send));
        assert!(piano.on_packet(peer, &packet(0, 64), now + STATE_TIMEOUT, send));
        assert_eq!(
            events,
            [
                MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note: 60,
                    velocity: 79
                },
                MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note: 60
                },
                MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note: 60,
                    velocity: 69
                },
            ]
        );
    }

    #[test]
    fn text_sender_holds_through_state_timeout_and_releases_on_zero_mask() {
        let peer = "127.0.0.1:5123".parse().unwrap();
        let other = "127.0.0.1:5124".parse().unwrap();
        let now = Instant::now();
        let mut piano = Piano::default();
        let mut events = Vec::new();
        let mut send = |event| events.push(event);
        assert!(piano.on_packet(peer, b"piano seq=42 mask=0x1 deltas=64", now, &mut send));
        assert!(!piano.on_packet(other, b"piano seq=43 mask=0", now, &mut send));
        let later = now + Duration::from_secs(60);
        piano.expire(later, &mut send);
        assert!(piano.on_packet(peer, b"piano seq=43 mask=0", later, send));
        assert_eq!(
            events,
            [
                MidiEvent::NoteOn {
                    channel: CHANNEL,
                    note: 36,
                    velocity: 69
                },
                MidiEvent::NoteOff {
                    channel: CHANNEL,
                    note: 36
                },
            ]
        );
    }
}
