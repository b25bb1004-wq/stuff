//! Page manager with write-ahead logging, crash recovery, and snapshot reads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::format::{
    decode_free_page, encode_free_page, encode_page0, encode_wal_frame, encode_zero_page,
    stamp_page_crc, FrameMeta, Page0Meta, WAL_FRAME_SIZE,
};
use crate::wal::{
    checkpoint_wal, frame_offset, read_log_frame_page, read_main_page, recover_or_init,
};
use crate::{Error, Page, Storage};

const AUTO_CHECKPOINT_FRAMES: u64 = 1000;

static NEXT_PAGER_ID: AtomicU64 = AtomicU64::new(1);

/// Read snapshot handle marking a committed point in the write-ahead log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pager_id: u64,
    id: u64,
}

#[derive(Debug, Clone, Copy)]
struct SnapshotState {
    mark: u64,
    page_count: u64,
}

#[derive(Debug)]
struct WriteTx {
    page_count: u64,
    free_head: u64,
    root: u64,
    dirty: BTreeMap<u64, Page>,
    freed: BTreeSet<u64>,
}

/// Transactional 4 KiB pager backed by a main store and a redo write-ahead log.
#[derive(Debug)]
pub struct Pager<S: Storage> {
    main: S,
    log: S,
    pager_id: u64,
    salt: u64,
    log_len: u64,
    kept_frames: u64,
    kept_map: BTreeMap<u64, u64>,
    page_frames: BTreeMap<u64, Vec<u64>>,
    committed: Page0Meta,
    tx: Option<WriteTx>,
    active_snapshots: BTreeMap<u64, SnapshotState>,
    next_snap_id: u64,
    poisoned: bool,
}

impl<S: Storage> Pager<S> {
    /// Open or initialize a database on `main` and `log`, replaying any valid log frames.
    pub fn open(mut main: S, mut log: S) -> Result<Self, Error> {
        let state = recover_or_init(&mut main, &mut log)?;
        Ok(Self {
            main,
            log,
            pager_id: NEXT_PAGER_ID.fetch_add(1, Ordering::Relaxed),
            salt: state.salt,
            log_len: state.log_len,
            kept_frames: state.kept_frames,
            kept_map: state.kept_map,
            page_frames: state.page_frames,
            committed: state.committed,
            tx: None,
            active_snapshots: BTreeMap::new(),
            next_snap_id: 1,
            poisoned: false,
        })
    }

    /// Return the database page count visible to the current transaction or latest commit.
    pub fn page_count(&self) -> u64 {
        self.tx
            .as_ref()
            .map_or(self.committed.page_count, |t| t.page_count)
    }

    /// Return the catalog root page number visible to the current transaction or latest commit.
    pub fn root(&self) -> u64 {
        self.tx.as_ref().map_or(self.committed.root, |t| t.root)
    }

    /// Begin a new write transaction; returns `Error::Busy` if one is already open.
    pub fn begin(&mut self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        if self.tx.is_some() {
            return Err(Error::Busy);
        }
        self.tx = Some(WriteTx {
            page_count: self.committed.page_count,
            free_head: self.committed.free_head,
            root: self.committed.root,
            dirty: BTreeMap::new(),
            freed: BTreeSet::new(),
        });
        Ok(())
    }

