//! Kernel process manager integrating `titan::proc::Table`, `kairos::Scheduler`, and Sv39.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use core::fmt::Write;
use core::ptr::read_volatile;

use hades::Fs;
use kairos::{Class, Config as SchedConfig, Decision, Scheduler, TaskId};
use midas::Locked;
use titan::elf;
use titan::frames::PAGE_SIZE;
use titan::proc::{Pid, ProcState, Table, WaitStatus};
use titan::sv39::{FrameAlloc, PageTable, Pte};
use titan::uaccess::{Errno, USER_MIN};

use crate::blk::Blk;
use crate::vfs::FdTable;
use crate::vm::{self, CLINT_BASE, FRAMES, USER_STACK_BOTTOM, USER_STACK_PAGES, USER_STACK_TOP};
use crate::{finisher_exit, TrapFrame, Uart, FINISHER_FAIL, FINISHER_PASS};

const CLINT_MTIME: u64 = CLINT_BASE + 0xBFF8;
const KSTACK_WORDS: usize = 1024; // 16 KiB, 16-byte aligned
const SSTATUS_SPIE: u64 = 1 << 5;

pub static KERNEL: Locked<Option<KernelState>> = Locked::new(None);

pub fn read_mtime() -> u64 {
    // SAFETY: `CLINT_MTIME` (`0x0200_BFF8`) is mapped MMIO in kernel and user page tables (`U = 0`).
    unsafe { read_volatile(CLINT_MTIME as *const u64) }
}

pub struct KernelProc {
    pub tid: TaskId,
    pub pt_root: u64,
    pub user_pages: BTreeMap<u64, u64>,
    pub heap_base: u64,
    pub brk: u64,
    _kstack: Box<[u128]>,
    pub tf_addr: u64,
    pub frames_freed: bool,
    pub fds: FdTable,
}

pub struct KernelState {
    pub table: Table,
    pub sched: Scheduler,
    pub procs: BTreeMap<Pid, KernelProc>,
    pub tid_to_pid: BTreeMap<TaskId, Pid>,
    pub current: Option<Pid>,
    pub initial_free_frames: usize,
    pub fs: Option<Fs<Blk>>,
}

impl KernelState {
    pub fn new(initial_free_frames: usize, fs: Option<Fs<Blk>>) -> Self {
        Self {
            table: Table::new(),
            sched: Scheduler::new(1, SchedConfig::default()),
            procs: BTreeMap::new(),
            tid_to_pid: BTreeMap::new(),
            current: None,
            initial_free_frames,
            fs,
        }
    }

