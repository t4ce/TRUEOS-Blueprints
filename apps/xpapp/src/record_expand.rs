//! Storm's in-place 16-byte to 44-byte record expansion.

pub const INPUT_BYTES: usize = 16;
pub const OUTPUT_BYTES: usize = 44;

/// Builds records 1..count. Record zero is finished by Storm's following code.
/// Read the entire compact input before writing the expanded output: the guest
/// loop copies backwards for exactly this reason.
pub fn build(source: &[u8], count: usize) -> Option<Vec<u8>> {
    if !(2..=65_536).contains(&count) || source.len() != count.checked_mul(INPUT_BYTES)? {
        return None;
    }
    let mut output = vec![0u8; (count - 1).checked_mul(OUTPUT_BYTES)?];
    for index in 1..count {
        let from = index * INPUT_BYTES;
        let to = (index - 1) * OUTPUT_BYTES;
        output[to..to + INPUT_BYTES].copy_from_slice(&source[from..from + INPUT_BYTES]);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backward_in_place_reference_for_boundary_counts() {
        for count in [2, 3, 10759] {
            let mut input = vec![0; count * INPUT_BYTES];
            for (index, byte) in input.iter_mut().enumerate() {
                *byte = (index as u8).wrapping_mul(73).wrapping_add(11);
            }
            let output = build(&input, count).unwrap();
            let mut reference = vec![0x99; count * OUTPUT_BYTES];
            reference[..input.len()].copy_from_slice(&input);
            for index in (1..count).rev() {
                let from = index * INPUT_BYTES;
                let to = index * OUTPUT_BYTES;
                reference.copy_within(from..from + INPUT_BYTES, to);
                reference[to + INPUT_BYTES..to + OUTPUT_BYTES].fill(0);
            }
            assert_eq!(output, reference[OUTPUT_BYTES..]);
            assert_eq!(&reference[..INPUT_BYTES], &input[..INPUT_BYTES]);
        }
    }
}
