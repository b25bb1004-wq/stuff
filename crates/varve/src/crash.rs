//! Crash-simulation storage wrapper implementing the Varve sector-loss model.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::{Error, Storage};

const SECTOR_SIZE: u64 = 512;

/// Storage wrapper that buffers unsynced writes and simulates 512-byte sector loss on crash.
#[derive(Debug)]
pub struct CrashStorage<S> {
    durable: S,
    pending: Vec<(u64, Vec<u8>)>,
    logical_len: u64,
    op_count: usize,
    budget: Option<Arc<AtomicUsize>>,
}

impl<S: Storage> CrashStorage<S> {
    /// Wrap `durable` without an automatic fault-injection budget.
    pub fn new(mut durable: S) -> Result<Self, Error> {
        let logical_len = durable.len()?;
        Ok(Self {
            durable,
            pending: Vec::new(),
            logical_len,
            op_count: 0,
            budget: None,
        })
    }

    /// Wrap `durable` with a shared operation budget that fails with `Error::Io` once exhausted.
    pub fn with_budget(mut durable: S, budget: Arc<AtomicUsize>) -> Result<Self, Error> {
        let logical_len = durable.len()?;
        Ok(Self {
            durable,
            pending: Vec::new(),
            logical_len,
            op_count: 0,
            budget: Some(budget),
        })
    }

    /// Number of mutating storage operations (`write_at`, `set_len`, `sync`) attempted.
    pub fn op_count(&self) -> usize {
        self.op_count
    }

    /// Number of unsynced writes currently buffered.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Apply a crash where each 512-byte sector of each unsynced write survives iff `keep(write_idx, sector_idx)`.
    pub fn crash_with(&mut self, mut keep: impl FnMut(usize, u64) -> bool) -> Result<(), Error> {
        let writes = std::mem::take(&mut self.pending);
        for (write_idx, (w_off, w_data)) in writes.into_iter().enumerate() {
            let mut pos = 0usize;
            while pos < w_data.len() {
                let cur_off = w_off.checked_add(pos as u64).ok_or(Error::Io)?;
                let sector_idx = cur_off / SECTOR_SIZE;
                let sector_end = (sector_idx + 1) * SECTOR_SIZE;
                let max_in_sector = usize::try_from(sector_end - cur_off).unwrap_or(usize::MAX);
                let chunk_len = max_in_sector.min(w_data.len() - pos);
                if keep(write_idx, sector_idx) {
                    if let Some(chunk) = w_data.get(pos..pos + chunk_len) {
                        self.durable.write_at(cur_off, chunk)?;
                    }
                }
                pos += chunk_len;
            }
        }
        self.logical_len = self.durable.len()?;
        Ok(())
    }

    /// Discard all unsynced writes (every unsynced sector is lost).
    pub fn crash_drop_all(&mut self) -> Result<(), Error> {
        self.crash_with(|_, _| false)
    }

    /// Flush all unsynced writes without a sync error (every unsynced sector survives).
    pub fn crash_keep_all(&mut self) -> Result<(), Error> {
        self.crash_with(|_, _| true)
    }

    /// Consume the wrapper and return the underlying durable store, dropping un-crashed writes.
    pub fn into_inner(self) -> S {
        self.durable
    }

    /// Borrow the underlying durable store.
    pub fn inner(&self) -> &S {
        &self.durable
    }

    fn check_budget(&mut self) -> Result<(), Error> {
        self.op_count = self.op_count.saturating_add(1);
        if let Some(budget) = &self.budget {
            let cur = budget.load(Ordering::SeqCst);
            if cur == 0 {
                return Err(Error::Io);
            }
            budget.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

impl<S: Storage> Storage for CrashStorage<S> {
    fn len(&mut self) -> Result<u64, Error> {
        Ok(self.logical_len)
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), Error> {
        let end = off.checked_add(buf.len() as u64).ok_or(Error::Io)?;
        if end > self.logical_len {
            return Err(Error::Io);
        }
        buf.fill(0);
        let durable_len = self.durable.len()?;
        if off < durable_len {
            let avail_u64 = (durable_len - off).min(buf.len() as u64);
            let avail = usize::try_from(avail_u64).map_err(|_| Error::Io)?;
            if let Some(dst) = buf.get_mut(..avail) {
                self.durable.read_at(off, dst)?;
            }
        }
        for (w_off, w_data) in &self.pending {
            let Some(w_end) = w_off.checked_add(w_data.len() as u64) else {
                continue;
            };
            let ov_start = off.max(*w_off);
            let ov_end = end.min(w_end);
            if ov_start < ov_end {
                let dst_start = usize::try_from(ov_start - off).map_err(|_| Error::Io)?;
                let dst_end = usize::try_from(ov_end - off).map_err(|_| Error::Io)?;
                let src_start = usize::try_from(ov_start - *w_off).map_err(|_| Error::Io)?;
                let src_end = usize::try_from(ov_end - *w_off).map_err(|_| Error::Io)?;
                if let (Some(dst), Some(src)) = (
                    buf.get_mut(dst_start..dst_end),
                    w_data.get(src_start..src_end),
                ) {
                    dst.copy_from_slice(src);
                }
            }
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), Error> {
        self.check_budget()?;
        let end = off.checked_add(data.len() as u64).ok_or(Error::Io)?;
        self.pending.push((off, data.to_vec()));
        if end > self.logical_len {
            self.logical_len = end;
        }
        Ok(())
    }

    fn set_len(&mut self, n: u64) -> Result<(), Error> {
        self.check_budget()?;
        self.durable.set_len(n)?;
        self.logical_len = n;
        let mut kept = Vec::with_capacity(self.pending.len());
        for (w_off, mut w_data) in self.pending.drain(..) {
            if w_off < n {
                let max_len = usize::try_from(n - w_off).unwrap_or(usize::MAX);
                if w_data.len() > max_len {
                    w_data.truncate(max_len);
                }
                kept.push((w_off, w_data));
            }
        }
        self.pending = kept;
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> {
        self.check_budget()?;
        let writes = std::mem::take(&mut self.pending);
        for (w_off, w_data) in writes {
            self.durable.write_at(w_off, &w_data)?;
        }
        self.durable.sync()?;
        self.logical_len = self.durable.len()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::MemStorage;

    #[test]
    fn sector_loss_and_durable_set_len() {
        let mem = MemStorage::new();
        let mut cs = CrashStorage::new(mem).unwrap();
        cs.write_at(0, &[1u8; 512]).unwrap();
        cs.sync().unwrap();

        // Two-sector write: keep sector 1, drop sector 0 and sector 2.
        cs.write_at(0, &[2u8; 1536]).unwrap();
        cs.crash_with(|_, sector| sector == 1).unwrap();
        assert_eq!(cs.inner().bytes().len(), 1024);
        assert_eq!(&cs.inner().bytes()[..512], &[1u8; 512]);
        assert_eq!(&cs.inner().bytes()[512..1024], &[2u8; 512]);

        // set_len is durable at once.
        cs.set_len(600).unwrap();
        cs.crash_drop_all().unwrap();
        assert_eq!(cs.inner().bytes().len(), 600);
    }
}
