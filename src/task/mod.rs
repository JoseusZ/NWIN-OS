// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multitasking layer: the per-task [`Task`] model, the
//! [`TaskManager`] that owns every runnable task, the round-robin
//! [`scheduler`], the ELF loader ([`elf`]) and the Ring 3 entry
//! helper ([`usermode`]).
//!
//! The public surface is small on purpose: the rest of the kernel
//! interacts with the subsystem through [`init_multitasking`] (boot)
//! and [`with_task_manager`] (everywhere else).

pub mod task;
pub mod task_manager;
pub mod scheduler;
pub mod usermode;
pub mod elf;

pub use task::{context_switch, PrivilegeLevel, Task, TaskContext, TaskId};
pub use task_manager::{init_task_manager, with_task_manager, TaskManager, TaskManagerStats, TaskState, TASK_MANAGER};

/// Boots the multitasking subsystem: initialises the
/// [`TaskManager`], spawns the kernel reaper daemon and, if a
/// `shell.elf` is present in the initramfs, deploys it as a
/// Ring 3 user task.
pub fn init_multitasking() {
    let (current_pml4, _) = x86_64::registers::control::Cr3::read();
    crate::task::init_task_manager();

    // TASK 1: THE REAPER THREAD (DAEMON, RING 0)
    fn hilo_segador() -> ! {
        use alloc::vec::Vec;
        crate::serial_println!("[DAEMON] Reaper thread online. Sleeping...");

        loop {
            x86_64::instructions::interrupts::enable_and_hlt();

            let tareas_muertas = crate::task::with_task_manager(|tm| {
                let mut muertas = Vec::new();
                for (&id, &estado) in tm.task_states.iter() {
                    if estado == crate::task::TaskState::Dead {
                        muertas.push(id);
                    }
                }
                muertas
            });

            for id in tareas_muertas {
                let tarea_purgada = crate::task::with_task_manager(|tm| tm.kill(id));
                if let Some(tarea_muerta) = tarea_purgada {
                    if tarea_muerta.privilege == crate::task::PrivilegeLevel::UserMode {
                        unsafe { crate::mm::memory::destroy_user_address_space(tarea_muerta.pml4_frame); }
                    }
                }
            }
        }
    }

    // CENTRAL INITIALISATION (THE BIG BANG OF TASKS)
    //
    // Phase 7: the four panic branches use Display `{}` instead of
    // Debug `{:?}` to stay consistent with Phase 5 (panic_handler) and
    // so that the KernelError prints with its readable field format
    // (`KERNEL:TASK:StackAllocation (64 KiB heap reservation failed)`)
    // rather than the raw Rust dump.
    crate::task::with_task_manager(|tm| {
        if let Err(e) = tm.spawn(hilo_segador, current_pml4, crate::task::PrivilegeLevel::KernelMode) {
            panic!("FATAL: Failed to spawn Reaper daemon. Error: {}", e);
        }

        let nombre_archivo = "shell.elf";
        if let Some(elf_slice) = crate::fs::vfs::find_file(nombre_archivo) {
            if let Some(target_pml4) = crate::mm::memory::create_isolated_pml4() {

                // Phase 7: load_elf still returns ElfError; we use
                // `.into()` (which activates From<ElfError> for KernelError,
                // Phase 2.1) instead of re-wrapping it manually with
                // SystemError::ElfParseFailed. The result in rax / log is
                // equivalent (kernel_err -> to_errno produces -ENOEXEC),
                // but the code is cleaner and reuses the consolidated
                // taxonomy: the ElfError is now channeled as FsError::Elf
                // (the canonical wrapper since Phase 1.5).
                let load_result: Result<u64, crate::core::error::KernelError> =
                    crate::task::elf::load_elf(elf_slice, target_pml4)
                        .map_err(|e| e.into());

                let user_stack_base = 0x800000;
                let user_stack_top = user_stack_base + 0x1000;
                let _ = crate::mm::memory::allocate_and_map_user_page(target_pml4, x86_64::VirtAddr::new(user_stack_base));

                match load_result {
                    Ok(entry_point) => {
                        let entry_fn: fn() -> ! = unsafe { ::core::mem::transmute(entry_point as usize) };
                        match tm.spawn_dynamic(entry_fn, target_pml4, user_stack_top) {
                            Ok(_) => crate::serial_println!("[OK] User Shell deployed and queued for Ring 3."),
                            Err(e) => panic!("FATAL: Failed to spawn Shell task. Error: {}", e),
                        }
                    },
                    Err(e) => panic!("FATAL: Failed to parse shell.elf. Error: {}", e),
                }
            } else { panic!("FATAL: Out of memory. Could not isolate PML4 for Shell."); }
        } else { panic!("FATAL: '{}' not found in Initramfs TAR.", nombre_archivo); }
    });
    
    crate::println!("[OK] TaskManager and async Scheduler online.");
    crate::core::idt::reload();
}