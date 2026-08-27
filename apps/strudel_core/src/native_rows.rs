//! Integer-only VM schema for `NativeRenderCommandV3`.
//!
//! The installed adapter currently returns its legacy eight-column event rows:
//! `[start,end,age,duration,midi,velocity,waveform,pan]`. They are converted to
//! oscillator commands. Once the JS adapter is switched it may emit the native
//! 23-column V1 schema, native V2 30-column schema, or V3's 31 columns:
//! `[start,end,age,duration,source_id,voice_id,kind,waveform,midi,gain,pan,
//!   playback_rate,sample_begin,sample_end,lpf,lpq,room,delay,phaser,shape,
//!   fm_depth,fm_rate,flags,attack,decay,release,filter_attack,filter_decay,
//!   sustain,filter_env_octaves_q8,filter_type]`.

extern crate alloc;

use alloc::vec::Vec;

use trueos::audio::{NativeRenderCommandV2, NativeRenderCommandV3};

use crate::json_rows::parse_integer_rows;

const LEGACY_COLUMNS: usize = 8;
const NATIVE_COLUMNS: usize = 23;
const NATIVE_V2_COLUMNS: usize = 30;
const NATIVE_V3_COLUMNS: usize = 31;

pub fn parse_native_command_rows(source: &str) -> Result<Vec<NativeRenderCommandV3>, &'static str> {
    let rows = parse_integer_rows(source)?;
    let mut commands = Vec::with_capacity(rows.len());
    for row in rows {
        let command = match row.len() {
            LEGACY_COLUMNS => into_v3(legacy_command(&row)?),
            NATIVE_COLUMNS => into_v3(native_command(&row, true)?),
            NATIVE_V2_COLUMNS => into_v3(native_command_v2(&row)?),
            NATIVE_V3_COLUMNS => native_command_v3(&row)?,
            _ => return Err("wrong native command column count"),
        };
        commands.push(command);
    }
    Ok(commands)
}

fn into_v3(base: NativeRenderCommandV2) -> NativeRenderCommandV3 {
    NativeRenderCommandV3 {
        base,
        filter_type: NativeRenderCommandV3::FILTER_12DB,
        reserved3: [0; 3],
        reserved4: 0,
    }
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

fn legacy_command(row: &[i64]) -> Result<NativeRenderCommandV2, &'static str> {
    let waveform = u8_value(row[6], "waveform")?;
    if waveform > 4 {
        return Err("invalid waveform");
    }
    let gain = u16_value(row[5], "velocity")?
        .checked_mul(32_767)
        .ok_or("gain overflow")?
        / 127;
    let command = NativeRenderCommandV2 {
        start_frame: u32_value(row[0], "start")?,
        end_frame: u32_value(row[1], "end")?,
        age_frames: u32_value(row[2], "age")?,
        duration_frames: u32_value(row[3], "duration")?,
        source_id: source_id("osc", waveform_name(waveform)),
        voice_id: 0,
        kind: NativeRenderCommandV2::KIND_OSCILLATOR,
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
        attack_frames: 240,
        decay_frames: 0,
        release_frames: 960,
        filter_attack_frames: 0,
        filter_decay_frames: 0,
        sustain_q15: 32_767,
        filter_env_octaves_q8: 0,
    };
    command
        .validate(u32::MAX)
        .map_err(|_| "invalid legacy command")?;
    Ok(command)
}

fn native_command(
    row: &[i64],
    validate_with_default_envelope: bool,
) -> Result<NativeRenderCommandV2, &'static str> {
    let command = NativeRenderCommandV2 {
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
        attack_frames: 240,
        decay_frames: 0,
        release_frames: 960,
        filter_attack_frames: 0,
        filter_decay_frames: 0,
        sustain_q15: 32_767,
        filter_env_octaves_q8: 0,
    };
    if validate_with_default_envelope {
        command
            .validate(u32::MAX)
            .map_err(|_| "invalid native command")?;
    }
    Ok(command)
}

fn native_command_v2(row: &[i64]) -> Result<NativeRenderCommandV2, &'static str> {
    // The 23-column prefix has only the legacy 20ms release default. Do not
    // validate a V2/V3 tail against that temporary value before installing
    // the row's actual envelope columns below.
    let mut command = native_command(&row[..NATIVE_COLUMNS], false)?;
    command.attack_frames = u32_value(row[23], "attack")?;
    command.decay_frames = u32_value(row[24], "decay")?;
    command.release_frames = u32_value(row[25], "release")?;
    command.filter_attack_frames = u32_value(row[26], "filter_attack")?;
    command.filter_decay_frames = u32_value(row[27], "filter_decay")?;
    command.sustain_q15 = u16_value(row[28], "sustain")?;
    command.filter_env_octaves_q8 = i16_value(row[29], "filter_env_octaves_q8")?;
    command
        .validate(u32::MAX)
        .map_err(|_| "invalid native v2 command")?;
    Ok(command)
}

fn native_command_v3(row: &[i64]) -> Result<NativeRenderCommandV3, &'static str> {
    let command = NativeRenderCommandV3 {
        base: native_command_v2(&row[..NATIVE_V2_COLUMNS])?,
        filter_type: u8_value(row[30], "filter_type")?,
        reserved3: [0; 3],
        reserved4: 0,
    };
    command
        .validate(u32::MAX)
        .map_err(|_| "invalid native v3 command")?;
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

    #[test]
    fn accepts_v2_envelope_columns_without_float_coercion() {
        let commands = parse_native_command_rows(
            "[[0,64,0,48,1,2,1,2,60,30000,0,65536,0,0,1200,2048,0,0,0,0,0,0,0,480,960,2400,120,360,16384,-1024]]",
        )
        .unwrap();
        let command = commands[0];
        assert_eq!(command.attack_frames, 480);
        assert_eq!(command.decay_frames, 960);
        assert_eq!(command.release_frames, 2400);
        assert_eq!(command.sustain_q15, 16_384);
        assert_eq!(command.filter_env_octaves_q8, -1024);
    }

    #[test]
    fn accepts_v3_filter_type_column() {
        let commands = parse_native_command_rows(
            "[[0,64,0,48,1,2,1,2,60,30000,0,65536,0,0,1200,2048,0,0,0,0,0,0,0,480,960,2400,120,360,16384,-1024,2]]",
        ).unwrap();
        assert_eq!(commands[0].filter_type, 2);
    }

    #[test]
    fn accepts_v3_release_tail_at_the_gate_boundary() {
        let commands = parse_native_command_rows(
            "[[0,960,96000,96000,2,1006632962,1,3,60,23737,-9175,65536,0,0,5200,0,3932,2621,0,3932,0,256,0,240,0,960,0,0,32767,0,0]]",
        )
        .unwrap();
        assert_eq!(commands[0].base.age_frames, 96_000);
        assert_eq!(commands[0].base.release_frames, 960);
    }

    #[test]
    fn accepts_v3_release_tail_beyond_the_legacy_default() {
        let commands = parse_native_command_rows(
            "[[0,2400,26400,24000,102,1020265062,1,2,60,6450,0,65536,0,0,2000,1536,6553,0,0,0,0,256,0,240,0,5760,0,0,32767,0,0]]",
        )
        .unwrap();
        assert_eq!(commands[0].base.age_frames, 26_400);
        assert_eq!(commands[0].base.duration_frames, 24_000);
        assert_eq!(commands[0].base.release_frames, 5_760);
    }
}
