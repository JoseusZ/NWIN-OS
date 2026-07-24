// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::fd::FdTable;

/// Per-task data model: identifier, saved-register snapshot, kernel
/// stack, page table, file-descriptor table and the privilege level
/// at which the task is entered.
pub struct Task {
    // Task identity
    pub id:          TaskId,
    pub parent_id:   Option<TaskId>,
    // Saved top of the kernel stack (context_switch reads/writes it)
    pub rsp:         u64,
    // Physical frame that holds the task's PML4
    pub pml4_frame:  x86_64::structures::paging::PhysFrame,
    // Backing memory of the kernel stack
    pub stack_start: u64,
    pub stack_end:   u64,
    _stack:          alloc::boxed::Box<[u8]>,
    // Ring 0 vs Ring 3
    pub privilege:   PrivilegeLevel,
    // User-space memory layout
    pub heap_start:    u64,
    pub program_break: u64,
    pub mmap_base:     u64,
    // Per-task file descriptor table
    pub fd_table: FdTable,
}

/// Process-wide unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl TaskId {
    /// Allocates the next available id from a process-wide atomic counter.
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Privilege level at which a task is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeLevel {
    /// Kernel thread. Full access to memory and hardware (Ring 0).
    KernelMode,
    /// User process. The scheduler enters it in Ring 3 via `iretq`.
    UserMode,
}

/// Caller-saved register snapshot defined by the SysV x86_64 ABI.
///
/// **Field order is critical**: it must match exactly the push/pop
/// sequence in [`context_switch`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TaskContext {
    pub rflags: u64, // RSP+0
    pub rbp:    u64, // RSP+8
    pub rbx:    u64, // RSP+16
    pub r12:    u64, // RSP+24
    pub r13:    u64, // RSP+32
    pub r14:    u64, // RSP+40
    pub r15:    u64, // RSP+48
}

