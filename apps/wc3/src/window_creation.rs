//! Win32 x86 window-creation wire layout and synchronous callback progression.
//! No application state is inferred from lpCreateParams: only guest code may
//! attach it to a window with SetWindowLongA.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    NcCreate,
    NcCalcSize,
    Create,
    Destroy,
    NcDestroy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Advance {
    Call(Phase),
    Complete(bool),
}

impl Phase {
    pub fn message(self) -> u32 {
        match self {
            Self::NcCreate => 0x81,
            Self::NcCalcSize => 0x83,
            Self::Create => 1,
            Self::Destroy => 2,
            Self::NcDestroy => 0x82,
        }
    }

    pub fn advance(self, result: u32) -> Advance {
        match self {
            Self::NcCreate if result == 0 => Advance::Call(Self::NcDestroy),
            Self::NcCreate => Advance::Call(Self::NcCalcSize),
            Self::NcCalcSize => Advance::Call(Self::Create),
            Self::Create if result == u32::MAX => Advance::Call(Self::Destroy),
            Self::Create => Advance::Complete(true),
            Self::Destroy => Advance::Call(Self::NcDestroy),
            Self::NcDestroy => Advance::Complete(false),
        }
    }

    pub fn lparam(self, scratch: u32) -> u32 {
        match self {
            Self::NcCreate | Self::Create => scratch,
            Self::NcCalcSize => scratch + 48,
            Self::Destroy | Self::NcDestroy => 0,
        }
    }
}

/// Default handling for creation, paint, and the scalar size notification.
/// Other messages remain an explicit frontier until their semantics are added.
pub fn default_proc_result(message: u32) -> Option<u32> {
    match message {
        0x81 => Some(1),
        1 | 2 | 5 | 0x82 | 0x83 | 0xf => Some(0),
        _ => None,
    }
}

/// CREATESTRUCTA (48 bytes) followed by the initial WM_NCCALCSIZE RECT.
/// `args` includes the original CreateWindowExA return address. Preserve its
/// guest class/name pointers and signed geometry bit patterns without copying
/// strings or using host-sized pointers.
pub fn payload(args: &[u32; 13]) -> Result<[u8; 64], &'static str> {
    let right = (args[5] as i32)
        .checked_add(args[7] as i32)
        .ok_or("creation RECT right overflow")?;
    let bottom = (args[6] as i32)
        .checked_add(args[8] as i32)
        .ok_or("creation RECT bottom overflow")?;
    let words = [
        args[12],
        args[11],
        args[10],
        args[9],
        args[8],
        args[7],
        args[6],
        args[5],
        args[4],
        args[3],
        args[2],
        args[1],
        args[5],
        args[6],
        right as u32,
        bottom as u32,
    ];
    let mut bytes = [0; 64];
    for (dst, word) in bytes.chunks_exact_mut(4).zip(words) {
        dst.copy_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

pub fn callback_frame(return_address: u32, hwnd: u32, phase: Phase, scratch: u32) -> [u8; 20] {
    let mut bytes = [0; 20];
    for (dst, word) in bytes.chunks_exact_mut(4).zip([
        return_address,
        hwnd,
        phase.message(),
        0,
        phase.lparam(scratch),
    ]) {
        dst.copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_uses_x86_layout_and_original_guest_pointers() {
        let bytes = payload(&[
            0xabcdef,
            0x80,
            0x102000,
            0x103000,
            0x80000000,
            (-20i32) as u32,
            30,
            640,
            480,
            0x55,
            0x66,
            0x400000,
            0x64900c8,
        ])
        .unwrap();
        let words: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(
            words,
            [
                0x64900c8,
                0x400000,
                0x66,
                0x55,
                480,
                640,
                30,
                (-20i32) as u32,
                0x80000000,
                0x103000,
                0x102000,
                0x80,
                (-20i32) as u32,
                30,
                620,
                510
            ]
        );
    }

    #[test]
    fn creation_success_and_rejection_follow_guest_results() {
        assert_eq!(Phase::NcCreate.advance(1), Advance::Call(Phase::NcCalcSize));
        assert_eq!(Phase::NcCalcSize.advance(0), Advance::Call(Phase::Create));
        assert_eq!(Phase::Create.advance(0), Advance::Complete(true));
        assert_eq!(Phase::NcCreate.advance(0), Advance::Call(Phase::NcDestroy));
        assert_eq!(
            Phase::Create.advance(u32::MAX),
            Advance::Call(Phase::Destroy)
        );
        assert_eq!(Phase::Destroy.advance(0), Advance::Call(Phase::NcDestroy));
        assert_eq!(Phase::NcDestroy.advance(99), Advance::Complete(false));
        assert_eq!(default_proc_result(0x81), Some(1));
        assert_eq!(default_proc_result(5), Some(0));
        assert_eq!(default_proc_result(0x1234), None);
    }

    #[test]
    fn callback_arguments_leave_scratch_above_stdcall_frame() {
        for phase in [Phase::NcCreate, Phase::NcCalcSize, Phase::Create] {
            let frame = callback_frame(0x2f0040, 0x57434003, phase, 0x43ffb00);
            let words: Vec<_> = frame
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(
                words,
                [
                    0x2f0040,
                    0x57434003,
                    phase.message(),
                    0,
                    phase.lparam(0x43ffb00)
                ]
            );
        }
    }
}
