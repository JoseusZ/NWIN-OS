// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use lazy_static::lazy_static;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

// Índice para nuestra pila de emergencia
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    // 1. Creamos el TSS con una pila de emergencia inquebrantable
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        
        // --- Pila de Privilegio 0 (RSP0) ---
        // VITAL: La CPU exige esta pila estática para aterrizar con seguridad 
        // cuando salta del Ring 3 (Usuario) al Ring 0 (Kernel) por una interrupción.
        tss.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5; // 20 KB para atrapar interrupciones de usuario
            static mut RSP0_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            
            let stack_start = VirtAddr::from_ptr(core::ptr::addr_of!(RSP0_STACK));
            stack_start + STACK_SIZE // Crece hacia abajo
        };

        // --- Pila de Emergencia (IST) para Dobles Fallos ---
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5; // 20 KB de memoria para emergencias
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            // Forma estándar y estable de Rust para obtener un puntero crudo de un static mut
            let stack_start = VirtAddr::from_ptr(core::ptr::addr_of!(STACK));
            let stack_end = stack_start + STACK_SIZE;
            
            stack_end // En x86_64, la pila crece hacia abajo, devolvemos el final
        };
        tss
    };
}

lazy_static! {
    // 2. Agregamos 'pub' para que el puente de syscalls pueda leer los selectores
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // Ring 0 (Modo Kernel)
        let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        
        // --- CORRECCIÓN CRÍTICA DE HARDWARE ---
        // Para sysret, el descriptor de DATOS de usuario DEBE preceder al de CÓDIGO.
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
        // --------------------------------------
        
        // Añadimos el segmento de estado de tareas (TSS)
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        (gdt, Selectors { 
            kernel_code_selector, 
            kernel_data_selector, 
            user_code_selector,
            user_data_selector,
            tss_selector 
        })
    };
}

#[allow(dead_code)]
// Hacemos públicos los selectores para cuando necesitemos configurar la instrucción SYSCALL
#[derive(Debug)]
pub struct Selectors {
    pub kernel_code_selector: SegmentSelector,
    pub kernel_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        ES::set_reg(GDT.1.kernel_data_selector);
        SS::set_reg(GDT.1.kernel_data_selector);
        
        // En x86_64, FS y GS son ignorados para protección de memoria base, 
        // pero Linux los usa para el Thread Local Storage (TLS). 
        // Por ahora los apuntamos a data, en la Fase 4 los manipularemos vía MSRs.
        FS::set_reg(GDT.1.kernel_data_selector);
        GS::set_reg(GDT.1.kernel_data_selector);
        
        // Activamos nuestro sistema de emergencias y el puente Ring 3 -> Ring 0
        load_tss(GDT.1.tss_selector);
    }
}

pub unsafe fn set_tss_rsp0(rsp0: VirtAddr) {
    let tss_ptr = &*TSS as *const _ as *mut TaskStateSegment;
    (*tss_ptr).privilege_stack_table[0] = rsp0;
}