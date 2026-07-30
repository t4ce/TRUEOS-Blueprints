//! Dedicated producer side of the GridPaper kernel snapshot transport.
//!
//! This intentionally exposes no generic UI or drawing calls. The Blueprint
//! publishes its fixed wire image; the kernel service owns presentation and
//! GPU residency.

pub const COLUMNS: usize = 39;
pub const ROWS: usize = 55;
pub const CELL_BYTES: usize = 13;
pub const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;
pub const TEXT_COLOR_ANIMATION_SLOTS: usize = 17;
pub const COLOR_KEYFRAME_CAPACITY: usize = 8;
pub const MIN_ANIMATION_DURATION_MS: u32 = 16;
pub const MAX_ANIMATION_DURATION_MS: u32 = 600_000;
/// A GridPaper Blueprint owns one document scene. Kernel-side replication is
/// expressed by launching another Blueprint, not by multiplying scenes inside
/// one producer.
pub const INSTANCE_CAPACITY: usize = 1;

/// Logical grid extent carried beside the fixed-capacity page image.
///
/// The backing buffer and row stride remain [`COLUMNS`] by [`ROWS`]; this
/// extent selects the visible top-left rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridSize {
    columns: u16,
    rows: u16,
}

impl GridSize {
    pub const FULL: Self = Self {
        columns: COLUMNS as u16,
        rows: ROWS as u16,
    };

    pub const fn new(columns: usize, rows: usize) -> Result<Self, GridSizeError> {
        if columns == 0 || columns > COLUMNS || rows == 0 || rows > ROWS {
            Err(GridSizeError { columns, rows })
        } else {
            Ok(Self {
                columns: columns as u16,
                rows: rows as u16,
            })
        }
    }

    pub const fn columns(self) -> usize {
        self.columns as usize
    }

    pub const fn rows(self) -> usize {
        self.rows as usize
    }
}

