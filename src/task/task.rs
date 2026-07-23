// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::fd::FdTable;

/// Identificador único y absoluto para cada tarea del sistema
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// --- Distinción de Privilegio ---
/// Define el nivel de privilegio en el que se ejecutará una tarea.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeLevel {
    /// Hilo del núcleo. Tiene acceso total a memoria y hardware (Ring 0).
    KernelMode,
    /// Proceso de usuario. El scheduler lo iniciará en Ring 3 mediante iretq.
    UserMode,
}

/// Registros callee-saved según la ABI SysV x86_64.
/// ORDEN CRÍTICO: debe coincidir exactamente con el push/pop del asm de context_switch.
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

pub struct Task {
    pub id:          TaskId,
    pub parent_id:   Option<TaskId>,
    pub rsp:         u64,
    pub pml4_frame:  x86_64::structures::paging::PhysFrame,
    pub stack_start: u64,
    pub stack_end:   u64,
    _stack:          alloc::boxed::Box<[u8]>,
    pub privilege:   PrivilegeLevel,
    pub heap_start:    u64, 
    pub program_break: u64,
    pub mmap_base:     u64, 
    pub fd_table: FdTable,
}

impl Task {
    pub fn new(
        entry_point: fn() -> !,
        pml4_frame: x86_64::structures::paging::PhysFrame,
        privilege: PrivilegeLevel,
        user_stack_top: u64,
        parent_id: Option<TaskId>, // <-- NUEVO ARGUMENTO
    ) -> Result<Self, crate::core::error::KernelError> { 
        
        const STACK_SIZE: usize = 65536; // 64 KiB
        let id = TaskId::new();
        let mut stack_vec = alloc::vec::Vec::new();

        // 2. Erradicamos el bucle de muerte. Devolvemos el error estructurado.
        //
        // Fase 7: usamos `TaskError::StackAllocation` (declarado en Fase 1.6)
        // en lugar del legacy bridge `SystemError::TaskCreationFailure`. La
        // conversion `From<TaskError> for KernelError` (Fase 2.1) se activa
        // automaticamente al envolver.
        if stack_vec.try_reserve_exact(STACK_SIZE).is_err() {
            return Err(crate::core::error::KernelError::Task(
                crate::core::error::TaskError::StackAllocation
            ));
        }

        stack_vec.resize(STACK_SIZE, 0);

        // 1. Extraemos los datos crudos del vector
        let ptr = stack_vec.as_mut_ptr();
        let len = stack_vec.len();

        // 2. Le quitamos el control al vector para que Rust no llame a 'drop'
        core::mem::forget(stack_vec);

        // 3. Construimos el Box directamente desde el puntero crudo
        let slice_ptr = core::ptr::slice_from_raw_parts_mut(ptr, len);
        let stack = unsafe { alloc::boxed::Box::from_raw(slice_ptr) };

        let stack_start = stack.as_ptr() as u64;
        let stack_top   = stack_start + STACK_SIZE as u64;
        let mut rsp = stack_top & !0xF;

        unsafe {
            if privilege == PrivilegeLevel::UserMode {
                
                // *** VITAL: El '| 3' fuerza físicamente a la CPU a bajar a Ring 3 ***
                let user_data_selector = (crate::core::gdt::GDT.1.user_data_selector.0 as u64) | 3;
                let user_code_selector = (crate::core::gdt::GDT.1.user_code_selector.0 as u64) | 3;

                rsp -= 8; *(rsp as *mut u64) = user_data_selector; // SS
                rsp -= 8; *(rsp as *mut u64) = user_stack_top;      // RSP usuario
                rsp -= 8; *(rsp as *mut u64) = 0x202;              // RFLAGS (IF=1)
                rsp -= 8; *(rsp as *mut u64) = user_code_selector; // CS
                rsp -= 8; *(rsp as *mut u64) = entry_point as usize as u64; // RIP → entry point del ELF

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
        let mmap_base  = 0x0000_4000_0000_0000; // 64 TiB virtual (NUEVO)
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

    pub fn is_valid_rsp(&self, rsp: u64) -> bool {
        rsp >= self.stack_start && rsp < self.stack_end
    }
}

// =======================================================
// --- NUEVO: TRAMPOLÍN DE SALTO A ESPACIO DE USUARIO ---
// =======================================================
/// Pequeña función puente que usa los 5 valores insertados en 
/// la pila por `Task::new` para forzar la bajada de privilegios a Ring 3.
#[unsafe(naked)]
extern "C" fn user_mode_trampoline() {
    {
        core::arch::naked_asm!(
            "iretq"
        );
    }
}

/// Cambio de contexto cooperativo/preemptivo entre dos tareas de kernel.
#[unsafe(naked)]
pub unsafe extern "sysv64" fn context_switch(old_rsp: *mut u64, new_rsp: u64) {
    core::arch::naked_asm!(
        // Guardia: punteros nulos
        "test rdi, rdi",
        "jz 2f",
        "test rsi, rsi",
        "jz 2f",

        // --- Guardar contexto de la tarea saliente ---
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        "pushfq",
        // Fuerza IF=1 en el RFLAGS guardado
        "or qword ptr [rsp], 0x200",

        // Guardar RSP de la tarea saliente
        "mov [rdi], rsp",

        // Cargar RSP de la tarea entrante
        "mov rsp, rsi",

        // --- Restaurar contexto de la tarea entrante ---
        "popfq",
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",

        // ret salta al entry_point (Kernel) o al user_mode_trampoline (Ring 3)
        "ret",

        // Etiqueta de error
        "2:",
        "int3",
        "hlt",
    );
}