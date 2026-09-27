//! War3's 2 × 1024 event bank construction, leaving the CPU-dependent tail to x86.

pub const HANDLES_BASE: u32 = 0x0045_70b8;
pub const GENERATIONS_BASE: u32 = 0x0045_90b8;
pub const BANK_LEN: usize = 1024;

pub struct Tables {
    pub handles: Vec<u8>,
    pub generations: [Vec<u8>; 2],
}

/// The closure creates a real unnamed event in allocation order. Returning 0
/// denotes failure, which the caller treats as fatal after any prior effects.
pub fn build(mut create: impl FnMut(bool) -> u32) -> Result<Tables, &'static str> {
    let mut handles = vec![0u8; BANK_LEN * 2 * 4];
    let mut generations = [vec![0u8; BANK_LEN * 4], vec![0u8; BANK_LEN * 4]];
    for bank in 0..2usize {
        for index in 0..BANK_LEN {
            let handle = create(bank == 1);
            if handle == 0 {
                return Err("event allocation returned null");
            }
            let offset = (bank * BANK_LEN + index) * 4;
            handles[offset..offset + 4].copy_from_slice(&handle.to_le_bytes());
            let generation = ((index as u32) + 1).wrapping_mul(0x0020_0000);
            let offset = index * 4;
            generations[bank][offset..offset + 4].copy_from_slice(&generation.to_le_bytes());
        }
    }
    Ok(Tables {
        handles,
        generations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_banks_preserve_handle_order_reset_mode_and_gap() {
        let mut next = 0x5743_200b;
        let mut manual_count = 0;
        let tables = build(|manual| {
            manual_count += usize::from(manual);
            let handle = next;
            next += 1;
            handle
        })
        .unwrap();
        assert_eq!(manual_count, 1024);
        assert_eq!(next, 0x5743_280b);
        let word = |bytes: &[u8], index: usize| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        };
        assert_eq!(word(&tables.handles, 0), 0x5743_200b);
        assert_eq!(word(&tables.handles, 1023), 0x5743_240a);
        assert_eq!(word(&tables.handles, 1024), 0x5743_240b);
        assert_eq!(word(&tables.generations[0], 0), 0x0020_0000);
        assert_eq!(word(&tables.generations[1], 1023), 0x8000_0000);
        assert_eq!(
            GENERATIONS_BASE + 0x2000 - (GENERATIONS_BASE + 0x1000),
            0x1000
        );
    }
}