impl Default for GridSize {
    fn default() -> Self {
        Self::FULL
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridSizeError {
    pub columns: usize,
    pub rows: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceId(u32);

impl InstanceId {
    pub const PRIMARY: Self = Self(0);

    pub const fn from_index(index: usize) -> Option<Self> {
        if index < INSTANCE_CAPACITY {
            Some(Self(index as u32))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    const fn raw(self) -> u32 {
        self.0
    }
}

const ANIMATION_WIRE_VERSION: u8 = 1;
const FONT_INSTANCE_WIRE_VERSION: u8 = 2;
const ANIMATION_WIRE_HEADER_BYTES: usize = 4;
const ANIMATION_RECORD_HEADER_BYTES: usize = 12;
const FONT_INSTANCE_RECORD_HEADER_BYTES: usize = 40;
const ANIMATION_KEYFRAME_BYTES: usize = 8;
pub const MAX_ANIMATION_WIRE_BYTES: usize = ANIMATION_WIRE_HEADER_BYTES
    + TEXT_COLOR_ANIMATION_SLOTS
        * (ANIMATION_RECORD_HEADER_BYTES + COLOR_KEYFRAME_CAPACITY * ANIMATION_KEYFRAME_BYTES);
pub const MAX_FONT_INSTANCE_WIRE_BYTES: usize = ANIMATION_WIRE_HEADER_BYTES
    + TEXT_COLOR_ANIMATION_SLOTS
        * (FONT_INSTANCE_RECORD_HEADER_BYTES + COLOR_KEYFRAME_CAPACITY * ANIMATION_KEYFRAME_BYTES);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrintRequest {
    token: u32,
}

impl PrintRequest {
    pub const fn token(self) -> u32 {
        self.token
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Selects the RGBA components changed by an animation. Components outside
/// the mask retain their value from the first keyframe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorChannels(u8);

impl ColorChannels {
    pub const RED: Self = Self(1 << 0);
    pub const GREEN: Self = Self(1 << 1);
    pub const BLUE: Self = Self(1 << 2);
    pub const ALPHA: Self = Self(1 << 3);
    pub const RGB: Self = Self(Self::RED.0 | Self::GREEN.0 | Self::BLUE.0);
    pub const RGBA: Self = Self(Self::RGB.0 | Self::ALPHA.0);

    pub const fn from_bits(bits: u8) -> Result<Self, AnimationDefinitionError> {
        if bits != 0 && bits & !Self::RGBA.0 == 0 {
            Ok(Self(bits))
        } else {
            Err(AnimationDefinitionError::InvalidChannels(bits))
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum AnimationTiming {
    #[default]
    Linear = 0,
    EaseInOutSine = 1,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum AnimationIteration {
    Once = 0,
    #[default]
    Loop = 1,
    Alternate = 2,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ColorKeyframe {
    offset_permille: u16,
    rgba: Rgba8,
}

impl ColorKeyframe {
    pub const fn new(offset_permille: u16, rgba: Rgba8) -> Self {
        Self {
            offset_permille,
            rgba,
        }
    }

    pub const fn offset_permille(self) -> u16 {
        self.offset_permille
    }

    pub const fn rgba(self) -> Rgba8 {
        self.rgba
    }
}

/// Fixed-storage CSS-like color animation suitable for a static Blueprint.
/// Offsets use 0..=1000 so definitions do not require floating-point ABI data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorAnimation {
    keyframes: [ColorKeyframe; COLOR_KEYFRAME_CAPACITY],
    keyframe_count: u8,
    channels: ColorChannels,
    duration_ms: u32,
    timing: AnimationTiming,
    iteration: AnimationIteration,
}

impl ColorAnimation {
    pub fn keyframes(
        keyframes: &[ColorKeyframe],
        channels: ColorChannels,
        duration_ms: u32,
        timing: AnimationTiming,
        iteration: AnimationIteration,
    ) -> Result<Self, AnimationDefinitionError> {
        if !(2..=COLOR_KEYFRAME_CAPACITY).contains(&keyframes.len()) {
            return Err(AnimationDefinitionError::KeyframeCount(keyframes.len()));
        }
        if !(MIN_ANIMATION_DURATION_MS..=MAX_ANIMATION_DURATION_MS).contains(&duration_ms) {
            return Err(AnimationDefinitionError::Duration(duration_ms));
        }
        ColorChannels::from_bits(channels.bits())?;
        if keyframes[0].offset_permille != 0 {
            return Err(AnimationDefinitionError::FirstOffset(
                keyframes[0].offset_permille,
            ));
        }
        if keyframes[keyframes.len() - 1].offset_permille != 1_000 {
            return Err(AnimationDefinitionError::LastOffset(
                keyframes[keyframes.len() - 1].offset_permille,
            ));
        }
        for pair in keyframes.windows(2) {
            if pair[1].offset_permille <= pair[0].offset_permille {
                return Err(AnimationDefinitionError::OffsetsNotIncreasing);
            }
        }
        let mut stored = [ColorKeyframe::new(0, Rgba8::TRANSPARENT); COLOR_KEYFRAME_CAPACITY];
        stored[..keyframes.len()].copy_from_slice(keyframes);
        Ok(Self {
            keyframes: stored,
            keyframe_count: keyframes.len() as u8,
            channels,
            duration_ms,
            timing,
            iteration,
        })
    }

    pub fn transition(
        from: Rgba8,
        to: Rgba8,
        channels: ColorChannels,
        duration_ms: u32,
        timing: AnimationTiming,
        iteration: AnimationIteration,
    ) -> Result<Self, AnimationDefinitionError> {
        Self::keyframes(
            &[ColorKeyframe::new(0, from), ColorKeyframe::new(1_000, to)],
            channels,
            duration_ms,
            timing,
            iteration,
        )
    }

    pub const fn keyframe_count(self) -> usize {
        self.keyframe_count as usize
    }

    pub fn keyframes_slice(&self) -> &[ColorKeyframe] {
        &self.keyframes[..self.keyframe_count()]
    }

    pub const fn channels(self) -> ColorChannels {
        self.channels
    }

    pub const fn duration_ms(self) -> u32 {
        self.duration_ms
    }

    pub const fn timing(self) -> AnimationTiming {
        self.timing
    }

    pub const fn iteration(self) -> AnimationIteration {
        self.iteration
    }
}

/// Static presentation properties consumed by the persistent GPU font engine.
/// Rotation is expressed in 1/100 degree and scale/opacity in 1/1000 units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontStyle {
    rotation_centidegrees: i16,
    scale_permille: u16,
    opacity_permille: u16,
    background: Rgba8,
}

impl FontStyle {
    pub const IDENTITY: Self = Self {
        rotation_centidegrees: 0,
        scale_permille: 1_000,
        opacity_permille: 1_000,
        background: Rgba8::TRANSPARENT,
    };

    pub const fn new(
        rotation_centidegrees: i16,
        scale_permille: u16,
        opacity_permille: u16,
        background: Rgba8,
    ) -> Result<Self, AnimationDefinitionError> {
        if rotation_centidegrees < -18_000 || rotation_centidegrees > 18_000 {
            return Err(AnimationDefinitionError::Rotation(rotation_centidegrees));
        }
        if scale_permille < 125 || scale_permille > 8_000 {
            return Err(AnimationDefinitionError::Scale(scale_permille));
        }
        if opacity_permille > 1_000 {
            return Err(AnimationDefinitionError::Opacity(opacity_permille));
        }
        Ok(Self {
            rotation_centidegrees,
            scale_permille,
            opacity_permille,
            background,
        })
    }

    pub const fn rotation_centidegrees(self) -> i16 {
        self.rotation_centidegrees
    }

    pub const fn scale_permille(self) -> u16 {
        self.scale_permille
    }

    pub const fn opacity_permille(self) -> u16 {
        self.opacity_permille
    }

    pub const fn background(self) -> Rgba8 {
        self.background
    }
}

impl Default for FontStyle {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Bounded predefined sine/cosine animation evaluated by the C++ GPU kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrigAnimation {
    period_ms: u32,
    phase_permille: u16,
    rotation_amplitude_centidegrees: i16,
    scale_amplitude_permille: i16,
    opacity_amplitude_permille: i16,
    translation_x_tenths_px: i16,
    translation_y_tenths_px: i16,
}

impl TrigAnimation {
    pub const NONE: Self = Self {
        period_ms: 0,
        phase_permille: 0,
        rotation_amplitude_centidegrees: 0,
        scale_amplitude_permille: 0,
        opacity_amplitude_permille: 0,
        translation_x_tenths_px: 0,
        translation_y_tenths_px: 0,
    };

    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        period_ms: u32,
        phase_permille: u16,
        rotation_amplitude_centidegrees: i16,
        scale_amplitude_permille: i16,
        opacity_amplitude_permille: i16,
        translation_x_tenths_px: i16,
        translation_y_tenths_px: i16,
    ) -> Result<Self, AnimationDefinitionError> {
        if period_ms < MIN_ANIMATION_DURATION_MS || period_ms > MAX_ANIMATION_DURATION_MS {
            return Err(AnimationDefinitionError::MotionPeriod(period_ms));
        }
        if phase_permille > 1_000 {
            return Err(AnimationDefinitionError::MotionPhase(phase_permille));
        }
        if rotation_amplitude_centidegrees < -18_000 || rotation_amplitude_centidegrees > 18_000 {
            return Err(AnimationDefinitionError::MotionRotation(
                rotation_amplitude_centidegrees,
            ));
        }
        if scale_amplitude_permille < -875 || scale_amplitude_permille > 4_000 {
            return Err(AnimationDefinitionError::MotionScale(
                scale_amplitude_permille,
            ));
        }
        if opacity_amplitude_permille < -1_000 || opacity_amplitude_permille > 1_000 {
            return Err(AnimationDefinitionError::MotionOpacity(
                opacity_amplitude_permille,
            ));
        }
        Ok(Self {
            period_ms,
            phase_permille,
            rotation_amplitude_centidegrees,
            scale_amplitude_permille,
            opacity_amplitude_permille,
            translation_x_tenths_px,
            translation_y_tenths_px,
        })
    }

    pub const fn period_ms(self) -> u32 {
        self.period_ms
    }

    pub const fn phase_permille(self) -> u16 {
        self.phase_permille
    }

    pub const fn rotation_amplitude_centidegrees(self) -> i16 {
        self.rotation_amplitude_centidegrees
    }

    pub const fn scale_amplitude_permille(self) -> i16 {
        self.scale_amplitude_permille
    }

    pub const fn opacity_amplitude_permille(self) -> i16 {
        self.opacity_amplitude_permille
    }

    pub const fn translation_x_tenths_px(self) -> i16 {
        self.translation_x_tenths_px
    }

    pub const fn translation_y_tenths_px(self) -> i16 {
        self.translation_y_tenths_px
    }
}

impl Default for TrigAnimation {
    fn default() -> Self {
        Self::NONE
    }
}

/// One selector-scoped GPU font program. Any member may remain at identity,
/// allowing color-only, transform-only, or combined presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontInstanceProgram {
    color: Option<ColorAnimation>,
    style: FontStyle,
    motion: TrigAnimation,
}

impl FontInstanceProgram {
    pub const fn new(
        color: Option<ColorAnimation>,
        style: FontStyle,
        motion: TrigAnimation,
    ) -> Self {
        Self {
            color,
            style,
            motion,
        }
    }

    pub const fn color_only(color: ColorAnimation) -> Self {
        Self::new(Some(color), FontStyle::IDENTITY, TrigAnimation::NONE)
    }

    pub const fn color(self) -> Option<ColorAnimation> {
        self.color
    }

    pub const fn style(self) -> FontStyle {
        self.style
    }

    pub const fn motion(self) -> TrigAnimation {
        self.motion
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationDefinitionError {
    KeyframeCount(usize),
    Duration(u32),
    InvalidChannels(u8),
    FirstOffset(u16),
    LastOffset(u16),
    OffsetsNotIncreasing,
    Rotation(i16),
    Scale(u16),
    Opacity(u16),
    MotionPeriod(u32),
    MotionPhase(u16),
    MotionRotation(i16),
    MotionScale(i16),
    MotionOpacity(i16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidSnapshot,
    InvalidScale,
    NotOwner,
    Transport,
    InvalidAnimation,
    InvalidInstance,
    PoolFull,
    InvalidGridSize,
    Unknown(i32),
}

/// Copy one immutable GridPaper generation into the kernel-owned back buffer.
///
/// Returning means the kernel has accepted its own stable copy. Rendering is
/// asynchronous and never runs in the Blueprint's call path.
pub fn submit_snapshot(
    generation: u64,
    scale_percent: u16,
    raw: &[u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_submit(
            generation,
            u32::from(scale_percent),
            raw.as_ptr(),
            raw.len(),
        )
    })
}

pub fn submit_instance_snapshot(
    instance: InstanceId,
    generation: u64,
    scale_percent: u16,
    raw: &[u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_submit_instance(
            instance.raw(),
            generation,
            u32::from(scale_percent),
            raw.as_ptr(),
            raw.len(),
        )
    })
}

/// Copy the latest logical page and atomically release its kernel projection.
///
/// UI4 cell edits are mirrored into this image. Replicatable producers should
/// call this at PreparePause, copy the result into their checkpointed state,
/// and then report Ready. The UI4 frame and GPU scene are disposable after this
/// call and must be recreated by submitting state after Resume.
pub fn checkpoint_snapshot(out: &mut [u8; PAGE_BYTES]) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_checkpoint(out.as_mut_ptr(), out.len())
    })
}

pub fn checkpoint_instance_snapshot(
    instance: InstanceId,
    out: &mut [u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_checkpoint_instance(
            instance.raw(),
            out.as_mut_ptr(),
            out.len(),
        )
    })
}

/// Submit a fixed-capacity page with a positive logical extent no larger than
/// the current column and row soft caps.
pub fn submit_sized_snapshot(
    size: GridSize,
    generation: u64,
    scale_percent: u16,
    raw: &[u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_submit_sized(
            generation,
            u32::from(scale_percent),
            size.columns() as u32,
            size.rows() as u32,
            raw.as_ptr(),
            raw.len(),
        )
    })
}

pub fn submit_instance_sized_snapshot(
    instance: InstanceId,
    size: GridSize,
    generation: u64,
    scale_percent: u16,
    raw: &[u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_submit_instance_sized(
            instance.raw(),
            generation,
            u32::from(scale_percent),
            size.columns() as u32,
            size.rows() as u32,
            raw.as_ptr(),
            raw.len(),
        )
    })
}

/// Atomically replace all text color animations. Each table index is the
/// foreground palette value it selects; index 17 (transparent) is deliberately
/// absent. Geometry and the fixed page snapshot are not resubmitted.
pub fn submit_text_animations(
    animations: &[Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS],
) -> Result<(), Error> {
    let mut wire = [0u8; MAX_ANIMATION_WIRE_BYTES];
    let wire_len = encode_text_animations(animations, &mut wire);
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_text_animations_submit(wire.as_ptr(), wire_len)
    })
}

pub fn submit_instance_text_animations(
    instance: InstanceId,
    animations: &[Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS],
) -> Result<(), Error> {
    let mut wire = [0u8; MAX_ANIMATION_WIRE_BYTES];
    let wire_len = encode_text_animations(animations, &mut wire);
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_text_animations_submit_instance(
            instance.raw(),
            wire.as_ptr(),
            wire_len,
        )
    })
}

