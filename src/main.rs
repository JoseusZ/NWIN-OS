// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `NWIN OS` kernel entry point.
//!
//! `_start` is called by the Limine bootloader once the higher-half
//! direct map (HHDM) and framebuffer are ready. It performs an ordered
//! bring-up of every subsystem, then enters the idle loop and never
//! returns.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod core;
pub mod mm;
pub mod task;
pub mod fs;

#[macro_use]
pub mod drivers;

use limine::request::{FramebufferRequest, HhdmRequest, MemmapRequest, ModulesRequest};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

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

/// Boot orchestrator.
///
/// Initialises the bring-up sequence in this exact order:
/// 1. Display and SIMD coprocessor.
/// 2. GDT, IDT and `syscall` MSRs.
/// 3. Memory subsystem (frame allocator, heap, page tables).
/// 4. PCI and AHCI block-device probe.
/// 5. Virtual file system.
/// 6. Multitasking (Reaper daemon, Ring-3 shell).
/// 7. PIT timer.
///
/// Enables interrupts last, then parks on `hlt` waiting for the PIT to
/// trigger the scheduler. Never returns.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    crate::serial_println!("\n\n>>> [SYSTEM] BOOTING NWIN OS (VFS-FAT32 BUILD) <<< \n\n");

    assert!(BASE_REVISION.is_supported());

    crate::serial_println!("=== TELEMETRY SUBSYSTEM ONLINE ===");
    crate::serial_println!("[OK] COM1 serial port initialised.");

    crate::drivers::display::init();
    crate::println!("=== NWIN OS kernel booting ===");
    crate::core::cpu::init();

    crate::core::gdt::init();
    crate::core::idt::init();
    crate::core::syscall::init();
    crate::println!("[OK] GDT, IDT and Syscall MSRs configured.");

    crate::mm::init();

    let _ahci_base = crate::drivers::pci::init();

    crate::fs::init_filesystem();

    crate::task::init_multitasking();

    crate::drivers::pit::init();

    crate::println!("[OK] Interrupts enabled. Handing control to the scheduler...");
    crate::serial_println!("[MAIN] Boot thread parked on hlt; awaiting PIT tick.");

    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}