//! Shared text and character-set conversions used by the WC3 personality.

pub(crate) fn decode_cp1252(byte: u8) -> u16 {
    match byte {
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x8e => 0x017d,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9e => 0x017e,
        0x9f => 0x0178,
        _ => byte as u16,
    }
}

pub(crate) fn encode_cp1252(value: u16) -> Option<u8> {
    match value {
        0x20ac => Some(0x80),
        0x201a => Some(0x82),
        0x0192 => Some(0x83),
        0x201e => Some(0x84),
        0x2026 => Some(0x85),
        0x2020 => Some(0x86),
        0x2021 => Some(0x87),
        0x02c6 => Some(0x88),
        0x2030 => Some(0x89),
        0x0160 => Some(0x8a),
        0x2039 => Some(0x8b),
        0x0152 => Some(0x8c),
        0x017d => Some(0x8e),
        0x2018 => Some(0x91),
        0x2019 => Some(0x92),
        0x201c => Some(0x93),
        0x201d => Some(0x94),
        0x2022 => Some(0x95),
        0x2013 => Some(0x96),
        0x2014 => Some(0x97),
        0x02dc => Some(0x98),
        0x2122 => Some(0x99),
        0x0161 => Some(0x9a),
        0x203a => Some(0x9b),
        0x0153 => Some(0x9c),
        0x017e => Some(0x9e),
        0x0178 => Some(0x9f),
        0x0000..=0x00ff => Some(value as u8),
        _ => None,
    }
}

pub fn cp1252_to_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| char::from_u32(decode_cp1252(*byte) as u32).unwrap_or('\u{fffd}'))
        .collect()
}

pub(crate) fn decode_span(bytes: &[u8], utf16le: bool) -> Result<String, &'static str> {
    if !utf16le {
        return String::from_utf8(bytes.to_vec()).map_err(|_| "registry UTF-8");
    }
    std::char::decode_utf16(
        bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
    )
    .map(|unit| unit.map_err(|_| "registry UTF-16"))
    .collect()
}

/// Run MSVCRT's x86 `_ftol` conversion against an FXSAVE-compatible x87 image.
///
/// `_ftol` takes no C arguments.  It truncates `ST(0)` toward zero, returns the
/// resulting signed 64-bit integer, and consumes that x87 stack value.  The
/// helper deliberately does not inherit the current x87 rounding mode: this is
/// the CRT helper's ANSI-C conversion contract.
pub fn x87_ftol(state: &mut [u8]) -> i64 {
    const FCW: usize = 0;
    const FSW: usize = 2;
    const FTW: usize = 4;
    const ST_SPACE: usize = 32;
    const ST_BYTES: usize = 16;
    const X87_INVALID: u16 = 1 << 0;
    const X87_STACK_FAULT: u16 = 1 << 6;

    let mut status = u16::from_le_bytes(state[FSW..FSW + 2].try_into().unwrap());
    let top = ((status >> 11) & 7) as usize;
    let mut tags = state[FTW];

    // An empty register is an x87 stack underflow.  The default masked x87
    // behavior stores the integer-indefinite value and advances TOP.
    let empty = tags & (1 << top) == 0;
    let value = if empty {
        status |= X87_INVALID | X87_STACK_FAULT;
        i64::MIN
    } else {
        // FXSAVE stores the data fields in logical ST(0)..ST(7) order, while
        // its abridged tag word is in physical R0..R7 order. `top` therefore
        // selects the FTW bit above, but ST(0)'s value is always the first
        // data field.
        let offset = ST_SPACE;
        let significand = u64::from_le_bytes(state[offset..offset + 8].try_into().unwrap());
        let exponent_word = u16::from_le_bytes(state[offset + 8..offset + 10].try_into().unwrap());
        let negative = exponent_word & 0x8000 != 0;
        let exponent = exponent_word & 0x7fff;

        // FISTP qword stores the x87 integer-indefinite value for NaNs,
        // infinities, malformed encodings, and values beyond i64.
        let magnitude = if exponent == 0 {
            Some(0u128)
        } else if exponent == 0x7fff || significand & (1 << 63) == 0 {
            None
        } else {
            let unbiased = i32::from(exponent) - 16_383;
            if unbiased < 0 {
                Some(0)
            } else if unbiased <= 63 {
                Some(u128::from(significand >> (63 - unbiased)))
            } else {
                // Every finite value with an unbiased exponent above 63 is
                // outside the signed 64-bit range, so avoid a wide shift.
                None
            }
        };

        match magnitude {
            Some(magnitude) if (!negative && magnitude <= i64::MAX as u128) => magnitude as i64,
            Some(magnitude) if negative && magnitude <= (1u128 << 63) => {
                if magnitude == 1u128 << 63 {
                    i64::MIN
                } else {
                    -(magnitude as i64)
                }
            }
            _ => {
                status |= X87_INVALID;
                i64::MIN
            }
        }
    };

    tags &= !(1 << top);
    // After popping ST(0), the next logical value becomes ST(0). The FXSAVE
    // data area is in logical stack order, so shift its remaining entries up
    // one slot before advancing TOP. The final field is now empty/undefined.
    state.copy_within(ST_SPACE + ST_BYTES..ST_SPACE + 8 * ST_BYTES, ST_SPACE);
    state[ST_SPACE + 7 * ST_BYTES..ST_SPACE + 8 * ST_BYTES].fill(0);
    state[FTW] = tags;
    status = (status & !(7 << 11)) | (((top as u16 + 1) & 7) << 11);
    state[FSW..FSW + 2].copy_from_slice(&status.to_le_bytes());
    let _ = FCW; // Documents the FXSAVE layout: `_ftol` ignores FCW rounding.
    value
}

