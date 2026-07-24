// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Round-robin preemptive scheduler driven by the PIT timer tick.
//!
//! Every tick, [`schedule`] (installed as the IRQ0 handler) picks
//! the next [`TaskId`] from the ready queue, updates the TSS.RSP0
//! (kernel stack used on Ring 3 interrupts), switches CR3 if the
//! next task lives in a different address space, and finally jumps
//! into the new task via [`context_switch`].

use core::sync::atomic::Ordering;
use crate::task::{context_switch, PrivilegeLevel, TaskId, TaskState, TASK_MANAGER};
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags}; // Required for the Ring 3 switch

/// Saved kernel RSP of the boot context, used as the "outgoing"
/// pointer the very first time a non-Boot task is dispatched.
static mut BOOT_RSP: u64 = 0;

/// Picks the next task from the ready queue and switches to it.
///
/// Invoked from the PIT interrupt handler. The path is:
/// 1. tick counter increment, re-enqueue current task if it is still
///    runnable.
/// 2. fetch next task; load its PML4 into CR3 if it changed.
/// 3. update TSS.RSP0 for the user-space interrupt trampoline.
/// 4. update [`crate::core::syscall::KERNEL_RSP`] for syscall entry.
/// 5. perform the [`context_switch`].
pub unsafe fn schedule() {
    let mut switch_data: Option<(*mut u64, u64)> = None;
    let mut next_cr3 = None; // next task's address space

    {
        let mut manager = TASK_MANAGER.lock();
        manager.ticks.fetch_add(1, Ordering::Relaxed);

        let current_id_opt = manager.current_task;

        if let Some(current_id) = current_id_opt {
            let state = manager.task_states.get(&current_id).copied().unwrap_or(TaskState::Ready);
            if state == TaskState::Running || state == TaskState::Ready {
                manager.mark_ready(current_id);
            }
        }

        if let Some(next_task_id) = manager.fetch_next_task() {
            if Some(next_task_id) != current_id_opt {
                manager.current_task = Some(next_task_id);

                if let Some(current_id) = current_id_opt {
                    if let Some(task) = manager.task_registry.get_mut(&current_id) {
                        let old_ptr = &mut task.rsp as *mut u64;
                        if let Some(next_task) = manager.task_registry.get(&next_task_id) {
                            if next_task.privilege == PrivilegeLevel::UserMode {
                                crate::core::gdt::set_tss_rsp0(VirtAddr::new(next_task.stack_end));
                            }
                            next_cr3 = Some(next_task.pml4_frame);
                            switch_data = Some((old_ptr, next_task.rsp));
                        }
                    }
                } else {
                    if let Some(next_task) = manager.task_registry.get(&next_task_id) {
                        if next_task.privilege == PrivilegeLevel::UserMode {
                            crate::core::gdt::set_tss_rsp0(VirtAddr::new(next_task.stack_end));
                        }
                        next_cr3 = Some(next_task.pml4_frame);
                        switch_data = Some((core::ptr::addr_of_mut!(BOOT_RSP), next_task.rsp));
                    }
                }
            }
        }
    }
    // Notify the hardware that we received the tick
    crate::core::idt::PICS.lock().notify_end_of_interrupt(crate::core::idt::TIMER_INTERRUPT);

    // Final setup before the jump

    // Update the safe stack for syscalls
    let current_task_id = crate::task::TASK_MANAGER.lock().current_task;

    if let Some(next_task_id) = current_task_id {
        if let Some(task) = crate::task::TASK_MANAGER.lock().task_registry.get(&next_task_id) {
            crate::core::syscall::KERNEL_RSP = task.stack_end;
        }
    }

    // Switch address space (CR3) only if it actually changed
    if let Some(pml4) = next_cr3 {
        let (current_cr3, _) = Cr3::read();
        if current_cr3 != pml4 {
            Cr3::write(pml4, Cr3Flags::empty());
        }
    }

    // Jump to the new task
    if let Some((old_rsp_ptr, new_rsp)) = switch_data {
        context_switch(old_rsp_ptr, new_rsp);
    }
}

/// Snapshot of scheduler-relevant metrics (degenerate view
/// over [`TaskManagerStats`]).
pub fn get_stats() -> SchedulerStats {
    let manager = TASK_MANAGER.lock();
    let tm_stats = manager.stats();
    SchedulerStats {
        total_tasks: tm_stats.total_tasks,
        ready_tasks: tm_stats.ready_tasks,
        current_task: tm_stats.current_task,
        ticks: tm_stats.ticks,
    }
}

/// Lightweight scheduler view of the task manager metrics.
#[derive(Debug, Clone, Copy)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub ready_tasks: usize,
    pub current_task: Option<TaskId>,
    pub ticks: u64,
}