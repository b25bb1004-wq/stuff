//! Binary pack encoding and decoding (`DPK1`) with SHA-256 integrity trailer.

use alloc::vec::Vec;
use onyx::sha256::Sha256;

use super::RemoteError;
use crate::id::Kind;

pub(crate) const MAX_PACK_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_PACK_OBJECTS: usize = 100_000;

fn kind_to_byte(kind: Kind) -> u8 {
    match kind {
        Kind::Blob => 1,
        Kind::Tree => 2,
        Kind::Commit => 3,
    }
}

fn byte_to_kind(b: u8) -> Option<Kind> {
    match b {
        1 => Some(Kind::Blob),
        2 => Some(Kind::Tree),
        3 => Some(Kind::Commit),
        _ => None,
    }
}

/// Encode `objects` as a `DPK1` pack with a trailing 32-byte SHA-256 checksum.
pub fn encode_pack(objects: &[(Kind, Vec<u8>)]) -> Vec<u8> {
    let total_payload: usize = objects
        .iter()
        .map(|(_, d)| 5usize.saturating_add(d.len()))
        .sum();
    let mut out = Vec::with_capacity(8usize.saturating_add(total_payload).saturating_add(32));
    out.extend_from_slice(b"DPK1");
    let count = u32::try_from(objects.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for (kind, data) in objects {
        out.push(kind_to_byte(*kind));
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(data);
    }
    let mut hasher = Sha256::new();
    hasher.update(&out);
    let digest = hasher.finalize();
    out.extend_from_slice(&digest);
    out
}

/// Decode and verify a `DPK1` pack, rejecting packs with more than `max_objects` entries.
pub fn decode_pack(pack: &[u8], max_objects: usize) -> Result<Vec<(Kind, Vec<u8>)>, RemoteError> {
    if pack.len() < 40 {
        return Err(RemoteError::Protocol);
    }
    let (payload, trailer) = pack.split_at(pack.len() - 32);
    if payload.get(..4) != Some(b"DPK1".as_slice()) {
        return Err(RemoteError::Protocol);
    }

    let mut hasher = Sha256::new();
    hasher.update(payload);
    if hasher.finalize().as_slice() != trailer {
        return Err(RemoteError::Protocol);
    }

    let count_bytes = payload.get(4..8).ok_or(RemoteError::Protocol)?;
    let count = u32::from_be_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
    ]) as usize;
    if count > max_objects {
        return Err(RemoteError::Protocol);
    }

    let mut rest = &payload[8..];
    if count.saturating_mul(5) > rest.len() {
        return Err(RemoteError::Protocol);
    }

    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let (&kind_b, after_kind) = rest.split_first().ok_or(RemoteError::Protocol)?;
        let kind = byte_to_kind(kind_b).ok_or(RemoteError::Protocol)?;
        let len_bytes = after_kind.get(..4).ok_or(RemoteError::Protocol)?;
        let len =
            u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        let after_len = &after_kind[4..];
        let data = after_len.get(..len).ok_or(RemoteError::Protocol)?;
        out.push((kind, data.to_vec()));
        rest = &after_len[len..];
    }

    if !rest.is_empty() {
        return Err(RemoteError::Protocol);
    }
    Ok(out)
}
