// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! One-way entry into Ring 3 from the kernel.
//!
//! The function builds a fake interrupt stack frame and executes
//! `iretq`, which atomically loads SS, RSP, RFLAGS, CS and RIP.
//! Because every segment selector has its RPL forced to `3` via
//! `selector | 3`, the CPU resumes execution in user mode and
//! never returns to this function.

use x86_64::VirtAddr;
use core::arch::asm;

/// Performs a one-way jump to Privilege Level 3 (User Space).
///
/// This function fakes an interrupt stack frame and executes
/// `iretq`. The pushed frame has the order SS, RSP, RFLAGS, CS, RIP
/// required by the architecture; selectors are forced to Ring 3 via
/// `selector | 3`.
pub unsafe fn jump_to_user_mode(entry_point: VirtAddr, user_stack: VirtAddr) -> ! {
    let user_data = crate::core::gdt::GDT.1.user_data_selector.0 | 3;
    let user_code = crate::core::gdt::GDT.1.user_code_selector.0 | 3;

    // RFLAGS with interrupts enabled (IF=1).
    let rflags: u64 = 0x202;

    asm!(
        "mov ds, cx",
        "mov es, cx",
        "mov fs, cx",
        "mov gs, cx",
        "push rcx",      // SS
        "push rsi",      // RSP
        "push rdx",      // RFLAGS
        "push r8",       // CS  -- r8 instead of the reserved rbx
        "push r9",       // RIP -- r9 instead of the reserved rbx
        "iretq",
        in("rcx") user_data as u64,
        in("rsi") user_stack.as_u64(),
        in("rdx") rflags,
        in("r8") user_code as u64,
        in("r9") entry_point.as_u64(),
        options(noreturn)
    );
}