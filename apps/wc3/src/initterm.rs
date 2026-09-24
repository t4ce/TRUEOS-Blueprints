//! Exact, deliberately small semantic replacements for leaf CRT initializers.
//! Planning is read-only; the caller validates mappings and commits one dword.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Code,
    Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub store: Option<(u32, u32)>,
    pub eax: Option<u32>,
}

/// Accept only RET, A1/A3/RET, or C7 05/RET, behind at most four E9 jumps.
/// `read` must enforce executable code and readable data ranges. No instruction
/// prefixes, arbitrary branches, calls, floating point, or cached output values.
pub fn plan(mut pc: u32, mut read: impl FnMut(u32, &mut [u8], Access) -> bool) -> Option<Effect> {
    for jumps in 0..=4 {
        let mut opcode = [0];
        if !read(pc, &mut opcode, Access::Code) {
            return None;
        }
        match opcode[0] {
            0xe9 if jumps < 4 => {
                let mut code = [0; 5];
                if !read(pc, &mut code, Access::Code) || code[0] != 0xe9 {
                    return None;
                }
                let displacement = i32::from_le_bytes(code[1..5].try_into().ok()?);
                pc = u32::try_from(i64::from(pc) + 5 + i64::from(displacement)).ok()?;
            }
            0xc3 => {
                return Some(Effect {
                    store: None,
                    eax: None,
                });
            }
            0xa1 | 0xc7 => {
                let mut code = [0; 11];
                if !read(pc, &mut code, Access::Code) || code[10] != 0xc3 {
                    return None;
                }
                if code[0] == 0xa1 && code[5] == 0xa3 {
                    let source = u32::from_le_bytes(code[1..5].try_into().ok()?);
                    let destination = u32::from_le_bytes(code[6..10].try_into().ok()?);
                    let mut value = [0; 4];
                    if !read(source, &mut value, Access::Data) {
                        return None;
                    }
                    let value = u32::from_le_bytes(value);
                    return Some(Effect {
                        store: Some((destination, value)),
                        eax: Some(value),
                    });
                }
                if code[..2] == [0xc7, 0x05] {
                    return Some(Effect {
                        store: Some((
                            u32::from_le_bytes(code[2..6].try_into().ok()?),
                            u32::from_le_bytes(code[6..10].try_into().ok()?),
                        )),
                        eax: None,
                    });
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(bytes: &[u8]) -> Option<Effect> {
        plan(0, |address, out, _| {
            let Some(input) = bytes.get(address as usize..address as usize + out.len()) else {
                return false;
            };
            out.copy_from_slice(input);
            true
        })
    }
    #[test]
    fn copy_reads_current_value_and_sets_eax() {
        let mut bytes = vec![0xa1, 11, 0, 0, 0, 0xa3, 32, 0, 0, 0, 0xc3, 1, 2, 3, 4];
        assert_eq!(
            run(&bytes),
            Some(Effect {
                store: Some((32, 0x04030201)),
                eax: Some(0x04030201)
            })
        );
        bytes[11] = 9;
        assert_eq!(run(&bytes).unwrap().eax, Some(0x04030209));
    }
    #[test]
    fn immediate_and_empty_preserve_eax() {
        assert_eq!(
            run(&[0xc7, 5, 32, 0, 0, 0, 7, 0, 0, 0, 0xc3]),
            Some(Effect {
                store: Some((32, 7)),
                eax: None
            })
        );
        assert_eq!(
            run(&[0xc3]),
            Some(Effect {
                store: None,
                eax: None
            })
        );
    }
    #[test]
    fn bounded_jumps_and_malformed_code_fall_back() {
        assert!(run(&[0xe9, 0, 0, 0, 0, 0xc3]).is_some());
        assert!(run(&[0xe9, 0xfb, 0xff, 0xff, 0xff]).is_none());
        assert!(run(&[0xe9, 0xf0, 0xff, 0xff, 0xff]).is_none());
        for bytes in [
            &[0xc2, 0, 0][..],
            &[0x90, 0xc3],
            &[0xa1],
            &[0xc7, 5, 0, 0, 0, 0, 1, 0, 0, 0, 0x90],
        ] {
            assert!(run(bytes).is_none());
        }
        assert!(plan(0, |_, _, _| false).is_none());
    }
}
