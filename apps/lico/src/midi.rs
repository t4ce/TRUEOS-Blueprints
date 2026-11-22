use crate::piano_udp::Piano;
use glicol::MidiEvent;
use std::io;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Instant;

const PIANO_PORT: u16 = 9696;
const MAX_PACKETS_PER_TICK: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    Usb,
    Wifi,
    Off,
}

impl Source {
    pub const CHOICES: &'static [Self] = &[
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        Self::Usb,
        Self::Wifi,
        Self::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            Self::Usb => "USB MIDI",
            Self::Wifi => "Wi-Fi piano · UDP 9696",
            Self::Off => "Off",
        }
    }
}

pub enum Input {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    Usb {
        cursor: Option<u64>,
    },
    Wifi {
        socket: UdpSocket,
        piano: Piano,
    },
    Disabled,
}

impl Default for Input {
    fn default() -> Self {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        return Self::Usb { cursor: None };
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        Self::Disabled
    }
}

impl Input {
    pub fn open(source: Source) -> io::Result<Self> {
        match source {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            Source::Usb => Ok(Self::Usb { cursor: None }),
            Source::Wifi => Self::bind_piano(),
            Source::Off => Ok(Self::Disabled),
        }
    }

    fn bind_piano() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PIANO_PORT)).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Wi-Fi piano could not bind UDP {PIANO_PORT}: {error}"),
            )
        })?;
        socket.set_nonblocking(true)?;
        Ok(Self::Wifi {
            socket,
            piano: Piano::default(),
        })
    }

    pub fn source(&self) -> Source {
        match self {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            Self::Usb { .. } => Source::Usb,
            Self::Wifi { .. } => Source::Wifi,
            Self::Disabled => Source::Off,
        }
    }

    pub fn needs_idle_poll(&self) -> bool {
        matches!(self, Self::Wifi { .. })
    }

    pub fn reset(&mut self) {
        match self {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            Self::Usb { cursor } => *cursor = None,
            Self::Wifi { piano, .. } => *piano = Piano::default(),
            Self::Disabled => {}
        }
    }

    pub fn poll(&mut self, listening: bool, mut send: impl FnMut(MidiEvent)) -> io::Result<()> {
        match self {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            Self::Usb { cursor } if listening => poll_usb(cursor, send),
            Self::Wifi { socket, piano } => {
                let now = Instant::now();
                if listening {
                    piano.expire(now, &mut send);
                }
                let mut bytes = [0; 1024];
                for _ in 0..MAX_PACKETS_PER_TICK {
                    let (len, peer) = match socket.recv_from(&mut bytes) {
                        Ok(result) => result,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            // TRUEOS can report a queued bind failure after bind() returns.
                            if let Some(error) = socket.take_error()? {
                                return Err(error);
                            }
                            break;
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error),
                    };
                    if listening && len < bytes.len() {
                        piano.on_packet(peer, &bytes[..len], now, &mut send);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn poll_usb(cursor: &mut Option<u64>, mut send: impl FnMut(MidiEvent)) {
    const BATCH: u32 = 64;
    let Some(seq) = *cursor else {
        // Start at the head of the 256-event ring so old key presses do not replay.
        let mut head = 0;
        for _ in 0..4 {
            let (events, next, _) = v::vinput::midi_read_v1(head, BATCH);
            head = next;
            if events.len() < BATCH as usize {
                break;
            }
        }
        *cursor = Some(head);
        return;
    };
    let (events, next, dropped) = v::vinput::midi_read_v1(seq, BATCH);
    *cursor = Some(next);
    if dropped != 0 {
        for channel in 0..16 {
            send(MidiEvent::AllNotesOff { channel });
        }
    }
    for event in events {
        let channel = event.channel;
        let note = event.note;
        send(if event.gate != 0 && event.velocity != 0 {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity: event.velocity,
            }
        } else {
            MidiEvent::NoteOff { channel, note }
        });
    }
}

#[derive(Default)]
pub struct Decoder {
    status: u8,
    first_data: Option<u8>,
}

impl Decoder {
    pub fn push(&mut self, byte: u8) -> Option<MidiEvent> {
        match byte {
            // Real-time bytes can occur between any two bytes of a message.
            0xf8..=0xff => return None,
            0xf0..=0xf7 => {
                self.status = 0;
                self.first_data = None;
                return None;
            }
            0x80..=0xef => {
                self.status = byte;
                self.first_data = None;
                return None;
            }
            _ => {}
        }

        let kind = self.status & 0xf0;
        if !matches!(kind, 0x80 | 0x90 | 0xb0) {
            return None;
        }
        let Some(first) = self.first_data.take() else {
            self.first_data = Some(byte);
            return None;
        };
        let channel = self.status & 0x0f;
        match (kind, first, byte) {
            (0x80, note, _) | (0x90, note, 0) => Some(MidiEvent::NoteOff { channel, note }),
            (0x90, note, velocity) => Some(MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            }),
            (0xb0, 123, 0) => Some(MidiEvent::AllNotesOff { channel }),
            _ => None,
        }
    }
}

// Raw MIDI 1.0 bytes on channel 1, delivered at these elapsed times in ms.
const PHRASE: &[(usize, &[u8])] = &[
    (0, &[0x90, 60, 105]),
    (55, &[0xf8, 64, 89, 67, 78]),
    (490, &[60, 0, 64, 0, 67, 0]),
    (620, &[0x90, 62, 98, 65, 84, 69, 76]),
    (1_080, &[0x80, 62, 0, 65, 0, 69, 0]),
    (1_230, &[0x90, 60, 103, 64, 88, 72, 80]),
    (1_790, &[0xb0, 123, 0]),
];

#[derive(Default)]
pub struct Demo {
    decoder: Decoder,
    frames: usize,
    segment: usize,
}

impl Demo {
    pub fn advance(
        &mut self,
        sample_rate: u32,
        frames: usize,
        mut send: impl FnMut(MidiEvent),
    ) -> bool {
        let elapsed_ms = self.frames * 1000 / sample_rate as usize;
        while let Some(&(at, bytes)) = PHRASE.get(self.segment) {
            if at > elapsed_ms {
                break;
            }
            for &byte in bytes {
                if let Some(event) = self.decoder.push(byte) {
                    send(event);
                }
            }
            self.segment += 1;
        }
        self.frames += frames;
        self.segment < PHRASE.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_split_messages_and_running_status() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(0x92).is_none());
        assert!(decoder.push(60).is_none());
        let events: Vec<_> = [0xf8, 96, 64, 88, 60, 0, 0x82, 64, 63]
            .into_iter()
            .filter_map(|byte| decoder.push(byte))
            .collect();
        assert_eq!(
            events,
            [
                MidiEvent::NoteOn {
                    channel: 2,
                    note: 60,
                    velocity: 96
                },
                MidiEvent::NoteOn {
                    channel: 2,
                    note: 64,
                    velocity: 88
                },
                MidiEvent::NoteOff {
                    channel: 2,
                    note: 60
                },
                MidiEvent::NoteOff {
                    channel: 2,
                    note: 64
                },
            ]
        );
    }

    #[test]
    fn system_common_cancels_running_status() {
        let mut decoder = Decoder::default();
        let events: Vec<_> = [0x90, 60, 0xf1, 99, 61, 98, 0xb3, 64, 127, 123, 0]
            .into_iter()
            .filter_map(|byte| decoder.push(byte))
            .collect();
        assert_eq!(events, [MidiEvent::AllNotesOff { channel: 3 }]);
    }
}
