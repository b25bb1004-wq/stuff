//! The raw `ecall` gate: number in `a7`, arguments in `a0..a5`, result in `a0`.
use core::arch::asm;

/// Traps into the kernel with system call `n` and returns its signed result.
pub fn call(n: u64, args: [u64; 6]) -> isize {
    let [mut a0, a1, a2, a3, a4, a5] = args;
    // SAFETY: `ecall` is the documented system call gate. The kernel validates every pointer and
    // length argument before touching user memory, and returns the result in `a0` only.
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0,
            inlateout("a1") a1 => _,
            inlateout("a2") a2 => _,
            inlateout("a3") a3 => _,
            inlateout("a4") a4 => _,
            inlateout("a5") a5 => _,
            in("a7") n,
            options(nostack),
        );
    }
    a0 as isize
}
