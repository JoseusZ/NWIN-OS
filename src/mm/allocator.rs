// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Global heap allocator.
//!
//! `InterruptSafeHeap` wraps `linked_list_allocator::Heap` behind a
//! `Mutex` and brackets every operation with `without_interrupts` so
//! an IRQ cannot enter the heap code mid-allocation. Registered with
//! `#[global_allocator]` so `alloc` calls in the kernel land here.

use linked_list_allocator::Heap;
use spin::Mutex;
use x86_64::instructions::interrupts;
use core::alloc::{GlobalAlloc, Layout};

/// Heap wrapper that closes the IRQ window around every allocation
/// and deallocation.
pub struct InterruptSafeHeap {
    inner: Mutex<Heap>,
}

impl InterruptSafeHeap {
    /// Returns an empty heap. Call [`Self::init`] once with the
    /// physical map of the kernel heap.
    pub const fn empty() -> Self {
        InterruptSafeHeap { inner: Mutex::new(Heap::empty()) }
    }

    /// Initialises the heap region with interrupts disabled.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `[heap_start, heap_start +
    /// heap_size)` is a contiguous, valid region of kernel memory and
    /// that no other code is concurrently touching the heap.
    pub unsafe fn init(&self, heap_start: *mut u8, heap_size: usize) {
        interrupts::without_interrupts(|| {
            self.inner.lock().init(heap_start, heap_size);
        });
    }
}

unsafe impl GlobalAlloc for InterruptSafeHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        interrupts::without_interrupts(|| {
            self.inner
                .lock()
                .allocate_first_fit(layout)
                .ok()
                .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr())
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        interrupts::without_interrupts(|| {
            self.inner
                .lock()
                .deallocate(core::ptr::NonNull::new_unchecked(ptr), layout);
        });
    }
}

/// Global allocator hooked up to `alloc` failures via `oom` if the
/// kernel runs out of heap space.
#[global_allocator]
pub static ALLOCATOR: InterruptSafeHeap = InterruptSafeHeap::empty();

/// Virtual address at which the kernel heap starts.
pub const HEAP_START: usize = 0x_4444_4444_0000;
/// Initial heap reservation size (1 MiB). May grow on demand via the
/// `linked_list_allocator` free-list once it runs out.
pub const HEAP_SIZE: usize = 1024 * 1024;

/// Maps the kernel heap region into the current page table and hands
/// the resulting bytes to the global allocator.
pub fn init_heap(
    mapper: &mut impl x86_64::structures::paging::Mapper<x86_64::structures::paging::Size4KiB>,
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<x86_64::structures::paging::Size4KiB>,
) -> Result<(), x86_64::structures::paging::mapper::MapToError<x86_64::structures::paging::Size4KiB>> {
    use x86_64::structures::paging::{Page, PageTableFlags};

    let page_range = {
        let heap_start = x86_64::VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(x86_64::structures::paging::mapper::MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    unsafe { ALLOCATOR.init(HEAP_START as *mut u8, HEAP_SIZE) };
    Ok(())
}