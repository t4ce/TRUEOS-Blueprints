#![no_std]

extern crate alloc;

mod audio_output;
mod event;
mod json_rows;
mod native_rows;
mod performance_input;
mod renderer;
mod strudel_vm;
mod tables;

use alloc::{format, string::String, vec::Vec};

use audio_output::AudioOutput;
use strudel_vm::StrudelVm;
use trueos::{
    audio::NativeBlockHeaderV3,
    logl::{self, level},
};

pub use performance_input::{PerformanceInputSource, PerformanceInputV1};

pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const BLOCK_FRAMES: usize = 2_400; // 50 ms
pub const DEFAULT_TARGET_QUEUE_FRAMES: usize = BLOCK_FRAMES * 6; // 300 ms lookahead
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;
const TRACE_BLOCKS_AFTER_COMMIT: u8 = 40;

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
    pub cps_numerator: u32,
    pub cps_denominator: u32,
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
    pattern_origin_frame: u64,
    last_queued_frames: usize,
    target_queue_frames: usize,
    buffer_frames: usize,
    cps_numerator: u32,
    cps_denominator: u32,
    performance_inputs: performance_input::PerformanceInputQueue,
    midi_read_seq: u64,
    keyboard_held: Vec<(u32, u8)>,
    mouse_seq: u32,
    trace_blocks_remaining: u8,
}

