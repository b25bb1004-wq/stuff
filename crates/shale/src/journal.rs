//! Write-ahead journal in the region `mkfs` reserves. Journal block 0 holds the commit record;
//! blocks 1.. hold copies of the logged blocks. A commit writes the copies, then the record with
//! a CRC-32C over the copies, then the blocks home, then clears the record. Mount replays a
//! complete record and drops a torn one. Blocks that were free before the operation are written
//! home first: nothing refers to them yet, so they are invisible until the record commits.
use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::crc::{crc32c, crc_begin, crc_end, crc_update};
use crate::disk::BlockDevice;
use crate::layout::{put, sub, u32_at, u64_at, Geo};
use crate::{Error, BLOCK};

const JMAGIC: &[u8] = b"SHJRNL02";
const REC_CRC: usize = BLOCK - 4;
/// Most logged blocks one commit record can name.
const JMAX: usize = (BLOCK - 20) / 8;

/// One block of memory.
pub(crate) type Page = Box<[u8; BLOCK]>;

/// Logged blocks one transaction may hold: the journal slots, capped by the record size.
pub(crate) fn capacity(geo: &Geo) -> usize {
    let slots = usize::try_from(geo.journal_len.saturating_sub(1)).unwrap_or(0);
    slots.min(JMAX)
}

/// A complete transaction read back from the journal: home block numbers and their copies.
pub(crate) struct Pending {
    homes: Vec<u64>,
    copies: Vec<Page>,
}

/// What the journal header holds.
pub(crate) enum Peek {
    /// No record: the journal is empty.
    Clean,
    /// A record with a bad checksum or bad fields: a torn commit, to be dropped.
    Invalid,
    /// A complete transaction that has not been written home yet.
    Committed(Pending),
}

/// What `recover` did.
pub(crate) enum Recovery {
    /// The journal was already empty.
    Clean,
    /// A complete transaction was written home.
    Replayed,
    /// A torn record was dropped.
    Discarded,
}

fn new_page() -> Page {
    Box::new([0u8; BLOCK])
}

fn copy_block(geo: &Geo, i: usize) -> Result<u64, Error> {
    geo.journal_start
        .checked_add(1 + i as u64)
        .ok_or(Error::Corrupt)
}

/// Read the journal header and, if it names a complete transaction, its copies.
pub(crate) fn peek<D: BlockDevice>(dev: &mut D, geo: &Geo) -> Result<Peek, Error> {
    let mut rec = new_page();
    dev.read(geo.journal_start, &mut rec)?;
    let r = &rec[..];
    if sub(r, 0, JMAGIC.len())? != JMAGIC {
        return Ok(Peek::Clean);
    }
    if u32_at(r, REC_CRC)? != crc32c(sub(r, 0, REC_CRC)?) {
        return Ok(Peek::Invalid);
    }
    let n = u32_at(r, 8)? as usize;
    if n > capacity(geo) {
        return Ok(Peek::Invalid);
    }
    let mut homes = Vec::with_capacity(n);
    for i in 0..n {
        let h = u64_at(r, 16 + 8 * i)?;
        if h < geo.bitmap_start || h >= geo.blocks {
            return Ok(Peek::Invalid);
        }
        homes.push(h);
    }
    let mut copies = Vec::with_capacity(n);
    let mut crc = crc_begin();
    for i in 0..n {
        let mut c = new_page();
        dev.read(copy_block(geo, i)?, &mut c)?;
        crc = crc_update(crc, &c[..]);
        copies.push(c);
    }
    if u32_at(r, 12)? != crc_end(crc) {
        return Ok(Peek::Invalid);
    }
    Ok(Peek::Committed(Pending { homes, copies }))
}

/// Empty the journal: zero the commit record and make that durable.
fn clear<D: BlockDevice>(dev: &mut D, geo: &Geo) -> Result<(), Error> {
    dev.write(geo.journal_start, &[0u8; BLOCK])?;
    dev.flush()
}

/// Bring the device to a state with no pending transaction: replay a complete one, drop a torn one.
pub(crate) fn recover<D: BlockDevice>(dev: &mut D, geo: &Geo) -> Result<Recovery, Error> {
    match peek(dev, geo)? {
        Peek::Clean => Ok(Recovery::Clean),
        Peek::Invalid => {
            clear(dev, geo)?;
            Ok(Recovery::Discarded)
        }
        Peek::Committed(p) => {
            for (h, c) in p.homes.iter().zip(p.copies.iter()) {
                dev.write(*h, c)?;
            }
            dev.flush()?;
            clear(dev, geo)?;
            Ok(Recovery::Replayed)
        }
    }
}

/// Write the commit record naming `logged` (in order) with checksum `data_crc`.
fn encode_record(
    logged: &[(u64, &[u8; BLOCK])],
    data_crc: u32,
    out: &mut [u8],
) -> Result<(), Error> {
    out.fill(0);
    put(out, 0, JMAGIC)?;
    let n = u32::try_from(logged.len()).map_err(|_| Error::TooLarge)?;
    put(out, 8, &n.to_le_bytes())?;
    put(out, 12, &data_crc.to_le_bytes())?;
    for (i, (h, _)) in logged.iter().enumerate() {
        put(out, 16 + 8 * i, &h.to_le_bytes())?;
    }
    let crc = crc32c(sub(out, 0, REC_CRC)?);
    put(out, REC_CRC, &crc.to_le_bytes())
}

/// Commit one transaction. `direct` blocks are written home first; `logged` blocks go through
/// the journal. TooLarge, before anything is written, if `logged` does not fit the journal.
pub(crate) fn commit<D: BlockDevice>(
    dev: &mut D,
    geo: &Geo,
    direct: &[(u64, &[u8; BLOCK])],
    logged: &[(u64, &[u8; BLOCK])],
) -> Result<(), Error> {
    if logged.len() > capacity(geo) {
        return Err(Error::TooLarge);
    }
    if direct.is_empty() && logged.is_empty() {
        return Ok(());
    }
    for (b, data) in direct {
        dev.write(*b, data)?;
    }
    dev.flush()?;
    if logged.is_empty() {
        return Ok(());
    }
    let mut crc = crc_begin();
    for (i, (_, data)) in logged.iter().enumerate() {
        dev.write(copy_block(geo, i)?, data)?;
        crc = crc_update(crc, &data[..]);
    }
    dev.flush()?;
    let mut rec = new_page();
    encode_record(logged, crc_end(crc), &mut rec[..])?;
    dev.write(geo.journal_start, &rec)?;
    dev.flush()?;
    for (b, data) in logged {
        dev.write(*b, data)?;
    }
    dev.flush()?;
    clear(dev, geo)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::MemDisk;

    #[test]
    fn capacity_is_capped_by_the_record() {
        let small = Geo::plan(4096, 64, 8).unwrap();
        assert_eq!(capacity(&small), 7);
        let big = Geo::plan(4096, 64, 1000).unwrap();
        assert_eq!(capacity(&big), JMAX);
    }

    #[test]
    fn empty_journal_is_clean() {
        let geo = Geo::plan(64, 32, 8).unwrap();
        let mut d = MemDisk::new(64);
        assert!(matches!(peek(&mut d, &geo), Ok(Peek::Clean)));
        assert!(matches!(recover(&mut d, &geo), Ok(Recovery::Clean)));
    }
}
