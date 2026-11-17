//! Door device driver (Silicon MMIO `0x1000_4000`) and the door calls 22 and 23.
//!
//! Receive buffers are allocated per call and sized to the front frame (at most 1 MiB, the
//! device limit), so `RX_CAP` always equals the frame length. Buffers live in the identity-mapped
//! kernel heap, so their virtual address is the physical address the device expects.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use titan::proc::Pid;
use titan::uaccess::Errno;

use crate::proc::KernelState;
use crate::vm::{self, DOOR_BASE};

const TX_ADDR: u64 = 0x00;
const TX_LEN: u64 = 0x08;
const TX_CHAN: u64 = 0x10;
const TX_GO: u64 = 0x18;
const RX_ADDR: u64 = 0x20;
const RX_CAP: u64 = 0x28;
const STATUS: u64 = 0x30;
const RX_LEN: u64 = 0x38;
const RX_CHAN: u64 = 0x40;
const RX_TAKE: u64 = 0x48;

const STATUS_RX_READY: u64 = 1;
const STATUS_TX_ERR: u64 = 2;

/// Largest frame the device accepts, in bytes.
pub const MAX_FRAME: u64 = 1 << 20;
/// Call result for "busy": the outbox is full, or the inbox is empty.
pub const EBUSY: isize = -7;

fn reg_read(off: u64) -> u64 {
    // SAFETY: `DOOR_BASE + off` is a 64-bit register of the door MMIO page, mapped `U = 0`
    // in every kernel and user page table (`vm::MMIO_RANGES`).
    unsafe { read_volatile((DOOR_BASE + off) as *const u64) }
}

fn reg_write(off: u64, value: u64) {
    // SAFETY: same MMIO page as `reg_read`; writes only the documented door registers.
    unsafe { write_volatile((DOOR_BASE + off) as *mut u64, value) }
}

/// Send `bytes` on `chan`. Returns 0, or `EBUSY` when the device refuses (outbox full).
/// Caller guarantees `1 <= bytes.len() <= MAX_FRAME`.
pub fn send(chan: u8, bytes: &[u8]) -> isize {
    reg_write(TX_ADDR, bytes.as_ptr() as u64);
    reg_write(TX_LEN, bytes.len() as u64);
    reg_write(TX_CHAN, chan as u64);
    reg_write(TX_GO, 1);
    if reg_read(STATUS) & STATUS_TX_ERR != 0 {
        EBUSY
    } else {
        0
    }
}

/// Channel and length of the front inbound frame, or `None` when the inbox is empty.
fn front() -> Option<(u8, u64)> {
    if reg_read(STATUS) & STATUS_RX_READY == 0 {
        return None;
    }
    Some((reg_read(RX_CHAN) as u8, reg_read(RX_LEN)))
}

/// Pop the front frame into `buf`, whose length must equal that frame's length.
fn take(buf: &mut [u8]) {
    reg_write(RX_ADDR, buf.as_mut_ptr() as u64);
    reg_write(RX_CAP, buf.len() as u64);
    reg_write(RX_TAKE, 1);
}

fn has_right(ks: &KernelState, pid: Pid) -> bool {
    ks.procs.get(&pid).is_some_and(|p| p.door)
}

/// Call 22: `door_send(chan, ptr, len)`.
pub fn sys_door_send(
    ks: &KernelState,
    pid: Pid,
    pt_root: u64,
    chan: u64,
    ptr: u64,
    len: u64,
) -> isize {
    if !has_right(ks, pid) || chan > 255 || len == 0 || len > MAX_FRAME {
        return Errno::BadArg.as_isize();
    }
    let mut buf: Vec<u8> = vec![0u8; len as usize];
    if let Err(e) = vm::copy_from_user(pt_root, ptr, &mut buf) {
        return e.as_isize();
    }
    send(chan as u8, &buf)
}

/// Call 23: `door_recv(ptr, cap, chan_out_ptr)`. Non-blocking; `EBUSY` when no frame is queued.
pub fn sys_door_recv(
    ks: &KernelState,
    pid: Pid,
    pt_root: u64,
    ptr: u64,
    cap: u64,
    chan_out: u64,
) -> isize {
    if !has_right(ks, pid) {
        return Errno::BadArg.as_isize();
    }
    let Some((chan, len)) = front() else {
        return EBUSY;
    };
    if len > cap || len > MAX_FRAME {
        return Errno::BadArg.as_isize();
    }
    if let Err(e) = vm::validate_user_write(pt_root, ptr, len) {
        return e.as_isize();
    }
    if let Err(e) = vm::validate_user_write(pt_root, chan_out, 1) {
        return e.as_isize();
    }
    let mut buf: Vec<u8> = vec![0u8; len as usize];
    take(&mut buf);
    if let Err(e) = vm::copy_to_user(pt_root, ptr, &buf) {
        return e.as_isize();
    }
    if let Err(e) = vm::copy_to_user(pt_root, chan_out, &[chan]) {
        return e.as_isize();
    }
    len as isize
}