fn encode_text_animations(
    animations: &[Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS],
    wire: &mut [u8; MAX_ANIMATION_WIRE_BYTES],
) -> usize {
    wire.fill(0);
    wire[0] = ANIMATION_WIRE_VERSION;
    wire[1] = animations.iter().flatten().count() as u8;
    let mut cursor = ANIMATION_WIRE_HEADER_BYTES;
    for (selector, animation) in animations.iter().enumerate() {
        let Some(animation) = animation else {
            continue;
        };
        let header_end = cursor + ANIMATION_RECORD_HEADER_BYTES;
        wire[cursor] = selector as u8;
        wire[cursor + 1] = animation.channels.bits();
        wire[cursor + 2] = animation.timing as u8;
        wire[cursor + 3] = animation.iteration as u8;
        wire[cursor + 4..cursor + 8].copy_from_slice(&animation.duration_ms.to_le_bytes());
        wire[cursor + 8] = animation.keyframe_count;
        cursor = header_end;
        for keyframe in animation.keyframes_slice() {
            let frame_end = cursor + ANIMATION_KEYFRAME_BYTES;
            wire[cursor..cursor + 2].copy_from_slice(&keyframe.offset_permille.to_le_bytes());
            wire[cursor + 4..frame_end].copy_from_slice(&[
                keyframe.rgba.r,
                keyframe.rgba.g,
                keyframe.rgba.b,
                keyframe.rgba.a,
            ]);
            cursor = frame_end;
        }
    }
    cursor
}

