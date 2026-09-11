//! Small allocation-free decoder and bounded encoders for cubesrv UDP v1.

extern crate alloc;

use alloc::vec::Vec;

pub const MAX_DATAGRAM: usize = 1_200;
pub const BLOB_CHUNK_BYTES: usize = 1_024;
const MAGIC: &[u8; 4] = b"CUB1";
const VERSION: u8 = 1;
const HEADER: usize = 8;

const HELLO: u8 = 1;
const TELEMETRY: u8 = 2;
const WORLD_REQUEST: u8 = 3;
const ASSET_REQUEST: u8 = 4;

const WELCOME: u8 = 0x81;
const PLAYER_STATE: u8 = 0x82;
const WORLD_CHUNK: u8 = 0x83;
const ASSET_CHUNK: u8 = 0x84;
const ERROR: u8 = 0xff;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Telemetry {
    pub sequence: u32,
    pub world_id: u8,
    pub position: [f32; 3],
    pub orientation: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClientPacket {
    Hello { world_id: u8 },
    SlideRequest { revision: u32, chunk: u16 },
    Telemetry(Telemetry),
    WorldRequest { chunk: u16 },
    AssetRequest { asset_id: u8, chunk: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Header,
    Version,
    Kind,
    Length,
    World,
    Float,
}

fn valid_world(world_id: u8) -> Result<u8, DecodeError> {
    if (1..=27).contains(&world_id) {
        Ok(world_id)
    } else {
        Err(DecodeError::World)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_vec3(bytes: &[u8], offset: usize) -> Result<[f32; 3], DecodeError> {
    let value = core::array::from_fn(|axis| {
        f32::from_le_bytes(
            bytes[offset + axis * 4..offset + axis * 4 + 4]
                .try_into()
                .unwrap(),
        )
    });
    value
        .iter()
        .all(|component| component.is_finite())
        .then_some(value)
        .ok_or(DecodeError::Float)
}

pub fn decode(bytes: &[u8]) -> Result<ClientPacket, DecodeError> {
    if bytes.len() < HEADER || &bytes[..4] != MAGIC {
        return Err(DecodeError::Header);
    }
    if bytes[4] != VERSION {
        return Err(DecodeError::Version);
    }
    let payload_len = read_u16(bytes, 6) as usize;
    if bytes.len() != HEADER + payload_len {
        return Err(DecodeError::Length);
    }
    let payload = &bytes[HEADER..];
    match bytes[5] {
        HELLO if payload.len() == 1 => Ok(ClientPacket::Hello {
            world_id: valid_world(payload[0])?,
        }),
        TELEMETRY if payload.len() == 29 => Ok(ClientPacket::Telemetry(Telemetry {
            sequence: read_u32(payload, 0),
            world_id: valid_world(payload[4])?,
            position: read_vec3(payload, 5)?,
            orientation: read_vec3(payload, 17)?,
        })),
        5 if payload.len() == 6 => Ok(ClientPacket::SlideRequest {
            revision: read_u32(payload, 0),
            chunk: read_u16(payload, 4),
        }),
        WORLD_REQUEST if payload.len() == 2 => Ok(ClientPacket::WorldRequest {
            chunk: read_u16(payload, 0),
        }),
        ASSET_REQUEST if payload.len() == 3 => Ok(ClientPacket::AssetRequest {
            asset_id: payload[0],
            chunk: read_u16(payload, 1),
        }),
        HELLO | TELEMETRY | WORLD_REQUEST | ASSET_REQUEST => Err(DecodeError::Length),
        _ => Err(DecodeError::Kind),
    }
}

fn packet(kind: u8, payload: &[u8]) -> Vec<u8> {
    debug_assert!(HEADER + payload.len() <= MAX_DATAGRAM);
    let mut bytes = Vec::with_capacity(HEADER + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.push(kind);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

pub fn welcome(
    player_id: u32,
    world_id: u8,
    world_bytes: usize,
    world_chunks: u16,
    asset_count: u8,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.extend_from_slice(&player_id.to_le_bytes());
    payload.push(world_id);
    payload.extend_from_slice(&(world_bytes as u32).to_le_bytes());
    payload.extend_from_slice(&world_chunks.to_le_bytes());
    payload.push(asset_count);
    packet(WELCOME, &payload)
}

pub fn player_state(player_id: u32, telemetry: Telemetry) -> Vec<u8> {
    let mut payload = Vec::with_capacity(33);
    payload.extend_from_slice(&player_id.to_le_bytes());
    payload.extend_from_slice(&telemetry.sequence.to_le_bytes());
    payload.push(telemetry.world_id);
    for value in telemetry.position.into_iter().chain(telemetry.orientation) {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    packet(PLAYER_STATE, &payload)
}

pub fn blob_chunk(kind: BlobKind, id: u8, requested: u16, blob: &[u8]) -> Option<Vec<u8>> {
    let chunk_count = blob.len().div_ceil(BLOB_CHUNK_BYTES);
    let start = requested as usize * BLOB_CHUNK_BYTES;
    if start >= blob.len() || chunk_count > u16::MAX as usize {
        return None;
    }
    let end = (start + BLOB_CHUNK_BYTES).min(blob.len());
    let mut payload = Vec::with_capacity(5 + end - start);
    payload.push(id);
    payload.extend_from_slice(&requested.to_le_bytes());
    payload.extend_from_slice(&(chunk_count as u16).to_le_bytes());
    payload.extend_from_slice(&blob[start..end]);
    Some(packet(
        match kind {
            BlobKind::World => WORLD_CHUNK,
            BlobKind::Asset => ASSET_CHUNK,
        },
        &payload,
    ))
}

#[derive(Clone, Copy)]
pub enum BlobKind {
    World,
    Asset,
}

pub fn error(code: u8) -> Vec<u8> {
    packet(ERROR, &[code])
}

pub fn is_newer(candidate: u32, current: u32) -> bool {
    let distance = candidate.wrapping_sub(current);
    distance != 0 && distance < (1 << 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(kind: u8, payload: &[u8]) -> Vec<u8> {
        packet(kind, payload)
    }

    #[test]
    fn telemetry_carries_world_and_vectors() {
        let mut payload = 7_u32.to_le_bytes().to_vec();
        payload.push(27);
        for value in [1.0_f32, 2.0, 3.0, 0.0, 0.0, -1.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode(&client(TELEMETRY, &payload)),
            Ok(ClientPacket::Telemetry(Telemetry {
                sequence: 7,
                world_id: 27,
                position: [1.0, 2.0, 3.0],
                orientation: [0.0, 0.0, -1.0],
            }))
        );
    }

    #[test]
    fn rejects_bad_world_and_non_finite_float() {
        assert_eq!(decode(&client(HELLO, &[0])), Err(DecodeError::World));
        let mut payload = 1_u32.to_le_bytes().to_vec();
        payload.push(1);
        payload.extend_from_slice(&f32::NAN.to_le_bytes());
        payload.extend_from_slice(&[0; 20]);
        assert_eq!(
            decode(&client(TELEMETRY, &payload)),
            Err(DecodeError::Float)
        );
    }

    #[test]
    fn wrapping_sequence_comparison_is_latest_wins() {
        assert!(is_newer(11, 10));
        assert!(!is_newer(10, 10));
        assert!(is_newer(0, u32::MAX));
        assert!(!is_newer(9, 10));
    }

    #[test]
    fn chunks_stay_below_datagram_limit() {
        let blob = [42_u8; BLOB_CHUNK_BYTES * 2 + 1];
        let first = blob_chunk(BlobKind::World, 1, 0, &blob).unwrap();
        let last = blob_chunk(BlobKind::World, 1, 2, &blob).unwrap();
        assert!(first.len() <= MAX_DATAGRAM);
        assert_eq!(last.len(), HEADER + 5 + 1);
        assert!(blob_chunk(BlobKind::World, 1, 3, &blob).is_none());
    }
}

/// The revision pins a transfer even when the ten-second clock advances.
/// Every manifest carries the connect spawn; clients apply it once per session.
pub fn slide_info(player: u32, revision: u32) -> Vec<u8> {
    let mut body = player.to_le_bytes().to_vec();
    body.extend_from_slice(&revision.to_le_bytes());
    for component in [0.0f32; 3] {
        body.extend_from_slice(&component.to_le_bytes());
    }
    packet(0x85, &body)
}
pub fn slide_chunk(revision: u32, chunk: u16, bytes: &[u8]) -> Option<Vec<u8>> {
    let start = chunk as usize * BLOB_CHUNK_BYTES;
    if start >= bytes.len() {
        return None;
    }
    let mut body = revision.to_le_bytes().to_vec();
    body.extend_from_slice(&chunk.to_le_bytes());
    body.extend_from_slice(&bytes[start..(start + BLOB_CHUNK_BYTES).min(bytes.len())]);
    Some(packet(0x86, &body))
}
