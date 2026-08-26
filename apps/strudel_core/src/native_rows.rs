//! Integer-only VM schema for `NativeRenderCommandV1`.
//!
//! The installed adapter currently returns its legacy eight-column event rows:
//! `[start,end,age,duration,midi,velocity,waveform,pan]`. They are converted to
//! oscillator commands. Once the JS adapter is switched it may emit the native
//! 23-column schema directly:
//! `[start,end,age,duration,source_id,voice_id,kind,waveform,midi,gain,pan,
//!   playback_rate,sample_begin,sample_end,lpf,lpq,room,delay,phaser,shape,
//!   fm_depth,fm_rate,flags]`.

extern crate alloc;

use alloc::vec::Vec;

use trueos::audio::NativeRenderCommandV1;

use crate::json_rows::parse_integer_rows;

const LEGACY_COLUMNS: usize = 8;
const NATIVE_COLUMNS: usize = 23;

pub fn parse_native_command_rows(source: &str) -> Result<Vec<NativeRenderCommandV1>, &'static str> {
    let rows = parse_integer_rows(source)?;
    let mut commands = Vec::with_capacity(rows.len());
    for row in rows {
        let command = match row.len() {
            LEGACY_COLUMNS => legacy_command(&row)?,
            NATIVE_COLUMNS => native_command(&row)?,
            _ => return Err("wrong native command column count"),
        };
        commands.push(command);
    }
    Ok(commands)
}

/// Stable source identity used for generated oscillator voices.
pub fn source_id(bank: &str, sound: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bank
        .bytes()
        .chain(core::iter::once(b':'))
        .chain(sound.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn legacy_command(row: &[i64]) -> Result<NativeRenderCommandV1, &'static str> {
    let waveform = u8_value(row[6], "waveform")?;
    if waveform > 4 {
        return Err("invalid waveform");
    }
    let gain = u16_value(row[5], "velocity")?
        .checked_mul(32_767)
        .ok_or("gain overflow")?
        / 127;
    let command = NativeRenderCommandV1 {
        start_frame: u32_value(row[0], "start")?,
        end_frame: u32_value(row[1], "end")?,
        age_frames: u32_value(row[2], "age")?,
        duration_frames: u32_value(row[3], "duration")?,
        source_id: source_id("osc", waveform_name(waveform)),
        voice_id: 0,
        kind: NativeRenderCommandV1::KIND_OSCILLATOR,
        waveform,
        midi_note: midi_value(row[4])?,
        gain_q15: gain,
        pan_q15: i16_value(row[7], "pan")?,
        playback_rate_q16: 65_536,
        sample_begin_q16: 0,
        sample_end_q16: 0,
        lpf_hz: 0,
        lpq_q8: 0,
        room_q15: 0,
        delay_q15: 0,
        phaser_q15: 0,
        shape_q15: 0,
        fm_depth_q8: 0,
        fm_rate_q8: 0,
        flags: 0,
        reserved0: 0,
        reserved1: 0,
        reserved2: 0,
    };
    command
        .validate(u32::MAX)
        .map_err(|_| "invalid legacy command")?;
    Ok(command)
}

fn native_command(row: &[i64]) -> Result<NativeRenderCommandV1, &'static str> {
    let command = NativeRenderCommandV1 {
        start_frame: u32_value(row[0], "start")?,
        end_frame: u32_value(row[1], "end")?,
        age_frames: u32_value(row[2], "age")?,
        duration_frames: u32_value(row[3], "duration")?,
        source_id: u64_value(row[4], "source_id")?,
        voice_id: u32_value(row[5], "voice_id")?,
        kind: u16_value(row[6], "kind")?,
        waveform: u8_value(row[7], "waveform")?,
        midi_note: midi_value(row[8])?,
        gain_q15: u16_value(row[9], "gain")?,
        pan_q15: i16_value(row[10], "pan")?,
        playback_rate_q16: i32_value(row[11], "playback_rate")?,
        sample_begin_q16: u32_value(row[12], "sample_begin")?,
        sample_end_q16: u32_value(row[13], "sample_end")?,
        lpf_hz: u16_value(row[14], "lpf")?,
        lpq_q8: u16_value(row[15], "lpq")?,
        room_q15: u16_value(row[16], "room")?,
        delay_q15: u16_value(row[17], "delay")?,
        phaser_q15: u16_value(row[18], "phaser")?,
        shape_q15: u16_value(row[19], "shape")?,
        fm_depth_q8: u16_value(row[20], "fm_depth")?,
        fm_rate_q8: u16_value(row[21], "fm_rate")?,
        flags: u32_value(row[22], "flags")?,
        reserved0: 0,
        reserved1: 0,
        reserved2: 0,
    };
    command
        .validate(u32::MAX)
        .map_err(|_| "invalid native command")?;
    Ok(command)
}

fn waveform_name(waveform: u8) -> &'static str {
    match waveform {
        1 => "square",
        2 => "saw",
        3 => "triangle",
        4 => "noise",
        _ => "sine",
    }
}
fn u64_value(value: i64, _: &'static str) -> Result<u64, &'static str> {
    u64::try_from(value).map_err(|_| "unsigned value out of range")
}
fn u32_value(value: i64, _: &'static str) -> Result<u32, &'static str> {
    u32::try_from(value).map_err(|_| "u32 value out of range")
}
fn u16_value(value: i64, _: &'static str) -> Result<u16, &'static str> {
    u16::try_from(value).map_err(|_| "u16 value out of range")
}
fn u8_value(value: i64, _: &'static str) -> Result<u8, &'static str> {
    u8::try_from(value).map_err(|_| "u8 value out of range")
}
fn i16_value(value: i64, _: &'static str) -> Result<i16, &'static str> {
    i16::try_from(value).map_err(|_| "i16 value out of range")
}
fn i32_value(value: i64, _: &'static str) -> Result<i32, &'static str> {
    i32::try_from(value).map_err(|_| "i32 value out of range")
}
fn midi_value(value: i64) -> Result<u8, &'static str> {
    let value = u8_value(value, "midi")?;
    if value <= 127 {
        Ok(value)
    } else {
        Err("invalid midi")
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_native_command_rows, source_id};

    #[test]
    fn fnv_source_id_is_stable() {
        assert_eq!(source_id("osc", "sine"), 0xe5ba_06b7_5dae_4c7f);
    }

    #[test]
    fn converts_legacy_vm_rows_to_typed_commands() {
        let commands = parse_native_command_rows("[[0,2400,0,2400,60,127,0,0]]").unwrap();
        assert_eq!(commands[0].kind, 1);
        assert_eq!(commands[0].gain_q15, 32_767);
        assert_eq!(commands[0].source_id, source_id("osc", "sine"));
    }

    #[test]
    fn accepts_the_documented_native_schema() {
        let commands = parse_native_command_rows(
            "[[0,32,0,64,42,7,1,3,60,30000,0,65536,0,0,0,0,0,0,0,0,0,0,0]]",
        )
        .unwrap();
        assert_eq!(commands[0].source_id, 42);
        assert_eq!(commands[0].voice_id, 7);
    }
}