#[cfg(test)]
mod tests {
    use super::x87_ftol;

    fn state_with_st0(significand: u64, exponent_word: u16) -> [u8; 832] {
        let mut state = [0u8; 832];
        state[4] = 1; // abridged tag word: physical ST(0) is nonempty
        state[32..40].copy_from_slice(&significand.to_le_bytes());
        state[40..42].copy_from_slice(&exponent_word.to_le_bytes());
        state
    }

    #[test]
    fn ftol_truncates_toward_zero_and_pops_st0() {
        // 1.5 and -1.5 in x87's explicit-integer-bit extended format.
        let mut positive = state_with_st0(0xc000_0000_0000_0000, 0x3fff);
        assert_eq!(x87_ftol(&mut positive), 1);
        assert_eq!(positive[4] & 1, 0);
        assert_eq!(
            (u16::from_le_bytes(positive[2..4].try_into().unwrap()) >> 11) & 7,
            1
        );

        let mut negative = state_with_st0(0xc000_0000_0000_0000, 0xbfff);
        assert_eq!(x87_ftol(&mut negative), -1);
        assert_eq!(negative[4] & 1, 0);
    }

    #[test]
    fn ftol_returns_integer_indefinite_for_nan() {
        let mut state = state_with_st0(0xc000_0000_0000_0000, 0x7fff);
        assert_eq!(x87_ftol(&mut state), i64::MIN);
        assert_ne!(u16::from_le_bytes(state[2..4].try_into().unwrap()) & 1, 0);
    }

    #[test]
    fn ftol_returns_integer_indefinite_when_left_shift_would_overflow() {
        // 2^128 would wrap to zero if the extended significand were shifted
        // into u128 without checking the result's width.
        let mut state = state_with_st0(0x8000_0000_0000_0000, 0x407f);
        assert_eq!(x87_ftol(&mut state), i64::MIN);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn ftol_nonzero_top_matches_native_conversion_and_pop() {
        use std::arch::asm;

        #[repr(align(16))]
        struct FxState([u8; 832]);

        struct Restore(FxState);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    asm!("fxrstor [{}]", in(reg) self.0.0.as_ptr(), options(nostack, preserves_flags));
                }
            }
        }

        let mut original = FxState([0; 832]);
        let mut image = FxState([0; 832]);
        let mut native_first = 0i64;
        let mut native_second = 0i64;
        let mut native_third = 0i64;
        let mut restored_second = 0i64;
        let mut restored_third = 0i64;
        let values = [-31.75f64, 22.75f64, 1.75f64];
        let truncate = 0x0f7fu16;
        let _restore = unsafe {
            asm!("fxsave [{}]", in(reg) original.0.as_mut_ptr(), options(nostack, preserves_flags));
            Restore(original)
        };

        unsafe {
            asm!(
                "fninit",
                "fldcw word ptr [{truncate}]",
                "fld qword ptr [{v0}]",
                "fld qword ptr [{v1}]",
                "fld qword ptr [{v2}]",
                "fxsave [{image}]",
                "fistp qword ptr [{first}]",
                "fistp qword ptr [{second}]",
                "fistp qword ptr [{third}]",
                truncate = in(reg) &truncate,
                v0 = in(reg) &values[0],
                v1 = in(reg) &values[1],
                v2 = in(reg) &values[2],
                image = in(reg) image.0.as_mut_ptr(),
                first = in(reg) &mut native_first,
                second = in(reg) &mut native_second,
                third = in(reg) &mut native_third,
                options(nostack),
            );
        }

        // The three pushes leave TOP=5, so this exercises FTW physical-index
        // lookup and the logical ST(0) data slot at the same time.
        assert_eq!(
            (u16::from_le_bytes(image.0[2..4].try_into().unwrap()) >> 11) & 7,
            5
        );
        assert_eq!(native_first, 1);
        assert_eq!(native_second, 22);
        assert_eq!(native_third, -31);
        assert_eq!(x87_ftol(&mut image.0), native_first);

        unsafe {
            asm!(
                "fxrstor [{image}]",
                "fistp qword ptr [{second}]",
                "fistp qword ptr [{third}]",
                image = in(reg) image.0.as_ptr(),
                second = in(reg) &mut restored_second,
                third = in(reg) &mut restored_third,
                options(nostack),
            );
        }
        assert_eq!(restored_second, native_second);
        assert_eq!(restored_third, native_third);
    }
}
