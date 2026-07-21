use core::sync::atomic::Ordering;
use crate::task::{context_switch, PrivilegeLevel, TaskId, TaskState, TASK_MANAGER};
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags}; // VITAL para el salto a Ring 3

static mut BOOT_RSP: u64 = 0;

pub unsafe fn schedule() {
    let mut switch_data: Option<(*mut u64, u64)> = None;
    let mut next_cr3 = None; // Para guardar el mapa de memoria de la siguiente tarea

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
                // Actualizamos la tarea actual una sola vez (limpiamos el print y la duplicación)
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
    // Avisamos al hardware que recibimos el tic
    crate::core::idt::PICS.lock().notify_end_of_interrupt(crate::core::idt::TIMER_INTERRUPT);

    // =========================================================
    // CONFIGURACIÓN FINAL ANTES DEL SALTO
    // =========================================================
    
    // 1. Actualizamos la pila segura para los Syscalls
    let current_task_id = crate::task::TASK_MANAGER.lock().current_task;
    
    if let Some(next_task_id) = current_task_id {
        // Ahora es 100% seguro volver a pedir el candado
        if let Some(task) = crate::task::TASK_MANAGER.lock().task_registry.get(&next_task_id) {
            crate::core::syscall::KERNEL_RSP = task.stack_end;
        }
    }

    // 2. Cambiamos el mapa de memoria (CR3) SI ES NECESARIO
    if let Some(pml4) = next_cr3 {
        let (current_cr3, _) = Cr3::read();
        if current_cr3 != pml4 {
            Cr3::write(pml4, Cr3Flags::empty());
        }
    }

    // 3. ¡Salto a la nueva tarea!
    if let Some((old_rsp_ptr, new_rsp)) = switch_data {
        context_switch(old_rsp_ptr, new_rsp);
    }
}

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

#[derive(Debug, Clone, Copy)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub ready_tasks: usize,
    pub current_task: Option<TaskId>,
    pub ticks: u64,
}