    /// Read page `n` from the open transaction if any, else the latest committed state.
    pub fn read(&mut self, n: u64) -> Result<Page, Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        if n >= self.page_count() {
            return Err(Error::Misuse);
        }
        if let Some(tx) = &self.tx {
            if n == 0 {
                // Page 0 as this transaction would commit it (counter bumps only at commit).
                return Ok(encode_page0(Page0Meta {
                    page_count: tx.page_count,
                    free_head: tx.free_head,
                    root: tx.root,
                    change_counter: self.committed.change_counter,
                }));
            }
            if let Some(page) = tx.dirty.get(&n) {
                return Ok(*page);
            }
        }
        self.read_committed_page(n)
    }

    /// Stage a modified page `n` in the open write transaction, stamping its CRC-32.
    pub fn write(&mut self, n: u64, page: &Page) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        let tx = self.tx.as_mut().ok_or(Error::Misuse)?;
        if n == 0 || n >= tx.page_count {
            return Err(Error::Misuse);
        }
        let mut copy = *page;
        stamp_page_crc(&mut copy);
        tx.dirty.insert(n, copy);
        Ok(())
    }

    /// Allocate a page from the free list or by extending `page_count`, initialized to zeros.
    pub fn alloc(&mut self) -> Result<u64, Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        let (free_head, page_count) = {
            let tx = self.tx.as_ref().ok_or(Error::Misuse)?;
            (tx.free_head, tx.page_count)
        };
        if free_head != 0 {
            if free_head >= page_count {
                return Err(Error::Corrupt);
            }
            let page = if let Some(p) = self.tx.as_ref().and_then(|t| t.dirty.get(&free_head)) {
                *p
            } else {
                self.read_committed_page(free_head)?
            };
            let next_free = decode_free_page(&page)?;
            let tx = self.tx.as_mut().ok_or(Error::Misuse)?;
            tx.free_head = next_free;
            tx.freed.remove(&free_head);
            tx.dirty.insert(free_head, encode_zero_page());
            Ok(free_head)
        } else {
            let tx = self.tx.as_mut().ok_or(Error::Misuse)?;
            let new_page = tx.page_count;
            tx.page_count = tx.page_count.checked_add(1).ok_or(Error::TooLarge)?;
            tx.dirty.insert(new_page, encode_zero_page());
            Ok(new_page)
        }
    }

    /// Free page `n`, turning it into a free-list node at the head of the free list.
    pub fn free(&mut self, n: u64) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        let tx = self.tx.as_mut().ok_or(Error::Misuse)?;
        if n == 0 || n >= tx.page_count {
            return Err(Error::Misuse);
        }
        if tx.freed.contains(&n) || tx.free_head == n {
            return Err(Error::Misuse);
        }
        let free_page = encode_free_page(tx.free_head);
        tx.dirty.insert(n, free_page);
        tx.free_head = n;
        tx.freed.insert(n);
        Ok(())
    }

    /// Update the catalog root page number in the open write transaction.
    pub fn set_root(&mut self, root: u64) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        let tx = self.tx.as_mut().ok_or(Error::Misuse)?;
        tx.root = root;
        Ok(())
    }

    /// Commit the open write transaction to the WAL and sync; auto-checkpoints past 1000 frames.
    pub fn commit(&mut self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        let tx = self.tx.take().ok_or(Error::Misuse)?;
        let new_meta = Page0Meta {
            page_count: tx.page_count,
            free_head: tx.free_head,
            root: tx.root,
            change_counter: self.committed.change_counter.wrapping_add(1),
        };
        let page0 = encode_page0(new_meta);

        let start_off = frame_offset(self.kept_frames).ok_or(Error::TooLarge)?;
        if self.log_len > start_off {
            if let Err(e) = self.log.set_len(start_off) {
                self.poisoned = true;
                return Err(e);
            }
            self.log_len = start_off;
        }

        let mut written = 0u64;
        let mut frame_indices: Vec<(u64, u64)> = Vec::with_capacity(tx.dirty.len() + 1);
        for (&page_no, page) in &tx.dirty {
            let idx = self
                .kept_frames
                .checked_add(written)
                .ok_or(Error::TooLarge)?;
            let off = frame_offset(idx).ok_or(Error::TooLarge)?;
            let frame = encode_wal_frame(
                FrameMeta {
                    page_no,
                    commit: 0,
                    salt: self.salt,
                },
                page,
            );
            if let Err(e) = self.log.write_at(off, &frame) {
                self.poisoned = true;
                return Err(e);
            }
            frame_indices.push((page_no, idx));
            written += 1;
        }

        let commit_idx = self
            .kept_frames
            .checked_add(written)
            .ok_or(Error::TooLarge)?;
        let commit_off = frame_offset(commit_idx).ok_or(Error::TooLarge)?;
        let commit_frame = encode_wal_frame(
            FrameMeta {
                page_no: 0,
                commit: tx.page_count,
                salt: self.salt,
            },
            &page0,
        );
        if let Err(e) = self.log.write_at(commit_off, &commit_frame) {
            self.poisoned = true;
            return Err(e);
        }
        frame_indices.push((0, commit_idx));
        written += 1;

        self.log_len = commit_off
            .checked_add(WAL_FRAME_SIZE as u64)
            .ok_or(Error::TooLarge)?;
        if let Err(e) = self.log.sync() {
            self.poisoned = true;
            return Err(e);
        }

        for (page_no, idx) in frame_indices {
            self.kept_map.insert(page_no, idx);
            self.page_frames.entry(page_no).or_default().push(idx);
        }
        self.kept_frames = self
            .kept_frames
            .checked_add(written)
            .ok_or(Error::TooLarge)?;
        self.committed = new_meta;

        if self.kept_frames > AUTO_CHECKPOINT_FRAMES && self.active_snapshots.is_empty() {
            self.do_checkpoint()?;
        }
        Ok(())
    }

    /// Discard the open write transaction without touching storage.
    pub fn rollback(&mut self) {
        self.tx = None;
    }

    /// Capture a read snapshot of the currently committed log state.
    pub fn snapshot(&mut self) -> Snapshot {
        let id = self.next_snap_id;
        self.next_snap_id = self.next_snap_id.wrapping_add(1);
        self.active_snapshots.insert(
            id,
            SnapshotState {
                mark: self.kept_frames,
                page_count: self.committed.page_count,
            },
        );
        Snapshot {
            pager_id: self.pager_id,
            id,
        }
    }

    /// Read page `n` as of the point when `snap` was captured.
    pub fn read_snapshot(&mut self, snap: Snapshot, n: u64) -> Result<Page, Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        if snap.pager_id != self.pager_id {
            return Err(Error::Misuse);
        }
        let state = *self.active_snapshots.get(&snap.id).ok_or(Error::Misuse)?;
        if n >= state.page_count {
            return Err(Error::Misuse);
        }
        if let Some(frames) = self.page_frames.get(&n) {
            let pos = frames.partition_point(|&idx| idx < state.mark);
            if pos > 0 {
                if let Some(&idx) = frames.get(pos - 1) {
                    return read_log_frame_page(&mut self.log, idx);
                }
            }
        }
        read_main_page(&mut self.main, n)
    }

    /// Release an active snapshot so future checkpoints can reclaim log frames.
    pub fn release(&mut self, snap: Snapshot) {
        if snap.pager_id == self.pager_id {
            self.active_snapshots.remove(&snap.id);
        }
    }

    /// Checkpoint kept WAL frames into `main` unless a snapshot is held; returns whether it ran.
    pub fn checkpoint(&mut self) -> Result<bool, Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        if !self.active_snapshots.is_empty() {
            return Ok(false);
        }
        self.do_checkpoint()?;
        Ok(true)
    }

    /// Return the number of committed frames currently kept in the WAL.
    pub fn log_frames(&self) -> u64 {
        self.kept_frames
    }

    /// Roll back any open transaction, checkpoint the WAL, and return `(main, log)`.
    pub fn close(mut self) -> Result<(S, S), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        self.rollback();
        self.do_checkpoint()?;
        Ok((self.main, self.log))
    }

    /// Consume the pager and return `(main, log)` without performing any I/O.
    pub fn into_storage(self) -> (S, S) {
        (self.main, self.log)
    }

    pub(crate) fn ensure_tx(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        if self.tx.is_none() {
            return Err(Error::Misuse);
        }
        Ok(())
    }

    pub(crate) fn view_page_count(&self, at: Option<Snapshot>) -> Result<u64, Error> {
        if self.poisoned {
            return Err(Error::Io);
        }
        match at {
            None => Ok(self.page_count()),
            Some(snap) => {
                if snap.pager_id != self.pager_id {
                    return Err(Error::Misuse);
                }
                let state = self.active_snapshots.get(&snap.id).ok_or(Error::Misuse)?;
                Ok(state.page_count)
            }
        }
    }

    pub(crate) fn read_view(&mut self, at: Option<Snapshot>, n: u64) -> Result<Page, Error> {
        match at {
            None => self.read(n),
            Some(snap) => self.read_snapshot(snap, n),
        }
    }

    fn read_committed_page(&mut self, n: u64) -> Result<Page, Error> {
        if let Some(&idx) = self.kept_map.get(&n) {
            read_log_frame_page(&mut self.log, idx)
        } else {
            read_main_page(&mut self.main, n)
        }
    }

    fn do_checkpoint(&mut self) -> Result<(), Error> {
        if let Err(e) = checkpoint_wal(
            &mut self.main,
            &mut self.log,
            &self.kept_map,
            &mut self.salt,
            &mut self.log_len,
        ) {
            self.poisoned = true;
            return Err(e);
        }
        self.kept_frames = 0;
        self.kept_map.clear();
        self.page_frames.clear();
        Ok(())
    }
}
