//! Bounded TME1 reassembly. An incomplete picture invalidates the H.264
//! reference chain; resume only at a picture carrying SPS, PPS and IDR.
use std::collections::HashMap;
use trueos_rdp_protocol::Packet;

const MAX_AU_BYTES: usize = 4 * 1024 * 1024;
const MAX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
const MAX_INFLIGHT: usize = 8;
const REORDER_MS: u64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveEvent {
    Reset,
    AccessUnit(Vec<u8>),
}

struct Assembly {
    fragments: Vec<Option<Vec<u8>>>,
    received: usize,
    bytes: usize,
    since: u64,
}

pub struct Receiver {
    session: u32,
    next: Option<u32>,
    pending: HashMap<u32, Assembly>,
    bytes: usize,
    synchronized: bool,
    pub accepted: u64,
    pub resets: u64,
    pub rejected: u64,
}

impl Receiver {
    pub fn new(session: u32) -> Self {
        Self {
            session,
            next: None,
            pending: HashMap::new(),
            bytes: 0,
            synchronized: false,
            accepted: 0,
            resets: 0,
            rejected: 0,
        }
    }

    pub fn invalidate(&mut self) {
        self.synchronized = false;
    }

    fn reset(&mut self, events: &mut Vec<ReceiveEvent>) {
        self.synchronized = false;
        self.resets += 1;
        events.push(ReceiveEvent::Reset);
    }

    pub fn push(&mut self, datagram: &[u8], now_ms: u64) -> Vec<ReceiveEvent> {
        let mut events = Vec::new();
        let packet = match Packet::parse(datagram) {
            Ok(packet) if packet.session_id == self.session => packet,
            _ => {
                self.rejected += 1;
                return events;
            }
        };
        let next = *self.next.get_or_insert(packet.access_unit_seq);
        let distance = packet.access_unit_seq.wrapping_sub(next);
        if distance > i32::MAX as u32 {
            return events; // A late or duplicated picture already retired.
        }
        if distance > 64
            || (!self.pending.contains_key(&packet.access_unit_seq)
                && self.pending.len() == MAX_INFLIGHT)
        {
            self.pending.clear();
            self.bytes = 0;
            self.next = Some(packet.access_unit_seq);
            self.reset(&mut events);
        }
        let unit = self
            .pending
            .entry(packet.access_unit_seq)
            .or_insert_with(|| Assembly {
                fragments: vec![None; packet.fragment_count as usize],
                received: 0,
                bytes: 0,
                since: now_ms,
            });
        let index = packet.fragment_index as usize;
        if unit.fragments.len() != packet.fragment_count as usize
            || unit.fragments[index]
                .as_ref()
                .is_some_and(|p| p.as_slice() != packet.payload)
            || (unit.fragments[index].is_none()
                && (unit.bytes.saturating_add(packet.payload.len()) > MAX_AU_BYTES
                    || self.bytes.saturating_add(packet.payload.len()) > MAX_BUFFER_BYTES))
        {
            self.pending.clear();
            self.bytes = 0;
            self.next = Some(packet.access_unit_seq.wrapping_add(1));
            self.rejected += 1;
            self.reset(&mut events);
            return events;
        }
        if unit.fragments[index].is_none() {
            unit.fragments[index] = Some(packet.payload.to_vec());
            unit.received += 1;
            unit.bytes += packet.payload.len();
            self.bytes += packet.payload.len();
        }
        events.extend(self.poll(now_ms));
        events
    }

    pub fn poll(&mut self, now_ms: u64) -> Vec<ReceiveEvent> {
        let mut events = Vec::new();
        while let Some(next) = self.next {
            if self
                .pending
                .get(&next)
                .is_some_and(|u| u.received == u.fragments.len())
            {
                let unit = self.pending.remove(&next).unwrap();
                self.bytes -= unit.bytes;
                self.next = Some(next.wrapping_add(1));
                let mut bytes = Vec::with_capacity(unit.bytes);
                for fragment in unit.fragments {
                    bytes.extend(fragment.unwrap());
                }
                if independently_decodable(&bytes) {
                    self.synchronized = true;
                }
                if self.synchronized {
                    self.accepted += 1;
                    events.push(ReceiveEvent::AccessUnit(bytes));
                }
            } else {
                let oldest = self.pending.values().map(|u| u.since).min();
                if oldest.is_none_or(|since| now_ms.saturating_sub(since) < REORDER_MS) {
                    break;
                }
                if let Some(unit) = self.pending.remove(&next) {
                    self.bytes -= unit.bytes;
                }
                self.reset(&mut events);
                self.next = self
                    .pending
                    .keys()
                    .copied()
                    .min_by_key(|seq| seq.wrapping_sub(next));
            }
        }
        events
    }
}

