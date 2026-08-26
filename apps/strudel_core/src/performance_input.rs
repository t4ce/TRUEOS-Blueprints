//! Live-performance input contract for the Strudel VM.
//!
//! Input producers use absolute audio frames.  They can therefore be collected
//! by the host independently of the browser and applied immediately before a
//! temporal query without changing the pattern clock.

use alloc::vec::Vec;

/// Origin of a performance gesture. MIDI is deliberately the direct, primary
/// note vocabulary; keyboard and pointer are expressive fallback producers.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerformanceInputSource {
    Midi = 1,
    Keyboard = 2,
    Pointer = 3,
}

impl PerformanceInputSource {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// One input edge or continuous control update.
///
/// * MIDI: `control` is the MIDI note and `value` is velocity (0..127).
/// * keyboard: `control` is the HID usage/key code; `gate` holds/releases it.
/// * pointer: control 0 is signed X delta (pitch sweep), control 1 is signed
///   Y delta (gain); `gate` is the primary-contact state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PerformanceInputV1 {
    pub source: PerformanceInputSource,
    pub device: u32,
    pub control: u32,
    pub value: i32,
    pub gate: bool,
    pub frame: u64,
}

impl PerformanceInputV1 {
    pub const fn midi(device: u32, note: u8, velocity: u8, gate: bool, frame: u64) -> Self {
        Self {
            source: PerformanceInputSource::Midi,
            device,
            control: note as u32,
            value: velocity as i32,
            gate,
            frame,
        }
    }
}

pub const MAX_PENDING_PERFORMANCE_INPUTS: usize = 256;

/// Bounded, frame-ordered handoff owned by the audio/QuickJS task.
#[derive(Default)]
pub struct PerformanceInputQueue {
    pending: Vec<PerformanceInputV1>,
}

impl PerformanceInputQueue {
    pub fn push(&mut self, mut input: PerformanceInputV1, current_frame: u64) {
        // A producer that has no audio clock can use frame zero for "next
        // block". Never move explicitly timestamped events backwards.
        if input.frame == 0 {
            input.frame = current_frame;
        }
        if self.pending.len() == MAX_PENDING_PERFORMANCE_INPUTS {
            self.pending.remove(0);
        }
        let index = self
            .pending
            .partition_point(|queued| queued.frame <= input.frame);
        self.pending.insert(index, input);
    }

    pub fn take_through(&mut self, inclusive_frame: u64) -> Vec<PerformanceInputV1> {
        let split = self
            .pending
            .partition_point(|input| input.frame <= inclusive_frame);
        self.pending.drain(..split).collect()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_frame_means_next_audio_block() {
        let mut queue = PerformanceInputQueue::default();
        queue.push(PerformanceInputV1::midi(2, 60, 90, true, 0), 480);
        assert!(queue.take_through(479).is_empty());
        assert_eq!(queue.take_through(480)[0].control, 60);
    }

    #[test]
    fn later_events_wait_for_their_frame() {
        let mut queue = PerformanceInputQueue::default();
        queue.push(PerformanceInputV1::midi(2, 60, 90, true, 100), 0);
        queue.push(PerformanceInputV1::midi(2, 64, 90, true, 10), 0);
        assert_eq!(queue.take_through(20)[0].control, 64);
        assert_eq!(queue.take_through(100)[0].control, 60);
    }
}
