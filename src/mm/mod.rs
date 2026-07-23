// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod memory;
pub mod allocator;

use x86_64::VirtAddr;

pub fn init() {
    let hhdm_response = crate::HHDM_REQUEST.response().expect("Fallo Fatal: HHDM no proporcionado por Limine");
    let phys_offset = VirtAddr::new(hhdm_response.offset);

    crate::mm::memory::get_allocator().init();
    let mut frame_allocator = crate::mm::memory::SystemFrameAllocator;
    
    let mut mapper = unsafe { crate::mm::memory::isolate_and_init_paging(phys_offset, &mut frame_allocator) };
    
    crate::mm::allocator::init_heap(&mut mapper, &mut frame_allocator).expect("Fallo critico al crear el Heap");
    crate::println!("[OK] Gestor de Memoria y Heap listos.");
}