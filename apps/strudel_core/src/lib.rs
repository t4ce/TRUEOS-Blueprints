#![no_std]

extern crate alloc;

mod audio_output;
mod event;
mod json_rows;
mod renderer;
mod strudel_vm;
mod tables;

use alloc::{format, string::String};

use audio_output::AudioOutput;
use renderer::render_block;
use strudel_vm::StrudelVm;

pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const BLOCK_FRAMES: usize = 2_400; // 50 ms
pub const DEFAULT_TARGET_QUEUE_FRAMES: usize = BLOCK_FRAMES * 6; // 300 ms lookahead
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;

// In Strudel/Tidal terms 0.5 cycles per second corresponds to a conventional
// 120 BPM four-beat cycle.
pub const CPS_NUMERATOR: u32 = 1;
pub const CPS_DENOMINATOR: u32 = 2;

#[derive(Clone, Debug)]
pub struct CoreSnapshot {
    pub revision: u64,
    pub source: String,
    pub runtime_status_json: String,
    pub absolute_frame: u64,
    pub queued_frames: usize,
    pub target_queue_frames: usize,
    pub buffer_frames: usize,
}

#[derive(Clone, Debug)]
pub struct CommitReport {
    pub revision: u64,
    pub runtime_status_json: String,
}

#[derive(Clone, Debug)]
pub struct PumpReport {
    pub diagnostics: String,
    pub queued_frames: usize,
}

/// UI-independent owner of QuickJS, temporal queries, synthesis, and the TRUEOS
/// PCM stream. The HTTP/Monaco layer communicates with this object only through
/// source commits and snapshots; browser code never touches audio or timing.
pub struct StrudelCore {
    vm: StrudelVm,
    audio: AudioOutput,
    active_source: String,
    runtime_status_json: String,
    revision: u64,
    absolute_frame: u64,
    last_queued_frames: usize,
    target_queue_frames: usize,
    buffer_frames: usize,
}

impl StrudelCore {
    pub fn boot() -> Result<Self, String> {
        let mut vm = StrudelVm::new();
        let install = vm.install()?;

        let audio = AudioOutput::open()?;
        let buffer_frames = audio
            .buffer_frames()
            .unwrap_or(DEFAULT_TARGET_QUEUE_FRAMES * 4);
        let target_queue_frames = if buffer_frames > BLOCK_FRAMES {
            DEFAULT_TARGET_QUEUE_FRAMES.min(buffer_frames - BLOCK_FRAMES)
        } else {
            BLOCK_FRAMES
        };
        let last_queued_frames = audio.queued_frames().unwrap_or(0);

        Ok(Self {
            vm,
            audio,
            active_source: install.initial_source.into(),
            runtime_status_json: install.core_status_json,
            revision: 1,
            absolute_frame: 0,
            last_queued_frames,
            target_queue_frames,
            buffer_frames,
        })
    }

    /// Pump QuickJS jobs and refill the host-owned PCM lane to the configured
    /// lookahead. This method never waits on the browser or HTTP server.
    pub fn pump(&mut self) -> Result<PumpReport, String> {
        let diagnostics = self.vm.poll();
        let mut queued = self
            .audio
            .queued_frames()
            .map_err(|code| format!("audio queued-frames failed rc={code}"))?;

        while queued < self.target_queue_frames {
            let events = self.vm.query_frames(
                self.absolute_frame,
                BLOCK_FRAMES as u32,
                SAMPLE_RATE_HZ,
                CPS_NUMERATOR,
                CPS_DENOMINATOR,
            )?;
            let samples = render_block(BLOCK_FRAMES, &events);
            let frames = self.audio.write_all(&samples)?;
            if frames != BLOCK_FRAMES {
                return Err(format!(
                    "short completed audio block: wrote {frames}, expected {BLOCK_FRAMES}"
                ));
            }

            self.absolute_frame = self.absolute_frame.saturating_add(frames as u64);
            queued = queued.saturating_add(frames);
        }

        self.last_queued_frames = queued;
        Ok(PumpReport {
            diagnostics,
            queued_frames: queued,
        })
    }

    /// Commit one JavaScript expression that yields a Pattern. The currently
    /// sounding pattern remains active when evaluation fails.
    pub fn commit_expression(&mut self, source: &str) -> Result<CommitReport, String> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(format!(
                "pattern source is too large: {} bytes (maximum {})",
                source.len(),
                MAX_SOURCE_BYTES
            ));
        }
        if source.trim().is_empty() {
            return Err("pattern source is empty".into());
        }

        let runtime_status_json = self.vm.commit_expression(source)?;
        self.active_source.clear();
        self.active_source.push_str(source);
        self.runtime_status_json = runtime_status_json.clone();
        self.revision = self.revision.saturating_add(1);

        Ok(CommitReport {
            revision: self.revision,
            runtime_status_json,
        })
    }

    pub fn snapshot(&self) -> CoreSnapshot {
        CoreSnapshot {
            revision: self.revision,
            source: self.active_source.clone(),
            runtime_status_json: self.runtime_status_json.clone(),
            absolute_frame: self.absolute_frame,
            queued_frames: self
                .audio
                .queued_frames()
                .unwrap_or(self.last_queued_frames),
            target_queue_frames: self.target_queue_frames,
            buffer_frames: self.buffer_frames,
        }
    }
}