pub fn independently_decodable(bytes: &[u8]) -> bool {
    let (mut sps, mut pps) = (false, false);
    let mut index = 0;
    while index + 3 < bytes.len() {
        let header = if bytes[index..].starts_with(&[0, 0, 0, 1]) {
            index + 4
        } else if bytes[index..].starts_with(&[0, 0, 1]) {
            index + 3
        } else {
            index += 1;
            continue;
        };
        if let Some(byte) = bytes.get(header) {
            match byte & 31 {
                7 => sps = true,
                8 => pps = sps,
                5 if sps && pps => return true,
                1 | 5 => return false,
                _ => {}
            }
        }
        index = header + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDR: &[u8] = &[0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 1, 0, 0, 1, 0x65, 1];
    const P: &[u8] = &[0, 0, 1, 0x41, 1];

    fn packet(seq: u32, index: u16, count: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 32 + payload.len()];
        bytes[..4].copy_from_slice(b"TME1");
        bytes[4] = 1;
        bytes[5] = u8::from(index == 0) | (u8::from(index + 1 == count) << 1);
        bytes[6..8].copy_from_slice(&32u16.to_be_bytes());
        bytes[8..12].copy_from_slice(&7u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&seq.to_be_bytes());
        bytes[20..22].copy_from_slice(&index.to_be_bytes());
        bytes[22..24].copy_from_slice(&count.to_be_bytes());
        bytes[24..26].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        let mut crc = !0u32;
        for byte in payload {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            }
        }
        bytes[28..32].copy_from_slice(&(!crc).to_be_bytes());
        bytes[32..].copy_from_slice(payload);
        bytes
    }

    #[test]
    fn reorders_fragments_and_ignores_duplicates() {
        let mut rx = Receiver::new(7);
        assert!(rx.push(&packet(1, 1, 2, &IDR[8..]), 0).is_empty());
        assert!(rx.push(&packet(1, 1, 2, &IDR[8..]), 1).is_empty());
        assert_eq!(
            rx.push(&packet(1, 0, 2, &IDR[..8]), 2),
            vec![ReceiveEvent::AccessUnit(IDR.to_vec())]
        );
        assert!(rx.push(&packet(1, 0, 2, &IDR[..8]), 3).is_empty());
    }

    #[test]
    fn lost_picture_requires_fresh_headers_and_idr() {
        let mut rx = Receiver::new(7);
        rx.push(&packet(0, 0, 1, IDR), 0);
        rx.push(&packet(1, 0, 2, P), 1);
        assert!(rx.push(&packet(2, 0, 1, P), 2).is_empty());
        assert_eq!(rx.poll(130), vec![ReceiveEvent::Reset]);
        assert!(rx.push(&packet(3, 0, 1, P), 131).is_empty());
        assert_eq!(
            rx.push(&packet(4, 0, 1, IDR), 132),
            vec![ReceiveEvent::AccessUnit(IDR.to_vec())]
        );
    }

    #[test]
    fn sequence_wrap_and_reordered_pictures() {
        let mut rx = Receiver::new(7);
        rx.push(&packet(u32::MAX - 1, 0, 1, IDR), 0);
        assert!(rx.push(&packet(0, 0, 1, P), 1).is_empty());
        assert_eq!(
            rx.push(&packet(u32::MAX, 0, 1, P), 2),
            vec![
                ReceiveEvent::AccessUnit(P.to_vec()),
                ReceiveEvent::AccessUnit(P.to_vec())
            ]
        );
    }

    #[test]
    fn untrusted_session_crc_and_headerless_keyframe_are_rejected() {
        let mut rx = Receiver::new(8);
        assert!(rx.push(&packet(0, 0, 1, IDR), 0).is_empty());
        let mut rx = Receiver::new(7);
        let mut corrupt = packet(0, 0, 1, IDR);
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(rx.push(&corrupt, 0).is_empty());
        assert!(!independently_decodable(&[0, 0, 1, 0x65, 1]));
        assert!(rx.push(&packet(0, 0, 1, P), 0).is_empty());
    }
}