/// Atomically replace the complete persistent font presentation table. The
/// page image and Skrifa-generated coverage stay resident and unchanged.
pub fn submit_font_instances(
    programs: &[Option<FontInstanceProgram>; TEXT_COLOR_ANIMATION_SLOTS],
) -> Result<(), Error> {
    let mut wire = [0u8; MAX_FONT_INSTANCE_WIRE_BYTES];
    let wire_len = encode_font_instances(programs, &mut wire);
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_text_animations_submit(wire.as_ptr(), wire_len)
    })
}

pub fn submit_instance_font_instances(
    instance: InstanceId,
    programs: &[Option<FontInstanceProgram>; TEXT_COLOR_ANIMATION_SLOTS],
) -> Result<(), Error> {
    let mut wire = [0u8; MAX_FONT_INSTANCE_WIRE_BYTES];
    let wire_len = encode_font_instances(programs, &mut wire);
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_text_animations_submit_instance(
            instance.raw(),
            wire.as_ptr(),
            wire_len,
        )
    })
}

fn encode_font_instances(
    programs: &[Option<FontInstanceProgram>; TEXT_COLOR_ANIMATION_SLOTS],
    wire: &mut [u8; MAX_FONT_INSTANCE_WIRE_BYTES],
) -> usize {
    wire.fill(0);
    wire[0] = FONT_INSTANCE_WIRE_VERSION;
    wire[1] = programs.iter().flatten().count() as u8;
    let mut cursor = ANIMATION_WIRE_HEADER_BYTES;
    for (selector, program) in programs.iter().enumerate() {
        let Some(program) = program else {
            continue;
        };
        let header_end = cursor + FONT_INSTANCE_RECORD_HEADER_BYTES;
        let style = program.style;
        let motion = program.motion;
        wire[cursor] = selector as u8;
        if let Some(animation) = program.color {
            wire[cursor + 1] = animation.channels.bits();
            wire[cursor + 2] = animation.timing as u8;
            wire[cursor + 3] = animation.iteration as u8;
            wire[cursor + 4..cursor + 8].copy_from_slice(&animation.duration_ms.to_le_bytes());
            wire[cursor + 8] = animation.keyframe_count;
        }
        wire[cursor + 12..cursor + 14].copy_from_slice(&style.rotation_centidegrees.to_le_bytes());
        wire[cursor + 14..cursor + 16].copy_from_slice(&style.scale_permille.to_le_bytes());
        wire[cursor + 16..cursor + 18].copy_from_slice(&style.opacity_permille.to_le_bytes());
        wire[cursor + 18..cursor + 22].copy_from_slice(&[
            style.background.r,
            style.background.g,
            style.background.b,
            style.background.a,
        ]);
        wire[cursor + 22..cursor + 26].copy_from_slice(&motion.period_ms.to_le_bytes());
        wire[cursor + 26..cursor + 28].copy_from_slice(&motion.phase_permille.to_le_bytes());
        wire[cursor + 28..cursor + 30]
            .copy_from_slice(&motion.rotation_amplitude_centidegrees.to_le_bytes());
        wire[cursor + 30..cursor + 32]
            .copy_from_slice(&motion.scale_amplitude_permille.to_le_bytes());
        wire[cursor + 32..cursor + 34]
            .copy_from_slice(&motion.opacity_amplitude_permille.to_le_bytes());
        wire[cursor + 34..cursor + 36]
            .copy_from_slice(&motion.translation_x_tenths_px.to_le_bytes());
        wire[cursor + 36..cursor + 38]
            .copy_from_slice(&motion.translation_y_tenths_px.to_le_bytes());
        cursor = header_end;
        if let Some(animation) = program.color {
            for keyframe in animation.keyframes_slice() {
                let frame_end = cursor + ANIMATION_KEYFRAME_BYTES;
                wire[cursor..cursor + 2].copy_from_slice(&keyframe.offset_permille.to_le_bytes());
                wire[cursor + 4..frame_end].copy_from_slice(&[
                    keyframe.rgba.r,
                    keyframe.rgba.g,
                    keyframe.rgba.b,
                    keyframe.rgba.a,
                ]);
                cursor = frame_end;
            }
        }
    }
    cursor
}

