// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod task;
pub mod task_manager;
pub mod scheduler;
pub mod usermode;
pub mod elf;

pub use task::{context_switch, PrivilegeLevel, Task, TaskContext, TaskId};
pub use task_manager::{init_task_manager, with_task_manager, TaskManager, TaskManagerStats, TaskState, TASK_MANAGER};

pub fn init_multitasking() {
    let (current_pml4, _) = x86_64::registers::control::Cr3::read();
    crate::task::init_task_manager();
    
    // TAREA 1: EL HILO DEL SEGADOR (DAEMON RING 0)
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

    // INICIALIZACIÓN CENTRAL (EL BIG BANG DE LAS TAREAS)
    crate::task::with_task_manager(|tm| {
        if let Err(e) = tm.spawn(hilo_segador, current_pml4, crate::task::PrivilegeLevel::KernelMode) {
            panic!("FATAL: Failed to spawn Reaper daemon. Error: {:?}", e);
        }

        let nombre_archivo = "shell.elf"; 
        if let Some(elf_slice) = crate::fs::vfs::find_file(nombre_archivo) {
            if let Some(target_pml4) = crate::mm::memory::create_isolated_pml4() {
                
                let load_result = crate::task::elf::load_elf(elf_slice, target_pml4);
                let user_stack_base = 0x800000;
                let user_stack_top = user_stack_base + 0x1000;
                let _ = crate::mm::memory::allocate_and_map_user_page(target_pml4, x86_64::VirtAddr::new(user_stack_base));
                
                match load_result {
                    Ok(entry_point) => {
                        let entry_fn: fn() -> ! = unsafe { ::core::mem::transmute(entry_point as usize) };
                        match tm.spawn_dynamic(entry_fn, target_pml4, user_stack_top) {
                            Ok(_) => crate::serial_println!("[OK] User Shell deployed and queued for Ring 3."),
                            Err(e) => panic!("FATAL: Failed to spawn Shell task. Error: {:?}", e),
                        }
                    },
                    Err(e) => { 
                        let sys_err = crate::core::error::KernelError::System(crate::core::error::SystemError::ElfParseFailed(e));
                        panic!("FATAL: Failed to parse shell.elf. Error: {:?}", sys_err); 
                    }
                }
            } else { panic!("FATAL: Out of memory. Could not isolate PML4 for Shell."); }
        } else { panic!("FATAL: '{}' not found in Initramfs TAR.", nombre_archivo); }
    });
    
    crate::println!("[OK] TaskManager y Scheduler asincrono en linea.");
    crate::core::idt::reload();
}