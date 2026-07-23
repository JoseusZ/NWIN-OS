// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Memory subsystem: physical frame allocator, page-table mapper, and
//! the kernel heap.
//!
//! `init` is the single bring-up entry point called from `_start`
//! after the GDT/IDT are in place. It assumes Limine has already
//! supplied the higher-half direct-map (HHDM) offset.

pub mod allocator;
pub mod memory;

use x86_64::VirtAddr;

/// Initialises the bitmap frame allocator, the page-table mapper and
/// the kernel heap in this exact order.
///
/// # Panics
///
/// Panics if the Limine HHDM request did not produce a response or if
/// the heap cannot be mapped — both unrecoverable during boot.
pub fn init() {
    let hhdm_response = crate::HHDM_REQUEST
        .response()
        .expect("fatal: HHDM not provided by Limine");
    let phys_offset = VirtAddr::new(hhdm_response.offset);

    crate::mm::memory::get_allocator().init();
    let mut frame_allocator = crate::mm::memory::SystemFrameAllocator;

    let mut mapper = unsafe { crate::mm::memory::isolate_and_init_paging(phys_offset, &mut frame_allocator) };

    crate::mm::allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("fatal: failed to create kernel heap");
    crate::println!("[OK] Memory manager and heap ready.");
}