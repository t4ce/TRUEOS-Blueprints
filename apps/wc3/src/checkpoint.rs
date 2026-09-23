//! Checked CPU/page-delta codec shared by the table and audited loop checkpoints.
use sha2::{Digest, Sha256};
use trueos::x86::{DebugRegisters, ExtendedState, Registers, X86_EXTENDED_STATE_BYTES};

#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub from: u32,
    pub to: u32,
    pub table_bytes: u32,
}

/// Exact input and page-set comparison, to be completed before any guest write.
pub fn matches_inputs(
    checkpoint: &TableCheckpoint,
    identity: [u8; 32],
    registers: Registers,
    debug: DebugRegisters,
    extended: &ExtendedState,
    pages: impl Iterator<Item = (u32, [u8; 32])>,
) -> bool {
    checkpoint.table_base == 0
        && checkpoint.image_sha256 == identity
        && checkpoint.before_registers == registers
        && checkpoint.before_debug_registers == debug
        && checkpoint.before_extended_state == *extended
        && checkpoint
            .pages
            .iter()
            .map(|p| (p.va, p.before_sha256))
            .eq(pages)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DwordScanWatch {
    pub last_heartbeat_index: Option<u32>,
}
#[cfg(test)]
const TEST_BOUNDARY: Boundary = Boundary {
    from: 0x004614a5,
    to: 0x004614c5,
    table_bytes: 0x8000,
};
fn checkpoint_sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

const TABLE_CHECKPOINT_MAGIC: &[u8; 8] = b"WC3TFCP1";
const TABLE_CHECKPOINT_VERSION: u32 = 1;
const TABLE_CHECKPOINT_PAGE_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCheckpointPage {
    pub va: u32,
    pub before_sha256: [u8; 32],
    pub after: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCheckpoint {
    pub image_sha256: [u8; 32],
    pub table_base: u32,
    pub before_registers: Registers,
    pub before_debug_registers: DebugRegisters,
    pub before_extended_state: ExtendedState,
    pub after_registers: Registers,
    pub after_debug_registers: DebugRegisters,
    pub after_extended_state: ExtendedState,
    pub pages: Vec<TableCheckpointPage>,
    pub after_single_step_count: u64,
    pub after_dword_scan_watch: Option<DwordScanWatch>,
}

#[cfg(test)]
mod table_checkpoint_tests {
    use super::*;

    #[test]
    fn table_checkpoint_codec_rejects_tampering_and_round_trips_extended_state() {
        let mut before_extended_state = ExtendedState {
            mask: 0x7,
            bytes: [0; X86_EXTENDED_STATE_BYTES],
        };
        before_extended_state.bytes[0] = 0x37;
        let mut after_extended_state = before_extended_state;
        after_extended_state.bytes[831] = 0xa5;
        let checkpoint = TableCheckpoint {
            image_sha256: [0x11; 32],
            table_base: 0x1400_b810,
            before_registers: Registers {
                eip: TEST_BOUNDARY.from,
                eflags: 0x106,
                ..Registers::default()
            },
            before_debug_registers: DebugRegisters {
                dr0: 1,
                dr6: 0x4000,
                dr7: 0x403,
                ..DebugRegisters::default()
            },
            before_extended_state,
            after_registers: Registers {
                eip: TEST_BOUNDARY.to,
                eflags: 0x106,
                ..Registers::default()
            },
            after_debug_registers: DebugRegisters {
                dr0: 1,
                dr6: 0x4000,
                dr7: 0x403,
                ..DebugRegisters::default()
            },
            after_extended_state,
            pages: vec![
                TableCheckpointPage {
                    va: 0x0040_0000,
                    before_sha256: [0x22; 32],
                    after: None,
                },
                TableCheckpointPage {
                    va: 0x1400_b000,
                    before_sha256: [0x33; 32],
                    after: Some(vec![0x44; TABLE_CHECKPOINT_PAGE_BYTES]),
                },
            ],
            after_single_step_count: 0x2000,
            after_dword_scan_watch: Some(DwordScanWatch {
                last_heartbeat_index: Some(0x2000),
            }),
        };
        let encoded = encode(&checkpoint, TEST_BOUNDARY).unwrap();
        let decoded = decode(&encoded, TEST_BOUNDARY).unwrap();
        assert_eq!(decoded, checkpoint);

        let mut tampered = encoded;
        tampered[20] ^= 1;
        assert_eq!(
            decode(&tampered, TEST_BOUNDARY),
            Err("table checkpoint checksum".into())
        );
    }
}

fn checkpoint_put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn checkpoint_put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn checkpoint_put_registers(out: &mut Vec<u8>, registers: Registers) {
    for value in [
        registers.eax,
        registers.ebx,
        registers.ecx,
        registers.edx,
        registers.esi,
        registers.edi,
        registers.ebp,
        registers.esp,
        registers.eip,
        registers.eflags,
        registers.fs_base,
    ] {
        checkpoint_put_u32(out, value);
    }
}

fn checkpoint_put_debug_registers(out: &mut Vec<u8>, registers: DebugRegisters) {
    for value in [
        registers.dr0,
        registers.dr1,
        registers.dr2,
        registers.dr3,
        registers.dr6,
        registers.dr7,
    ] {
        checkpoint_put_u32(out, value);
    }
}

fn checkpoint_put_extended_state(out: &mut Vec<u8>, state: &ExtendedState) {
    checkpoint_put_u64(out, state.mask);
    out.extend_from_slice(&state.bytes);
}

pub fn encode(checkpoint: &TableCheckpoint, boundary: Boundary) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.extend_from_slice(TABLE_CHECKPOINT_MAGIC);
    checkpoint_put_u32(&mut out, TABLE_CHECKPOINT_VERSION);
    out.extend_from_slice(&checkpoint.image_sha256);
    checkpoint_put_u32(&mut out, boundary.from);
    checkpoint_put_u32(&mut out, boundary.to);
    checkpoint_put_u32(&mut out, checkpoint.table_base);
    checkpoint_put_u32(&mut out, boundary.table_bytes);
    checkpoint_put_registers(&mut out, checkpoint.before_registers);
    checkpoint_put_debug_registers(&mut out, checkpoint.before_debug_registers);
    checkpoint_put_extended_state(&mut out, &checkpoint.before_extended_state);
    checkpoint_put_registers(&mut out, checkpoint.after_registers);
    checkpoint_put_debug_registers(&mut out, checkpoint.after_debug_registers);
    checkpoint_put_extended_state(&mut out, &checkpoint.after_extended_state);
    checkpoint_put_u64(&mut out, checkpoint.after_single_step_count);
    match checkpoint.after_dword_scan_watch {
        Some(watch) => {
            out.push(1);
            checkpoint_put_u32(&mut out, watch.last_heartbeat_index.unwrap_or(u32::MAX));
        }
        None => out.push(0),
    }
    checkpoint_put_u32(
        &mut out,
        u32::try_from(checkpoint.pages.len()).map_err(|_| "table checkpoint page count")?,
    );
    for page in &checkpoint.pages {
        checkpoint_put_u32(&mut out, page.va);
        out.extend_from_slice(&page.before_sha256);
        match &page.after {
            Some(after) => {
                if after.len() != TABLE_CHECKPOINT_PAGE_BYTES {
                    return Err("table checkpoint changed page length".into());
                }
                out.push(1);
                out.extend_from_slice(after);
            }
            None => out.push(0),
        }
    }
    let digest = checkpoint_sha256(&out);
    out.extend_from_slice(&digest);
    Ok(out)
}

struct CheckpointReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CheckpointReader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or("table checkpoint decode overflow")?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or("table checkpoint truncated")?;
        self.offset = end;
        Ok(slice)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.take(N)?
            .try_into()
            .map_err(|_| "table checkpoint array".to_owned())
    }
}

