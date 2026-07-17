//! Dedicated producer side of the GridPaper kernel snapshot transport.
//!
//! This intentionally exposes no generic UI or drawing calls. The Blueprint
//! publishes its fixed wire image; the kernel service owns presentation and
//! GPU residency.

pub const COLUMNS: usize = 37;
pub const ROWS: usize = 53;
pub const CELL_BYTES: usize = 13;
pub const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;
pub const TEXT_COLOR_ANIMATION_SLOTS: usize = 17;
pub const COLOR_KEYFRAME_CAPACITY: usize = 8;
pub const MIN_ANIMATION_DURATION_MS: u32 = 16;
pub const MAX_ANIMATION_DURATION_MS: u32 = 600_000;
pub const INSTANCE_CAPACITY: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceId(u32);

impl InstanceId {
    pub const PRIMARY: Self = Self(0);
    pub const NATIVE: Self = Self(1);

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
const ANIMATION_WIRE_HEADER_BYTES: usize = 4;
const ANIMATION_RECORD_HEADER_BYTES: usize = 12;
const ANIMATION_KEYFRAME_BYTES: usize = 8;
pub const MAX_ANIMATION_WIRE_BYTES: usize = ANIMATION_WIRE_HEADER_BYTES
    + TEXT_COLOR_ANIMATION_SLOTS
        * (ANIMATION_RECORD_HEADER_BYTES + COLOR_KEYFRAME_CAPACITY * ANIMATION_KEYFRAME_BYTES);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationDefinitionError {
    KeyframeCount(usize),
    Duration(u32),
    InvalidChannels(u8),
    FirstOffset(u16),
    LastOffset(u16),
    OffsetsNotIncreasing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidSnapshot,
    InvalidScale,
    NotOwner,
    Transport,
    InvalidAnimation,
    InvalidInstance,
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

/// Detach this Blueprint producer. The kernel retains the scene, while its
/// UI4 presentation is released until a running producer attaches again.
pub fn close() -> Result<(), Error> {
    status(unsafe { v::bp_abi::trueos_cabi_gridpaper_close() })
}

pub fn close_instance(instance: InstanceId) -> Result<(), Error> {
    status(unsafe { v::bp_abi::trueos_cabi_gridpaper_close_instance(instance.raw()) })
}

/// Take one focused-GridPaper F10 request, if present.
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
        other => Err(Error::Unknown(other)),
    }
}
