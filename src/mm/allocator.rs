// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use linked_list_allocator::Heap;
use spin::Mutex;
use x86_64::instructions::interrupts;
use core::alloc::{GlobalAlloc, Layout};

// 1. Nuestro propio contenedor que protege el Heap contra interrupciones
pub struct InterruptSafeHeap {
    inner: Mutex<Heap>,
}

impl InterruptSafeHeap {
    pub const fn empty() -> Self {
        InterruptSafeHeap { inner: Mutex::new(Heap::empty()) }
    }
    
    pub unsafe fn init(&self, heap_start: *mut u8, heap_size: usize) {
        interrupts::without_interrupts(|| {
            self.inner.lock().init(heap_start as *mut u8, heap_size);
        });
    }
}

// 2. Le enseñamos a Rust cómo usar este contenedor seguro
unsafe impl GlobalAlloc for InterruptSafeHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        interrupts::without_interrupts(|| {
            self.inner.lock().allocate_first_fit(layout)
                .ok()
                .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr())
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        interrupts::without_interrupts(|| {
            self.inner.lock().deallocate(core::ptr::NonNull::new_unchecked(ptr), layout);
        });
    }
}

// 3. Registramos nuestro Allocator invulnerable
#[global_allocator]
pub static ALLOCATOR: InterruptSafeHeap = InterruptSafeHeap::empty();

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB de Heap inicial

pub fn init_heap(mapper: &mut impl x86_64::structures::paging::Mapper<x86_64::structures::paging::Size4KiB>, 
                 frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<x86_64::structures::paging::Size4KiB>) -> Result<(), x86_64::structures::paging::mapper::MapToError<x86_64::structures::paging::Size4KiB>> {
    
    use x86_64::structures::paging::{Page, PageTableFlags};

    let page_range = {
        let heap_start = x86_64::VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator.allocate_frame().ok_or(x86_64::structures::paging::mapper::MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    // Llamamos a nuestro init protegido
    unsafe { ALLOCATOR.init(HEAP_START as *mut u8, HEAP_SIZE) };
    Ok(())
}