    pub fn spawn_process(
        &mut self,
        parent: Option<Pid>,
        name: &str,
        elf_bytes: &[u8],
    ) -> Result<Pid, Errno> {
        let prog = elf::parse(elf_bytes).map_err(|_| Errno::BadArg)?;
        let pt_root = vm::create_user_pt().ok_or(Errno::NoMem)?;
        let mut user_pages: BTreeMap<u64, u64> = BTreeMap::new();
        let mut max_seg_end = USER_MIN;

        for seg in &prog.segments {
            if seg.mem_size == 0 {
                continue;
            }
            let seg_end = seg.vaddr + seg.mem_size;
            if seg_end > max_seg_end {
                max_seg_end = seg_end;
            }
            if !seg.r && !seg.x {
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(Errno::BadArg);
            }
            if seg.w && !seg.r {
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(Errno::BadArg);
            }
            let mut flags = Pte::V | Pte::U | Pte::A | Pte::D;
            if seg.r {
                flags |= Pte::R;
            }
            if seg.w {
                flags |= Pte::W;
            }
            if seg.x {
                flags |= Pte::X;
            }

            let file_slice = &elf_bytes[seg.file.clone()];
            let page_start = seg.vaddr & !(PAGE_SIZE - 1);
            let page_limit = (seg_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let mut page_va = page_start;
            while page_va < page_limit {
                let pa = if let Some(&existing_pa) = user_pages.get(&page_va) {
                    existing_pa
                } else {
                    let mut frames_guard = FRAMES.lock();
                    let mut pt = PageTable::from_root(pt_root, &mut *frames_guard);
                    if pt.translate(page_va).is_some() {
                        drop(frames_guard);
                        vm::destroy_user_pt(pt_root, &user_pages);
                        return Err(Errno::NoMem);
                    }
                    let Some(new_pa) = pt.allocator_mut().alloc() else {
                        drop(frames_guard);
                        vm::destroy_user_pt(pt_root, &user_pages);
                        return Err(Errno::NoMem);
                    };
                    vm::zero_frame(new_pa);
                    if pt.map(page_va, new_pa, flags).is_err() {
                        let _ = pt.allocator_mut().free(new_pa);
                        drop(frames_guard);
                        vm::destroy_user_pt(pt_root, &user_pages);
                        return Err(Errno::NoMem);
                    }
                    user_pages.insert(page_va, new_pa);
                    new_pa
                };

                let file_va_start = seg.vaddr;
                let file_va_end = seg.vaddr + (file_slice.len() as u64);
                let ov_start = page_va.max(file_va_start);
                let ov_end = (page_va + PAGE_SIZE).min(file_va_end);
                if ov_start < ov_end {
                    let dst_off = (ov_start - page_va) as usize;
                    let src_start = (ov_start - file_va_start) as usize;
                    let src_end = (ov_end - file_va_start) as usize;
                    vm::copy_into_frame(pa, dst_off, &file_slice[src_start..src_end]);
                }

                page_va += PAGE_SIZE;
            }
        }

        // Map 16 KiB user stack ending at USER_STACK_TOP (0x3F_FFFF_F000).
        let stack_flags = Pte::V | Pte::U | Pte::R | Pte::W | Pte::A | Pte::D;
        for i in 0..USER_STACK_PAGES {
            let va = USER_STACK_BOTTOM + i * PAGE_SIZE;
            if user_pages.contains_key(&va) {
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(Errno::BadArg);
            }
            let mut frames_guard = FRAMES.lock();
            let mut pt = PageTable::from_root(pt_root, &mut *frames_guard);
            let Some(pa) = pt.allocator_mut().alloc() else {
                drop(frames_guard);
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(Errno::NoMem);
            };
            vm::zero_frame(pa);
            if pt.map(va, pa, stack_flags).is_err() {
                let _ = pt.allocator_mut().free(pa);
                drop(frames_guard);
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(Errno::NoMem);
            }
            user_pages.insert(va, pa);
        }

        let heap_base = (max_seg_end.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
        let heap_base = heap_base.max(USER_MIN);

        let kstack = vec![0u128; KSTACK_WORDS].into_boxed_slice();
        let kstack_top = (kstack.as_ptr() as u64) + ((KSTACK_WORDS * 16) as u64);
        let tf_addr = kstack_top - (core::mem::size_of::<TrapFrame>() as u64);
        let tf_ptr = tf_addr as *mut TrapFrame;
        // SAFETY: `tf_ptr` points to the zeroed top 288 bytes of `kstack`, aligned to 16 bytes.
        unsafe {
            (*tf_ptr).regs[2] = USER_STACK_TOP;
            (*tf_ptr).sstatus = SSTATUS_SPIE;
            (*tf_ptr).sepc = prog.entry;
        }

        let pid = match self.table.spawn(parent, name) {
            Ok(p) => p,
            Err(e) => {
                vm::destroy_user_pt(pt_root, &user_pages);
                return Err(e);
            }
        };

        let Ok(tid) = self.sched.spawn(Class::Normal { nice: 0 }, 0, read_mtime()) else {
            let _ = self.table.exit(pid, -1);
            if let Some(ppid) = parent {
                let _ = self.table.wait(ppid, pid);
            }
            vm::destroy_user_pt(pt_root, &user_pages);
            return Err(Errno::NoMem);
        };

        self.tid_to_pid.insert(tid, pid);
        self.procs.insert(
            pid,
            KernelProc {
                tid,
                pt_root,
                user_pages,
                heap_base,
                brk: heap_base,
                _kstack: kstack,
                tf_addr,
                frames_freed: false,
                fds: FdTable::new(),
            },
        );
        Ok(pid)
    }

    pub fn schedule_next(&mut self) -> *mut TrapFrame {
        let now = read_mtime();
        if let Some(old_pid) = self.current {
            if self.table.state(old_pid) == Some(ProcState::Running) {
                let _ = self.table.set_runnable(old_pid);
            }
        }
        let Decision { task, .. } = self.sched.pick(0, now).unwrap_or(Decision {
            task: None,
            slice_us: 0,
        });
        let Some(next_tid) = task else {
            crate::fail("no runnable process");
        };
        let Some(&next_pid) = self.tid_to_pid.get(&next_tid) else {
            crate::fail("unknown task id");
        };
        if self.table.state(next_pid) == Some(ProcState::Runnable) {
            let _ = self.table.set_running(next_pid);
        }
        let Some(p) = self.procs.get(&next_pid) else {
            crate::fail("missing proc");
        };
        if self.current != Some(next_pid) {
            vm::write_satp(vm::satp_for_root(p.pt_root));
            self.current = Some(next_pid);
        }
        p.tf_addr as *mut TrapFrame
    }

    pub fn exit_current(&mut self, code: i32) -> *mut TrapFrame {
        let Some(pid) = self.current else {
            crate::fail("exit without current proc");
        };

        if pid == 1 {
            vm::switch_to_kernel_pt();
            if let Some(fs) = self.fs.take() {
                let _ = fs.unmount();
            }
            for p in self.procs.values_mut() {
                p.fds.clear();
                if !p.frames_freed {
                    vm::destroy_user_pt(p.pt_root, &p.user_pages);
                    p.user_pages.clear();
                    p.frames_freed = true;
                }
            }
            let free_now = vm::free_frame_count();
            let _ = writeln!(Uart, "init exited {code}");
            let _ = writeln!(Uart, "frames {free_now} free");
            let status = if code == 0 && free_now == self.initial_free_frames {
                FINISHER_PASS
            } else {
                FINISHER_FAIL
            };
            finisher_exit(status);
        }

        vm::switch_to_kernel_pt();
        if let Some(p) = self.procs.get_mut(&pid) {
            p.fds.clear();
            if !p.frames_freed {
                vm::destroy_user_pt(p.pt_root, &p.user_pages);
                p.user_pages.clear();
                p.frames_freed = true;
            }
            let _ = self.sched.exit(p.tid, read_mtime());
        }
        if let Ok(Some(ppid)) = self.table.exit(pid, code) {
            if let Some(parent) = self.procs.get(&ppid) {
                let _ = self.sched.wake(parent.tid, read_mtime());
            }
        }
        self.current = None;
        self.schedule_next()
    }

    pub fn kill_child(&mut self, caller_pid: Pid, target_pid: Pid) -> Result<(), Errno> {
        let woke = self.table.kill_child(caller_pid, target_pid)?;
        if let Some(p) = self.procs.get_mut(&target_pid) {
            p.fds.clear();
            if !p.frames_freed {
                vm::destroy_user_pt(p.pt_root, &p.user_pages);
                p.user_pages.clear();
                p.frames_freed = true;
            }
            let _ = self.sched.exit(p.tid, read_mtime());
        }
        if let Some(ppid) = woke {
            if let Some(parent) = self.procs.get(&ppid) {
                let _ = self.sched.wake(parent.tid, read_mtime());
            }
        }
        Ok(())
    }

    pub fn wait_child(&mut self, parent_pid: Pid, child_pid: Pid) -> Result<WaitStatus, Errno> {
        match self.table.wait(parent_pid, child_pid)? {
            WaitStatus::Exited(code) => {
                if let Some(mut old) = self.procs.remove(&child_pid) {
                    self.tid_to_pid.remove(&old.tid);
                    if !old.frames_freed {
                        vm::destroy_user_pt(old.pt_root, &old.user_pages);
                        old.user_pages.clear();
                        old.frames_freed = true;
                    }
                }
                Ok(WaitStatus::Exited(code))
            }
            WaitStatus::WouldBlock => {
                if let Some(p) = self.procs.get(&parent_pid) {
                    let _ = self.sched.block(p.tid, read_mtime());
                }
                Ok(WaitStatus::WouldBlock)
            }
        }
    }

    pub fn sbrk(&mut self, pid: Pid, delta: i64) -> Result<u64, Errno> {
        let p = self.procs.get_mut(&pid).ok_or(Errno::BadArg)?;
        let old_brk = p.brk;
        if delta == 0 {
            return Ok(old_brk);
        }

        let new_brk = if delta > 0 {
            old_brk.checked_add(delta as u64).ok_or(Errno::NoMem)?
        } else {
            let sub = delta.unsigned_abs();
            let b = old_brk.checked_sub(sub).ok_or(Errno::BadArg)?;
            if b < p.heap_base {
                return Err(Errno::BadArg);
            }
            b
        };

        let old_top = (old_brk.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
        let new_top = (new_brk.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);

        if new_top > old_top {
            if new_top > USER_STACK_BOTTOM - PAGE_SIZE {
                return Err(Errno::NoMem);
            }
            let flags = Pte::V | Pte::U | Pte::R | Pte::W | Pte::A | Pte::D;
            let mut mapped_here = alloc::vec::Vec::new();
            let mut frames_guard = FRAMES.lock();
            let mut pt = PageTable::from_root(p.pt_root, &mut *frames_guard);
            let mut va = old_top;
            while va < new_top {
                if p.user_pages.contains_key(&va) || pt.translate(va).is_some() {
                    for &(m_va, m_pa) in &mapped_here {
                        let _ = pt.unmap(m_va);
                        let _ = pt.allocator_mut().free(m_pa);
                    }
                    vm::sfence_vma();
                    return Err(Errno::NoMem);
                }
                let Some(pa) = pt.allocator_mut().alloc() else {
                    for &(m_va, m_pa) in &mapped_here {
                        let _ = pt.unmap(m_va);
                        let _ = pt.allocator_mut().free(m_pa);
                    }
                    vm::sfence_vma();
                    return Err(Errno::NoMem);
                };
                vm::zero_frame(pa);
                if pt.map(va, pa, flags).is_err() {
                    let _ = pt.allocator_mut().free(pa);
                    for &(m_va, m_pa) in &mapped_here {
                        let _ = pt.unmap(m_va);
                        let _ = pt.allocator_mut().free(m_pa);
                    }
                    vm::sfence_vma();
                    return Err(Errno::NoMem);
                }
                mapped_here.push((va, pa));
                va += PAGE_SIZE;
            }
            for (m_va, m_pa) in mapped_here {
                p.user_pages.insert(m_va, m_pa);
            }
            vm::sfence_vma();
        } else if new_top < old_top {
            let mut frames_guard = FRAMES.lock();
            let mut pt = PageTable::from_root(p.pt_root, &mut *frames_guard);
            let mut va = new_top;
            while va < old_top {
                if let Some(pa) = p.user_pages.remove(&va) {
                    let _ = pt.unmap(va);
                    let _ = pt.allocator_mut().free(pa);
                }
                va += PAGE_SIZE;
            }
            vm::sfence_vma();
        }

        p.brk = new_brk;
        Ok(old_brk)
    }
}