impl Task {
    /// Builds a new [`Task`] with a freshly allocated 64 KiB kernel
    /// stack and a hand-crafted initial stack frame that matches the
    /// target [`PrivilegeLevel`].
    ///
    /// For [`PrivilegeLevel::UserMode`] the frame is laid out so that
    /// an `iretq` jumps into user space with the kernel's chosen SS,
    /// RSP, RFLAGS, CS and RIP. For [`PrivilegeLevel::KernelMode`] a
    /// `0xDEAD_C0DE_DEAD_C0DE` canary is pushed below the entry point
    /// to catch stack underflow during development.
    ///
    /// Returns [`KernelError::Task`] wrapping
    /// [`TaskError::StackAllocation`] if the heap cannot satisfy the
    /// stack reservation.
    pub fn new(
        entry_point: fn() -> !,
        pml4_frame: x86_64::structures::paging::PhysFrame,
        privilege: PrivilegeLevel,
        user_stack_top: u64,
        parent_id: Option<TaskId>,
    ) -> Result<Self, crate::core::error::KernelError> {

        const STACK_SIZE: usize = 65536; // 64 KiB
        let id = TaskId::new();
        let mut stack_vec = alloc::vec::Vec::new();

        // Eradicate the death loop: return a structured error instead.
        //
        // Phase 7: use `TaskError::StackAllocation` (declared in Phase 1.6)
        // instead of the legacy bridge `SystemError::TaskCreationFailure`.
        // The `From<TaskError> for KernelError` conversion (Phase 2.1) is
        // triggered automatically when wrapping.
        if stack_vec.try_reserve_exact(STACK_SIZE).is_err() {
            return Err(crate::core::error::KernelError::Task(
                crate::core::error::TaskError::StackAllocation
            ));
        }

        stack_vec.resize(STACK_SIZE, 0);

        // Hand the buffer to a Box without going through Vec::drop.
        let ptr = stack_vec.as_mut_ptr();
        let len = stack_vec.len();
        core::mem::forget(stack_vec);
        let slice_ptr = core::ptr::slice_from_raw_parts_mut(ptr, len);
        let stack = unsafe { alloc::boxed::Box::from_raw(slice_ptr) };

        let stack_start = stack.as_ptr() as u64;
        let stack_top   = stack_start + STACK_SIZE as u64;
        let mut rsp = stack_top & !0xF;

        unsafe {
            if privilege == PrivilegeLevel::UserMode {

                // **VITAL**: the `| 3` physically forces the CPU to drop to Ring 3.
                let user_data_selector = (crate::core::gdt::GDT.1.user_data_selector.0 as u64) | 3;
                let user_code_selector = (crate::core::gdt::GDT.1.user_code_selector.0 as u64) | 3;

                rsp -= 8; *(rsp as *mut u64) = user_data_selector; // SS
                rsp -= 8; *(rsp as *mut u64) = user_stack_top;      // user RSP
                rsp -= 8; *(rsp as *mut u64) = 0x202;              // RFLAGS (IF=1)
                rsp -= 8; *(rsp as *mut u64) = user_code_selector; // CS
                rsp -= 8; *(rsp as *mut u64) = entry_point as usize as u64; // RIP = ELF entry point

                rsp -= 8; *(rsp as *mut u64) = user_mode_trampoline as *const () as u64;

            } else {
                rsp -= 8; *(rsp as *mut u64) = 0xDEAD_C0DE_DEAD_C0DE;
                rsp -= 8; *(rsp as *mut u64) = entry_point as usize as u64;
            }

            let context_size = core::mem::size_of::<TaskContext>() as u64;
            rsp -= context_size;

            core::ptr::write(rsp as *mut TaskContext, TaskContext {
                rflags: 0x202,
                rbp:    0, rbx:    0, r12:    0,
                r13:    0, r14:    0, r15:    0,
            });
        }

        let heap_start = 0x0000_0002_0000_0000; // 8 GiB virtual
        let mmap_base  = 0x0000_4000_0000_0000; // 64 TiB virtual
        let fd_table = FdTable::new();

        Ok(Self {
            id,
            parent_id,
            rsp, pml4_frame, stack_start,
            stack_end: stack_top, _stack: stack,
            privilege,
            heap_start,
            program_break: heap_start,
            mmap_base,
            fd_table,
        })
    }

    /// Returns `true` iff `rsp` lies inside this task's kernel stack
    /// range. Used by the scheduler to validate resumption points.
    pub fn is_valid_rsp(&self, rsp: u64) -> bool {
        rsp >= self.stack_start && rsp < self.stack_end
    }
}

/// Tiny bridge that consumes the five values pushed on the stack
/// by [`Task::new`] (SS, RSP, RFLAGS, CS, RIP) to force a privilege
/// drop down to Ring 3 via `iretq`.
#[unsafe(naked)]
extern "C" fn user_mode_trampoline() {
    {
        core::arch::naked_asm!(
            "iretq"
        );
    }
}

/// Cooperative / preemptive context switch between two kernel tasks.
///
/// `old_rsp` receives the outgoing task's new RSP; `new_rsp` is the
/// incoming task's saved RSP. Either pointer being null triggers an
/// `int3` + `hlt` via label `2:`.
#[unsafe(naked)]
pub unsafe extern "sysv64" fn context_switch(old_rsp: *mut u64, new_rsp: u64) {
    core::arch::naked_asm!(
        // Guard: null pointers
        "test rdi, rdi",
        "jz 2f",
        "test rsi, rsi",
        "jz 2f",

        // --- Save outgoing task context ---
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        "pushfq",
        // Force IF=1 into the saved RFLAGS
        "or qword ptr [rsp], 0x200",

        // Save outgoing task RSP
        "mov [rdi], rsp",

        // Load incoming task RSP
        "mov rsp, rsi",

        // --- Restore incoming task context ---
        "popfq",
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",

        // ret jumps to entry_point (kernel) or user_mode_trampoline (Ring 3)
        "ret",

        // Error label
        "2:",
        "int3",
        "hlt",
    );
}