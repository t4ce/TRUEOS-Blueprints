#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderEvent {
    /// First frame in the current render block.
    pub start_frame: u32,
    /// Exclusive final frame in the current render block.
    pub end_frame: u32,
    /// Age of the voice at `start_frame`, measured from its whole-event onset.
    pub age_frames: u64,
    /// Whole-event duration. This lets the renderer apply release envelopes even
    /// when a hap crosses a block boundary.
    pub duration_frames: u64,
    pub midi_note: u8,
    pub velocity: u8,
    /// 0=sine, 1=square, 2=saw, 3=triangle, 4=noise.
    pub waveform: u8,
    /// -32768=left, 0=center, 32767=right.
    pub pan_q15: i16,
}