fn checkpoint_get_registers(input: &mut CheckpointReader<'_>) -> Result<Registers, String> {
    Ok(Registers {
        eax: input.u32()?,
        ebx: input.u32()?,
        ecx: input.u32()?,
        edx: input.u32()?,
        esi: input.u32()?,
        edi: input.u32()?,
        ebp: input.u32()?,
        esp: input.u32()?,
        eip: input.u32()?,
        eflags: input.u32()?,
        fs_base: input.u32()?,
        ..Registers::default()
    })
}

fn checkpoint_get_debug_registers(
    input: &mut CheckpointReader<'_>,
) -> Result<DebugRegisters, String> {
    Ok(DebugRegisters {
        dr0: input.u32()?,
        dr1: input.u32()?,
        dr2: input.u32()?,
        dr3: input.u32()?,
        dr6: input.u32()?,
        dr7: input.u32()?,
    })
}

fn checkpoint_get_extended_state(
    input: &mut CheckpointReader<'_>,
) -> Result<ExtendedState, String> {
    Ok(ExtendedState {
        mask: input.u64()?,
        bytes: input.array::<X86_EXTENDED_STATE_BYTES>()?,
    })
}

pub fn decode(bytes: &[u8], boundary: Boundary) -> Result<TableCheckpoint, String> {
    if bytes.len() < TABLE_CHECKPOINT_MAGIC.len() + 32 {
        return Err("table checkpoint truncated".into());
    }
    let (body, trailing) = bytes.split_at(bytes.len() - 32);
    if checkpoint_sha256(body) != <[u8; 32]>::try_from(trailing).unwrap() {
        return Err("table checkpoint checksum".into());
    }
    let mut input = CheckpointReader {
        bytes: body,
        offset: 0,
    };
    if input.array::<8>()? != *TABLE_CHECKPOINT_MAGIC || input.u32()? != TABLE_CHECKPOINT_VERSION {
        return Err("table checkpoint format".into());
    }
    let image_sha256 = input.array::<32>()?;
    if input.u32()? != boundary.from || input.u32()? != boundary.to {
        return Err("table checkpoint boundaries".into());
    }
    let table_base = input.u32()?;
    if input.u32()? != boundary.table_bytes {
        return Err("table checkpoint table bytes".into());
    }
    let before_registers = checkpoint_get_registers(&mut input)?;
    let before_debug_registers = checkpoint_get_debug_registers(&mut input)?;
    let before_extended_state = checkpoint_get_extended_state(&mut input)?;
    let after_registers = checkpoint_get_registers(&mut input)?;
    let after_debug_registers = checkpoint_get_debug_registers(&mut input)?;
    let after_extended_state = checkpoint_get_extended_state(&mut input)?;
    if before_registers.eip != boundary.from || after_registers.eip != boundary.to {
        return Err("table checkpoint register boundaries".into());
    }
    let after_single_step_count = input.u64()?;
    let after_dword_scan_watch = match input.take(1)?[0] {
        0 => None,
        1 => Some(DwordScanWatch {
            last_heartbeat_index: match input.u32()? {
                u32::MAX => None,
                value => Some(value),
            },
        }),
        _ => return Err("table checkpoint dword watch".into()),
    };
    let page_count = usize::try_from(input.u32()?).map_err(|_| "table checkpoint page count")?;
    if page_count > 4096 {
        return Err("table checkpoint excessive pages".into());
    }
    let mut pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        let va = input.u32()?;
        if va & 4095 != 0
            || pages
                .last()
                .is_some_and(|p: &TableCheckpointPage| p.va >= va)
        {
            return Err("table checkpoint page order/alignment".into());
        }
        let before_sha256 = input.array::<32>()?;
        let after = match input.take(1)?[0] {
            0 => None,
            1 => Some(input.take(TABLE_CHECKPOINT_PAGE_BYTES)?.to_vec()),
            _ => return Err("table checkpoint changed-page marker".into()),
        };
        pages.push(TableCheckpointPage {
            va,
            before_sha256,
            after,
        });
    }
    if input.offset != body.len() {
        return Err("table checkpoint trailing body".into());
    }
    Ok(TableCheckpoint {
        image_sha256,
        table_base,
        before_registers,
        before_debug_registers,
        before_extended_state,
        after_registers,
        after_debug_registers,
        after_extended_state,
        pages,
        after_single_step_count,
        after_dword_scan_watch,
    })
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    fn fixture() -> TableCheckpoint {
        TableCheckpoint {
            image_sha256: [1; 32],
            table_base: 0,
            before_registers: Registers {
                eip: TEST_BOUNDARY.from,
                ..Registers::default()
            },
            after_registers: Registers {
                eip: TEST_BOUNDARY.to,
                ..Registers::default()
            },
            before_debug_registers: DebugRegisters::default(),
            after_debug_registers: DebugRegisters::default(),
            before_extended_state: ExtendedState {
                mask: 7,
                bytes: [0; X86_EXTENDED_STATE_BYTES],
            },
            after_extended_state: ExtendedState {
                mask: 7,
                bytes: [0; X86_EXTENDED_STATE_BYTES],
            },
            pages: vec![
                TableCheckpointPage {
                    va: 0x400000,
                    before_sha256: [2; 32],
                    after: Some(vec![3; 4096]),
                },
                TableCheckpointPage {
                    va: 0x401000,
                    before_sha256: [4; 32],
                    after: None,
                },
            ],
            after_single_step_count: 123,
            after_dword_scan_watch: None,
        }
    }
    #[test]
    fn input_guard_rejects_every_cpu_and_memory_mismatch() {
        let c = fixture();
        let valid = |candidate: &TableCheckpoint| {
            matches_inputs(
                candidate,
                c.image_sha256,
                c.before_registers,
                c.before_debug_registers,
                &c.before_extended_state,
                c.pages.iter().map(|p| (p.va, p.before_sha256)),
            )
        };
        assert!(valid(&c));
        let mut bad = c.clone();
        bad.image_sha256[0] ^= 1;
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.before_registers.eflags ^= 0x100;
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.before_debug_registers.dr7 ^= 1;
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.before_extended_state.bytes[500] ^= 1;
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.pages[1].before_sha256[0] ^= 1;
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.pages.pop();
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.pages.push(c.pages[1].clone());
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.pages.reverse();
        assert!(!valid(&bad));
        let mut bad = c.clone();
        bad.pages[1].va += 4096;
        assert!(!valid(&bad));
    }
    #[test]
    fn codec_rejects_wrong_region_truncation_and_invalid_page_sets() {
        let c = fixture();
        let bytes = encode(&c, TEST_BOUNDARY).unwrap();
        assert!(
            decode(
                &bytes,
                Boundary {
                    from: 0x45af54,
                    to: 0x45b005,
                    table_bytes: 0
                }
            )
            .is_err()
        );
        for len in [0, 7, 40, bytes.len() - 32, bytes.len() - 1] {
            assert!(decode(&bytes[..len], TEST_BOUNDARY).is_err());
        }
        let mut bad = c.clone();
        bad.pages[1].va = bad.pages[0].va;
        assert!(decode(&encode(&bad, TEST_BOUNDARY).unwrap(), TEST_BOUNDARY).is_err());
        let mut bad = c.clone();
        bad.pages[1].va += 1;
        assert!(decode(&encode(&bad, TEST_BOUNDARY).unwrap(), TEST_BOUNDARY).is_err());
        let mut bad = c.clone();
        bad.after_registers.eip += 1;
        assert!(decode(&encode(&bad, TEST_BOUNDARY).unwrap(), TEST_BOUNDARY).is_err());
        assert_eq!(decode(&bytes, TEST_BOUNDARY).unwrap(), c);
    }
}
