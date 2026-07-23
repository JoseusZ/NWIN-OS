#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

extern crate alloc;

pub mod core;
pub mod mm;
pub mod task;
pub mod fs;

#[macro_use] 
pub mod drivers;

use limine::request::{FramebufferRequest, MemmapRequest, HhdmRequest, ModulesRequest};
use limine::{BaseRevision, RequestsStartMarker, RequestsEndMarker};

// ==========================================
// PETICIONES A LIMINE (BOOTLOADER)
// ==========================================
#[used]
#[link_section = ".requests_start_marker"]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".requests"]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".requests"]
pub static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[used]
#[link_section = ".requests"]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests"]
pub static MODULES_REQUEST: ModulesRequest = ModulesRequest::new();

#[used]
#[link_section = ".requests_end_marker"]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

// ==========================================
// PUNTO DE ENTRADA PRINCIPAL (ORQUESTADOR)
// ==========================================
#[no_mangle]
pub extern "C" fn _start() -> ! {


    crate::serial_println!("\n\n>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<< \n\n");

    assert!(BASE_REVISION.is_supported());

    crate::serial_println!("=== SISTEMA DE TELEMETRIA EN LINEA ===");
    crate::serial_println!("[OK] Puerto COM1 Inicializado.");
    
    // 1. Hardware Básico y Pantalla
    crate::drivers::display::init();
    crate::println!("=== NWIN OS Kernel Iniciando ===");
    crate::core::cpu::init();
    
    // 2. Tablas del Sistema (Interrupciones y Llamadas)
    crate::core::gdt::init();
    crate::core::idt::init();
    crate::core::syscall::init();
    crate::println!("[OK] GDT, IDT y Syscalls cargadas.");

    // 3. Subsistema de Memoria
    crate::mm::init();

    // 4. Controladores de Hardware (Drivers)
    let _ahci_base = crate::drivers::pci::init();
    
    // 5. Sistema de Archivos Virtual
    crate::fs::init_filesystem();

    // 6. Multitarea y Planificador
    crate::task::init_multitasking();
    
    // 7. Reloj del Sistema
    crate::drivers::pit::init();

    crate::println!("[OK] Habilitando interrupciones. Cediendo control al usuario...");
    crate::serial_println!("[MAIN] Halting boot thread. Waiting for PIT timer...");
    
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
        crate::serial_println!("[MAIN] Woke up from HLT! (This shouldn't happen)");
    }
}