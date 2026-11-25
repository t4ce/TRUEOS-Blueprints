use crate::{Buffer, Input, Message, MidiEvent, Node};
use hashbrown::HashMap;

struct Phrase {
    channel: u8,
    root: u8,
    next_step: usize,
    frames_remaining: f64,
}

/// A MIDI key-down schedules a one-shot sequence of pitch-ratio triggers.
pub struct MidiArp {
    channel: u8,
    step_beats: f64,
    offsets: Vec<i8>,
    sample_rate: usize,
    frames_per_step: f64,
    held: [u128; 16],
    phrase: Option<Phrase>,
}

impl MidiArp {
    pub fn new(
        channel: u8,
        step_beats: f32,
        offsets: Vec<i8>,
        sample_rate: usize,
        bpm: f32,
    ) -> Self {
        let mut node = Self {
            channel,
            step_beats: f64::from(step_beats),
            offsets,
            sample_rate,
            frames_per_step: 1.0,
            held: [0; 16],
            phrase: None,
        };
        node.set_bpm(bpm);
        node
    }

    fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            let frames =
                (self.sample_rate as f64 * 60.0 * self.step_beats / f64::from(bpm)).max(1.0);
            if let Some(phrase) = &mut self.phrase {
                phrase.frames_remaining *= frames / self.frames_per_step;
            }
            self.frames_per_step = frames;
        }
    }

    fn midi(&mut self, event: MidiEvent) {
        let channel = match event {
            MidiEvent::NoteOn { channel, .. }
            | MidiEvent::NoteOff { channel, .. }
            | MidiEvent::AllNotesOff { channel } => channel,
        };
        if channel >= 16 || (self.channel != 0 && self.channel != channel + 1) {
            return;
        }
        let held = &mut self.held[usize::from(channel)];
        match event {
            MidiEvent::NoteOn {
                note, velocity: 0, ..
            }
            | MidiEvent::NoteOff { note, .. }
                if note < 128 =>
            {
                *held &= !(1_u128 << note);
            }
            MidiEvent::NoteOn { note, velocity, .. }
                if note < 128 && (1..128).contains(&velocity) =>
            {
                let key = 1_u128 << note;
                if *held & key == 0 {
                    *held |= key;
                    self.phrase = (!self.offsets.is_empty()).then_some(Phrase {
                        channel,
                        root: note,
                        next_step: 0,
                        frames_remaining: self.frames_per_step,
                    });
                }
            }
            MidiEvent::AllNotesOff { .. } => {
                *held = 0;
                if self
                    .phrase
                    .as_ref()
                    .is_some_and(|phrase| phrase.channel == channel)
                {
                    self.phrase = None;
                }
            }
            _ => {}
        }
    }
}

impl<const N: usize> Node<N> for MidiArp {
    fn process(&mut self, _inputs: &mut HashMap<usize, Input<N>>, output: &mut [Buffer<N>]) {
        for sample in output[0].iter_mut() {
            *sample = 0.0;
            let Some(phrase) = &mut self.phrase else {
                continue;
            };
            if phrase.frames_remaining >= 1.0 {
                phrase.frames_remaining -= 1.0;
                continue;
            }
            let pitch = i16::from(phrase.root) + i16::from(self.offsets[phrase.next_step]);
            if (0..128).contains(&pitch) {
                *sample = 2.0_f32.powf((f32::from(pitch) - 60.0) / 12.0);
            }
            phrase.next_step += 1;
            if phrase.next_step == self.offsets.len() {
                self.phrase = None;
            } else {
                // Retain the fractional part to prevent rounding drift across steps.
                phrase.frames_remaining += self.frames_per_step - 1.0;
            }
        }
    }

    fn send_msg(&mut self, info: Message) {
        match info {
            Message::Midi(event) => self.midi(event),
            Message::ReleaseMidiNotes => {
                self.held.fill(0);
                self.phrase = None;
            }
            Message::SetBPM(bpm) => self.set_bpm(bpm),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render<const N: usize>(arp: &mut MidiArp) -> Vec<(usize, f32)> {
        let mut out = [Buffer::<N>::SILENT];
        arp.process(&mut HashMap::new(), &mut out);
        out[0]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, sample)| *sample > 0.0)
            .collect()
    }

    fn send(arp: &mut MidiArp, message: Message) {
        <MidiArp as Node<1>>::send_msg(arp, message);
    }

    fn key(channel: u8, note: u8) -> Message {
        Message::Midi(MidiEvent::NoteOn {
            channel,
            note,
            velocity: 100,
        })
    }

    #[test]
    fn note_on_starts_once_and_note_off_allows_the_phrase_to_finish() {
        let mut arp = MidiArp::new(1, 0.25, vec![0, 12, -12], 100, 120.0);
        send(&mut arp, key(1, 60));
        assert!(render::<16>(&mut arp).is_empty());

        send(&mut arp, key(0, 60));
        assert!(render::<8>(&mut arp).is_empty());
        send(&mut arp, key(0, 60));
        send(
            &mut arp,
            Message::Midi(MidiEvent::NoteOn {
                channel: 0,
                note: 60,
                velocity: 0,
            }),
        );
        assert_eq!(render::<32>(&mut arp), [(4, 1.0), (17, 2.0), (29, 0.5)]);

        send(&mut arp, key(0, 60));
        assert_eq!(render::<13>(&mut arp), [(12, 1.0)]);
    }

    #[test]
    fn newest_key_wins_and_tempo_changes_preserve_remaining_beats() {
        let mut arp = MidiArp::new(0, 0.25, vec![0, 10], 100, 120.0);
        send(&mut arp, key(0, 60));
        assert!(render::<5>(&mut arp).is_empty());
        send(&mut arp, key(1, 72));
        assert!(render::<5>(&mut arp).is_empty());
        send(
            &mut arp,
            Message::Midi(MidiEvent::AllNotesOff { channel: 0 }),
        );
        send(&mut arp, Message::SetBPM(60.0));
        assert_eq!(render::<16>(&mut arp), [(15, 2.0)]);
        send(
            &mut arp,
            Message::Midi(MidiEvent::AllNotesOff { channel: 1 }),
        );
        assert!(render::<30>(&mut arp).is_empty());

        send(&mut arp, key(1, 127));
        let trigger = 2.0_f32.powf(67.0 / 12.0);
        assert_eq!(render::<55>(&mut arp), [(25, trigger)]);
        send(&mut arp, key(1, 60));
        send(&mut arp, Message::ReleaseMidiNotes);
        assert!(render::<55>(&mut arp).is_empty());
    }
}
