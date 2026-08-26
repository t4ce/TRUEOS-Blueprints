extern crate alloc;

use alloc::{vec, vec::Vec};

use crate::event::RenderEvent;
use crate::tables::{MIDI_PHASE_INC_Q32, SINE_Q15};

const ATTACK_FRAMES: u64 = 240; // 5 ms at 48 kHz
const RELEASE_FRAMES: u64 = 960; // 20 ms at 48 kHz
const Q15_ONE: i64 = 32_767;
const MIX_HEADROOM_DIVISOR: i64 = 3;

/// Render one block of interleaved stereo i16.
///
/// This intentionally uses a tiny deterministic synth. Strudel remains the
/// temporal engine; replacing this renderer with samples, a better synth, or a
/// native TRUEOS mixer does not change the pattern/QJS boundary.
pub fn render_block(frame_count: usize, events: &[RenderEvent]) -> Vec<i16> {
    let mut mix = vec![0i32; frame_count.saturating_mul(2)];

    for event in events {
        let start = usize::try_from(event.start_frame)
            .unwrap_or(frame_count)
            .min(frame_count);
        let end = usize::try_from(event.end_frame)
            .unwrap_or(frame_count)
            .min(frame_count);
        if start >= end || event.velocity == 0 {
            continue;
        }

        let increment = MIDI_PHASE_INC_Q32[event.midi_note as usize];
        let mut phase = u32::try_from(
            u64::from(increment)
                .wrapping_mul(event.age_frames)
                & u64::from(u32::MAX),
        )
        .unwrap_or(0);

        let pan = i32::from(event.pan_q15);
        let left_gain = if pan > 0 { 32_767 - pan } else { 32_767 };
        let right_gain = if pan < 0 { 32_767 + pan } else { 32_767 };

        for frame in start..end {
            let local = u64::try_from(frame - start).unwrap_or(0);
            let age = event.age_frames.saturating_add(local);
            let envelope = envelope_q15(age, event.duration_frames);
            if envelope == 0 {
                phase = phase.wrapping_add(increment);
                continue;
            }

            let raw = i64::from(wave_sample(event.waveform, phase, age));
            let voiced = raw
                .saturating_mul(i64::from(event.velocity))
                .saturating_mul(envelope)
                / (127 * Q15_ONE * MIX_HEADROOM_DIVISOR);

            let left = voiced.saturating_mul(i64::from(left_gain)) / Q15_ONE;
            let right = voiced.saturating_mul(i64::from(right_gain)) / Q15_ONE;
            let index = frame * 2;
            mix[index] = mix[index].saturating_add(clamp_i64_to_i32(left));
            mix[index + 1] = mix[index + 1].saturating_add(clamp_i64_to_i32(right));
            phase = phase.wrapping_add(increment);
        }
    }

    mix.into_iter().map(clamp_i32_to_i16).collect()
}

fn envelope_q15(age: u64, duration: u64) -> i64 {
    if age >= duration {
        return 0;
    }

    let attack = if age >= ATTACK_FRAMES {
        Q15_ONE
    } else {
        i64::try_from(age.saturating_mul(Q15_ONE as u64) / ATTACK_FRAMES).unwrap_or(Q15_ONE)
    };

    let remaining = duration - age;
    let release = if remaining >= RELEASE_FRAMES {
        Q15_ONE
    } else {
        i64::try_from(remaining.saturating_mul(Q15_ONE as u64) / RELEASE_FRAMES)
            .unwrap_or(Q15_ONE)
    };

    attack.min(release)
}

fn wave_sample(waveform: u8, phase: u32, age: u64) -> i16 {
    match waveform {
        1 => {
            if phase & 0x8000_0000 == 0 {
                i16::MAX
            } else {
                i16::MIN + 1
            }
        }
        2 => {
            let value = i32::from((phase >> 16) as u16) - 32_768;
            value as i16
        }
        3 => {
            let x = i32::from((phase >> 16) as u16);
            let value = if x < 32_768 {
                x * 2 - 32_768
            } else {
                98_302 - x * 2
            };
            value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        }
        4 => {
            // Stateless deterministic noise: continuity follows absolute voice age.
            let mut x = phase ^ (age as u32).wrapping_mul(0x9E37_79B9);
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 16) as i16
        }
        _ => SINE_Q15[(phase >> 24) as usize],
    }
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn clamp_i32_to_i16(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}
