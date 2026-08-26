#![no_std]

extern crate alloc;

mod audio_output;
mod event;
mod json_rows;
mod renderer;
mod strudel_vm;
mod tables;

use alloc::{format, string::String};

use trueos::{
    logl::{self, level},
    vsys,
};

use audio_output::AudioOutput;
use renderer::render_block;
use strudel_vm::StrudelVm;

const SAMPLE_RATE_HZ: u32 = 48_000;
const BLOCK_FRAMES: usize = 2_400; // 50 ms
const DEFAULT_TARGET_QUEUE_FRAMES: usize = BLOCK_FRAMES * 6; // 300 ms lookahead

// In Strudel/Tidal terms 0.5 cycles per second corresponds to a conventional
// 120 BPM four-beat cycle.
const CPS_NUMERATOR: u32 = 1;
const CPS_DENOMINATOR: u32 = 2;

fn main() {
    if let Err(error) = run() {
        logl::log(
            level::ERROR,
            format_args!("strudel_core: fatal integration error: {error}"),
        );
        loop {
            vsys::poll_once();
            vsys::sleep_ms(100);
        }
    }
}

fn run() -> Result<(), String> {
    let mut vm = StrudelVm::new();
    vm.install()?;

    let audio = AudioOutput::open()?;
    let buffer_frames = audio
        .buffer_frames()
        .unwrap_or(DEFAULT_TARGET_QUEUE_FRAMES * 4);
    let target_queue_frames = if buffer_frames > BLOCK_FRAMES {
        DEFAULT_TARGET_QUEUE_FRAMES.min(buffer_frames - BLOCK_FRAMES)
    } else {
        BLOCK_FRAMES
    };

    logl::log(
        level::INFO,
        format_args!(
            "strudel_core: temporal VM + PCM stream ready sample_rate={} block_frames={} queue_target={} buffer_frames={} cps={}/{}",
            SAMPLE_RATE_HZ,
            BLOCK_FRAMES,
            target_queue_frames,
            buffer_frames,
            CPS_NUMERATOR,
            CPS_DENOMINATOR,
        ),
    );

    let mut absolute_frame = 0u64;
    loop {
        let diagnostics = vm.poll();
        if !diagnostics.is_empty() {
            logl::log(
                level::DEBUG,
                format_args!("strudel_core/qjs: {diagnostics}"),
            );
        }

        let mut queued = audio.queued_frames().unwrap_or(0);
        while queued < target_queue_frames {
            let events = vm.query_frames(
                absolute_frame,
                BLOCK_FRAMES as u32,
                SAMPLE_RATE_HZ,
                CPS_NUMERATOR,
                CPS_DENOMINATOR,
            )?;
            let samples = render_block(BLOCK_FRAMES, &events);
            let frames = audio.write_all(&samples)?;
            if frames != BLOCK_FRAMES {
                return Err(format!(
                    "short completed audio block: wrote {frames}, expected {BLOCK_FRAMES}"
                ));
            }

            absolute_frame = absolute_frame.saturating_add(frames as u64);
            queued = queued.saturating_add(frames);
        }

        vsys::poll_once();
        vsys::sleep_ms(4);
    }
}