/// Detach this Blueprint producer and return its kernel GridPaper pool lease.
///
/// This invalidates the service-owned UI4 presentation, frame, and GPU scene.
/// The producer keeps logical page data in its own memory and must submit it
/// again after Resume.
pub fn close() -> Result<(), Error> {
    status(unsafe { v::bp_abi::trueos_cabi_gridpaper_close() })
}

pub fn close_instance(instance: InstanceId) -> Result<(), Error> {
    status(unsafe { v::bp_abi::trueos_cabi_gridpaper_close_instance(instance.raw()) })
}

/// Take one focused-GridPaper Print Screen request, if present.
pub fn take_print_request() -> Option<PrintRequest> {
    let token = unsafe { v::bp_abi::trueos_cabi_gridpaper_print_request_take() };
    (token != 0 && token <= u32::MAX as u64).then_some(PrintRequest {
        token: token as u32,
    })
}

pub fn take_instance_print_request(instance: InstanceId) -> Option<PrintRequest> {
    let token =
        unsafe { v::bp_abi::trueos_cabi_gridpaper_print_request_take_instance(instance.raw()) };
    (token != 0 && token <= u32::MAX as u64).then_some(PrintRequest {
        token: token as u32,
    })
}

fn status(code: i32) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        -1 => Err(Error::InvalidSnapshot),
        -2 => Err(Error::InvalidScale),
        -3 => Err(Error::NotOwner),
        -4 => Err(Error::Transport),
        -5 => Err(Error::InvalidAnimation),
        -6 => Err(Error::InvalidInstance),
        -7 => Err(Error::PoolFull),
        -8 => Err(Error::InvalidGridSize),
        other => Err(Error::Unknown(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_positive_extent_through_the_soft_caps_is_valid() {
        for columns in 1..=COLUMNS {
            for rows in 1..=ROWS {
                assert_eq!(
                    GridSize::new(columns, rows),
                    Ok(GridSize {
                        columns: columns as u16,
                        rows: rows as u16,
                    })
                );
            }
        }
        for (columns, rows) in [(0, 1), (1, 0), (COLUMNS + 1, 1), (1, ROWS + 1)] {
            assert!(GridSize::new(columns, rows).is_err());
        }
    }
}