impl StrudelCore {
    pub fn boot() -> Result<Self, String> {
        logl::log(level::INFO, "strudel_core: startup stage=qjs-create-begin");
        let mut vm = StrudelVm::new();
        logl::log(level::INFO, "strudel_core: startup stage=qjs-install-begin");
        let install = vm.install()?;
        logl::log(level::INFO, "strudel_core: startup stage=qjs-install-ready");

        match trueos::audio::endpoint_capabilities_v1() {
            Ok(caps) if caps.ready != 0 => logl::log(
                level::INFO,
                format_args!(
                    "strudel_core: hda endpoint v{} ready={} active={}Hz/{}ch/{}bit frame_bytes={} dma_frames={} lane_frames={} oss={} addr64={} paths={} selected_path={} dac_pcm_rates={:#010X} dac_stream_formats={:#010X}",
                    caps.version,
                    caps.ready,
                    caps.sample_rate_hz,
                    caps.active_channels,
                    caps.active_sample_bits,
                    caps.active_frame_bytes,
                    caps.dma_buffer_frames,
                    caps.queued_pcm_lane_frames,
                    caps.controller_output_streams,
                    caps.controller_addr64,
                    caps.output_path_count,
                    caps.selected_output_path_index,
                    caps.selected_dac_pcm_rates,
                    caps.selected_dac_stream_formats,
                ),
            ),
            Ok(caps) => logl::log(
                level::WARN,
                format_args!(
                    "strudel_core: hda endpoint v{} unavailable ready={} (audio open will report its own error)",
                    caps.version, caps.ready
                ),
            ),
            Err(error) => logl::log(
                level::WARN,
                format_args!("strudel_core: hda endpoint capability query failed rc={error}"),
            ),
        }
        logl::log(level::INFO, "strudel_core: startup stage=audio-open-begin");
        let audio = AudioOutput::open()?;
        logl::log(level::INFO, "strudel_core: startup stage=audio-open-ready");
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
            pattern_origin_frame: 0,
            last_queued_frames,
            target_queue_frames,
            buffer_frames,
            cps_numerator: CPS_NUMERATOR,
            cps_denominator: CPS_DENOMINATOR,
            performance_inputs: performance_input::PerformanceInputQueue::default(),
            midi_read_seq: 0,
            keyboard_held: Vec::new(),
            mouse_seq: 0,
            trace_blocks_remaining: 0,
        })
    }

    /// Pump QuickJS jobs and refill the host-owned PCM lane to the configured
    /// lookahead. This method never waits on the browser or HTTP server.
    pub fn pump(&mut self) -> Result<PumpReport, String> {
        let first_pump = self.absolute_frame == 0;
        if first_pump {
            logl::log(level::INFO, "strudel_core: first-pump stage=qjs-poll-begin");
        }
        let mut diagnostics = self.vm.poll();
        if first_pump {
            logl::log(level::INFO, "strudel_core: first-pump stage=qjs-poll-ready");
            logl::log(
                level::INFO,
                "strudel_core: first-pump stage=midi-read-begin",
            );
        }
        let (midi_events, next_seq, dropped) = trueos::hid::midi_read_v1(self.midi_read_seq, 64);
        if first_pump {
            logl::log(
                level::INFO,
                format_args!(
                    "strudel_core: first-pump stage=midi-read-ready events={} dropped={}",
                    midi_events.len(),
                    dropped,
                ),
            );
        }
        self.midi_read_seq = next_seq;
        if dropped != 0 {
            diagnostics.push_str("; MIDI input ring dropped events");
        }
        for event in midi_events {
            self.submit_performance_input(PerformanceInputV1::midi(
                event.controller_id,
                event.note,
                event.velocity,
                event.gate != 0,
                0,
            ));
        }
        if first_pump {
            logl::log(
                level::INFO,
                "strudel_core: first-pump stage=keyboard-read-begin",
            );
        }
        let keyboards = trueos::hid::hid_hut_keyboards();
        if first_pump {
            logl::log(
                level::INFO,
                format_args!(
                    "strudel_core: first-pump stage=keyboard-read-ready devices={}",
                    keyboards.len(),
                ),
            );
        }
        let mut next_keyboard_held = Vec::new();
        for keyboard in keyboards {
            let device = keyboard
                .controller_id
                .wrapping_mul(0x9E37_79B9)
                .wrapping_add(keyboard.slot_id);
            for usage in keyboard.keys.into_iter().filter(|usage| *usage != 0) {
                next_keyboard_held.push((device, usage));
                if !self.keyboard_held.contains(&(device, usage)) {
                    self.submit_performance_input(PerformanceInputV1 {
                        source: PerformanceInputSource::Keyboard,
                        device,
                        control: usage as u32,
                        value: 100,
                        gate: true,
                        frame: 0,
                    });
                }
            }
        }
        let released = self
            .keyboard_held
            .iter()
            .copied()
            .filter(|key| !next_keyboard_held.contains(key))
            .collect::<Vec<_>>();
        for (device, usage) in released {
            self.submit_performance_input(PerformanceInputV1 {
                source: PerformanceInputSource::Keyboard,
                device,
                control: usage as u32,
                value: 0,
                gate: false,
                frame: 0,
            });
        }
        self.keyboard_held = next_keyboard_held;

        if first_pump {
            logl::log(
                level::INFO,
                "strudel_core: first-pump stage=mouse-read-begin",
            );
        }
        let mouse = trueos::hid::mouse_poll();
        if first_pump {
            logl::log(
                level::INFO,
                format_args!(
                    "strudel_core: first-pump stage=mouse-read-ready present={}",
                    u8::from(mouse.is_some()),
                ),
            );
            logl::log(
                level::INFO,
                "strudel_core: first-pump stage=queue-read-begin",
            );
        }
        if let Some(mouse) = mouse {
            if mouse.seq != self.mouse_seq {
                self.mouse_seq = mouse.seq;
                let gate = mouse.buttons & 1 != 0;
                for (control, value) in [(0, mouse.dx), (1, mouse.dy)] {
                    if value != 0 {
                        self.submit_performance_input(PerformanceInputV1 {
                            source: PerformanceInputSource::Pointer,
                            device: mouse.slot_id,
                            control,
                            value,
                            gate,
                            frame: 0,
                        });
                    }
                }
            }
        }
        let mut queued = self
            .audio
            .queued_frames()
            .map_err(|code| format!("audio queued-frames failed rc={code}"))?;
        if first_pump {
            logl::log(
                level::INFO,
                format_args!("strudel_core: first-pump stage=queue-read-ready frames={queued}"),
            );
        }

        while queued < self.target_queue_frames {
            let trace_block = first_pump;
            let input_batch = self
                .performance_inputs
                .take_through(self.absolute_frame.saturating_add(BLOCK_FRAMES as u64));
            if !input_batch.is_empty() {
                if trace_block {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "strudel_core: first-pump stage=input-apply-begin events={}",
                            input_batch.len(),
                        ),
                    );
                }
                self.vm.apply_performance_inputs(&input_batch)?;
                if trace_block {
                    logl::log(
                        level::INFO,
                        "strudel_core: first-pump stage=input-apply-ready",
                    );
                }
            }
            if trace_block {
                logl::log(
                    level::INFO,
                    format_args!(
                        "strudel_core: first-pump stage=pattern-query-begin absolute_frame={}",
                        self.absolute_frame,
                    ),
                );
            }
            let pattern_frame = self
                .absolute_frame
                .saturating_sub(self.pattern_origin_frame);
            let commands = self.vm.query_native_commands(
                pattern_frame,
                BLOCK_FRAMES as u32,
                SAMPLE_RATE_HZ,
            )?;
            if self.trace_blocks_remaining != 0 {
                let active = commands
                    .iter()
                    .map(|command| {
                        let base = &command.base;
                        (base.midi_note, base.age_frames, base.duration_frames)
                    })
                    .collect::<Vec<_>>();
                logl::log(
                    level::INFO,
                    format_args!(
                        "strudel_core: pattern-trace revision={} absolute_frame={} pattern_frame={} active={active:?}",
                        self.revision, self.absolute_frame, pattern_frame,
                    ),
                );
                self.trace_blocks_remaining -= 1;
            }
            if trace_block {
                logl::log(
                    level::INFO,
                    format_args!(
                        "strudel_core: first-pump stage=pattern-query-ready commands={}",
                        commands.len(),
                    ),
                );
                logl::log(
                    level::INFO,
                    "strudel_core: first-pump stage=native-render-begin",
                );
            }
            let header =
                NativeBlockHeaderV3::new(BLOCK_FRAMES as u32, self.absolute_frame, self.revision);
            let frames = self.audio.render_native(&header, &commands)?;
            if trace_block {
                logl::log(
                    level::INFO,
                    format_args!(
                        "strudel_core: first-pump stage=native-render-ready frames={frames}"
                    ),
                );
            }
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

    /// Queue an edge from a local input producer. It is applied in QuickJS
    /// before the first audio block whose frame range contains the event.
    pub fn submit_performance_input(&mut self, input: PerformanceInputV1) {
        self.performance_inputs.push(input, self.absolute_frame);
    }

    pub fn pending_performance_inputs(&self) -> usize {
        self.performance_inputs.len()
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
        let (cps_numerator, cps_denominator) = self.vm.cps()?;
        self.active_source.clear();
        self.active_source.push_str(source);
        self.runtime_status_json = runtime_status_json.clone();
        self.revision = self.revision.saturating_add(1);
        // Keep the host/HDA timeline monotonic while giving each accepted
        // program its own cycle-zero. Already queued PCM drains first; the new
        // revision begins at the next block produced after this transaction.
        self.pattern_origin_frame = self.absolute_frame;
        self.cps_numerator = cps_numerator;
        self.cps_denominator = cps_denominator;
        self.trace_blocks_remaining = TRACE_BLOCKS_AFTER_COMMIT;

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
            cps_numerator: self.cps_numerator,
            cps_denominator: self.cps_denominator,
        }
    }
}
