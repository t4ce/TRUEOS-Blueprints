use std::{collections::VecDeque, f32::consts::TAU};

use crate::{Buffer, Input, Message, MidiEvent, Node};
use hashbrown::HashMap;

const MAX_VOICES: usize = 8;
const VOICE_GAIN: f32 = 0.12;

struct Voice {
    channel: u8,
    note: u8,
    phase: f32,
    phase_step: f32,
    level: f32,
    target: f32,
    level_step: f32,
    ramp_frames: usize,
}

impl Voice {
    fn new(channel: u8, note: u8, sample_rate: usize) -> Self {
        let frequency = 440.0 * 2.0_f32.powf((f32::from(note) - 69.0) / 12.0);
        Self {
            channel,
            note,
            phase: 0.0,
            phase_step: frequency / sample_rate as f32,
            level: 0.0,
            target: 0.0,
            level_step: 0.0,
            ramp_frames: 0,
        }
    }

    fn ramp(&mut self, target: f32, frames: usize) {
        self.target = target;
        self.ramp_frames = frames;
        if frames == 0 {
            self.level = target;
        } else {
            self.level_step = (target - self.level) / frames as f32;
        }
    }

    fn sample(&mut self) -> f32 {
        let sample = (self.phase * TAU).sin() * self.level;
        self.phase = (self.phase + self.phase_step).fract();
        if self.ramp_frames > 0 {
            self.ramp_frames -= 1;
            self.level = if self.ramp_frames == 0 {
                self.target
            } else {
                self.level + self.level_step
            };
        }
        sample
    }
}

pub struct MidiSynth {
    voices: VecDeque<Voice>,
    channel: u8,
    sample_rate: usize,
    attack_frames: usize,
    release_frames: usize,
}

impl MidiSynth {
    pub fn new(channel: u8, attack: f32, release: f32, sample_rate: usize) -> Self {
        Self {
            voices: VecDeque::with_capacity(MAX_VOICES),
            channel,
            sample_rate,
            attack_frames: (attack * sample_rate as f32).round() as usize,
            release_frames: (release * sample_rate as f32).round() as usize,
        }
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let existing = self
            .voices
            .iter()
            .position(|voice| voice.channel == channel && voice.note == note);
        let mut voice = existing
            .and_then(|index| self.voices.remove(index))
            .unwrap_or_else(|| Voice::new(channel, note, self.sample_rate));
        if self.voices.len() == MAX_VOICES {
            // Prefer a releasing voice, then the oldest remaining key.
            let index = self.voices.iter().position(|voice| voice.target == 0.0);
            self.voices.remove(index.unwrap_or(0));
        }
        voice.ramp(
            f32::from(velocity) * (VOICE_GAIN / 127.0),
            self.attack_frames,
        );
        self.voices.push_back(voice);
    }

    fn release(&mut self, channel: Option<u8>, note: Option<u8>) {
        for voice in &mut self.voices {
            if voice.target > 0.0
                && channel.is_none_or(|channel| voice.channel == channel)
                && note.is_none_or(|note| voice.note == note)
            {
                voice.ramp(0.0, self.release_frames);
            }
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
        match event {
            MidiEvent::NoteOn {
                note, velocity: 0, ..
            }
            | MidiEvent::NoteOff { note, .. } => {
                self.release(Some(channel), Some(note));
            }
            MidiEvent::NoteOn { note, velocity, .. } if note < 128 && velocity < 128 => {
                self.note_on(channel, note, velocity);
            }
            MidiEvent::AllNotesOff { .. } => self.release(Some(channel), None),
            _ => {}
        }
    }
}

impl<const N: usize> Node<N> for MidiSynth {
    fn process(&mut self, _inputs: &mut HashMap<usize, Input<N>>, output: &mut [Buffer<N>]) {
        for sample in &mut *output[0] {
            *sample = self.voices.iter_mut().map(Voice::sample).sum();
        }
        self.voices
            .retain(|voice| voice.target > 0.0 || voice.ramp_frames > 0);
    }

    fn send_msg(&mut self, info: Message) {
        match info {
            Message::Midi(event) => self.midi(event),
            Message::ReleaseMidiNotes => self.release(None, None),
            _ => {}
        }
    }
}
