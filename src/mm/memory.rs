// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Physical frame allocator with Copy-on-Write reference counting.
//!
//! `BitmapFrameAllocator` owns two parallel arrays:
//! - `bitmap`, one bit per 4 KiB frame: `1` = busy / `0` = free.
//! - `ref_counts`, one `u16` per frame: tracks how many mappings own
//!   the frame so the deallocation path can avoid double frees and
//!   the IDT page-fault handler can clone a CoW page before writing.
//!
//! Both arrays are placed inside the first `MEMMAP_USABLE` region
//! large enough to host them, then the global `ALLOCATOR` is wired
//! to the page-table mapper via [`SystemFrameAllocator`].

use spin::Mutex;
use limine::memmap::MEMMAP_USABLE;
use x86_64::structures::paging::{
    FrameAllocator as PagingFrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB, Translate
};
use x86_64::{PhysAddr, VirtAddr};
use x86_64::registers::control::{Cr3, Cr3Flags};

/// Size of a single physical frame in bytes.
pub const FRAME_SIZE: u64 = 4096;

/// Bitmap-backed frame allocator with per-frame reference counts.
pub struct BitmapFrameAllocator {
    bitmap: &'static mut [u8],
    ref_counts: &'static mut [u16],
    total_frames: usize,
    last_free_frame_hint: usize,
}

static ALLOCATOR: Mutex<BitmapFrameAllocator> = Mutex::new(BitmapFrameAllocator::empty());

impl BitmapFrameAllocator {
    /// Returns an empty allocator. Call [`Self::init`] once to bind
    /// the bitmap and reference-count slices to physical memory.
    pub const fn empty() -> Self {
        Self {
            bitmap: &mut [],
            ref_counts: &mut [],
            total_frames: 0,
            last_free_frame_hint: 0,
        }
    }

    /// Initialises the allocator from the Limine memory map:
    /// 1. Computes the high physical address reachable from any of
    ///    the four memmap kinds the kernel can touch (usable RAM,
    ///    bootloader-reclaimable, executable/modules, framebuffer).
    /// 2. Picks the first usable region large enough to host the
    ///    bitmap + the reference-count array and parks the metadata
    ///    there.
    /// 3. Marks every usable frame free in the bitmap and reserves
    ///    the metadata region.
    pub fn init(&mut self) {
        let mmap_response = crate::MEMMAP_REQUEST.response().expect("panic: no memory map");
        let entries = mmap_response.entries();

        let mut max_addr = 0;
        for entry in entries {
            if entry.type_ == limine::memmap::MEMMAP_USABLE
                || entry.type_ == limine::memmap::MEMMAP_BOOTLOADER_RECLAIMABLE
                || entry.type_ == limine::memmap::MEMMAP_EXECUTABLE_AND_MODULES
                || entry.type_ == limine::memmap::MEMMAP_FRAMEBUFFER
            {
                let top = entry.base + entry.length;
                if top > max_addr { max_addr = top; }
            }
        }

        let total_frames = ((max_addr + FRAME_SIZE - 1) / FRAME_SIZE) as usize;

        let raw_bitmap_size = (total_frames + 7) / 8;
        let bitmap_size_aligned = (raw_bitmap_size + 7) & !7;

        let refcounts_size_in_bytes = total_frames * core::mem::size_of::<u16>();
        let metadata_total_size = bitmap_size_aligned as u64 + refcounts_size_in_bytes as u64;

        // Defensive: avoids false positives if a region's base is 0x0.
        let mut metadata_phys_addr: Option<u64> = None;
        for entry in entries {
            if entry.type_ == MEMMAP_USABLE && entry.length >= metadata_total_size {
                metadata_phys_addr = Some(entry.base);
                break;
            }
        }

        let metadata_phys_addr = metadata_phys_addr.expect("panic: no contiguous RAM for allocator metadata");

        let hhdm_offset = crate::HHDM_REQUEST.response()
            .expect("fatal: bootloader did not provide HHDM").offset;

        let metadata_virt_ptr = (hhdm_offset + metadata_phys_addr) as *mut u8;

        unsafe {
            let bitmap_slice = core::slice::from_raw_parts_mut(metadata_virt_ptr, raw_bitmap_size);
            bitmap_slice.fill(0xFF); // Every frame is busy by default.
            self.bitmap = bitmap_slice;

            let refcounts_virt_ptr = metadata_virt_ptr.add(bitmap_size_aligned) as *mut u16;
            let refcounts_slice = core::slice::from_raw_parts_mut(refcounts_virt_ptr, total_frames);
            refcounts_slice.fill(1); // Reserved frames start at refcount 1.
            self.ref_counts = refcounts_slice;
        }

        self.total_frames = total_frames;

        for entry in entries {
            if entry.type_ == MEMMAP_USABLE {
                let start_frame = ((entry.base + FRAME_SIZE - 1) / FRAME_SIZE) as usize;
                let end_frame = ((entry.base + entry.length) / FRAME_SIZE) as usize;

                for i in start_frame..end_frame {
                    self.force_free_bit(i);
                }
            }
        }

        let metadata_start_frame = (metadata_phys_addr / FRAME_SIZE) as usize;
        let metadata_end_frame = metadata_start_frame + ((metadata_total_size + FRAME_SIZE - 1) / FRAME_SIZE) as usize;

        for i in metadata_start_frame..metadata_end_frame {
            self.force_set_bit(i);
        }

        self.force_set_bit(0); // Always reserve frame 0 (NULL guard).
    }

    // ----------------------------------------------------------------
    // Internal bit manipulation helpers
    // ----------------------------------------------------------------

    fn force_set_bit(&mut self, frame: usize) {
        if frame >= self.total_frames { return; }
        let byte = frame / 8;
        let bit = frame % 8;
        self.bitmap[byte] |= 1 << bit;
        self.ref_counts[frame] = 1;
    }

    fn force_free_bit(&mut self, frame: usize) {
        if frame >= self.total_frames { return; }
        let byte = frame / 8;
        let bit = frame % 8;
        self.bitmap[byte] &= !(1 << bit);
        self.ref_counts[frame] = 0;
    }

    fn test_bit(&self, frame: usize) -> bool {
        // Out-of-range frames are treated as busy to keep `allocate_frame` sound.
        if frame >= self.total_frames { return true; }
        let byte = frame / 8;
        let bit = frame % 8;
        (self.bitmap[byte] & (1 << bit)) != 0
    }

    // ----------------------------------------------------------------
    // Public allocator API (with Copy-on-Wire reference counting)
    // ----------------------------------------------------------------

    /// Picks the lowest free frame starting from the cached hint and
    /// wraps once around the table when the hint is reached.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        for i in self.last_free_frame_hint..self.total_frames {
            if !self.test_bit(i) {
                self.force_set_bit(i);
                self.last_free_frame_hint = i + 1;
                return Some((i as u64) * FRAME_SIZE);
            }
        }

        for i in 0..self.last_free_frame_hint {
            if !self.test_bit(i) {
                self.force_set_bit(i);
                self.last_free_frame_hint = i + 1;
                return Some((i as u64) * FRAME_SIZE);
            }
        }
        None
    }

    /// Drops one reference to the frame holding `phys_addr`. The
    /// frame returns to the free pool only when the reference
    /// counter reaches zero; otherwise the underlying physical
    /// memory stays mapped for the other owners (CoW path).
    pub fn deallocate_frame(&mut self, phys_addr: u64) {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        assert!(frame < self.total_frames, "fatal: deallocate of unknown frame");

        if self.ref_counts[frame] > 0 {
            self.ref_counts[frame] -= 1;

            if self.ref_counts[frame] == 0 {
                let byte = frame / 8;
                let bit = frame % 8;
                self.bitmap[byte] &= !(1 << bit);

                if frame < self.last_free_frame_hint {
                    self.last_free_frame_hint = frame;
                }
            }
        } else {
            panic!("double free detected in Ring 0 (frame {})", frame);
        }
    }

    /// Adds an extra reference to a frame. Required by the future
    /// `fork` syscall so child tasks share their pages copy-on-write.
    pub fn reference_frame(&mut self, phys_addr: u64) {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        assert!(frame < self.total_frames, "fatal: reference of unknown frame");
        assert!(self.ref_counts[frame] > 0, "tried to share an unowned frame");

        self.ref_counts[frame] += 1;
    }

    /// Exposes the current reference count so the page-fault handler
    /// can decide whether to clone the page before writing to it.
    pub fn get_ref_count(&self, phys_addr: u64) -> u16 {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        if frame >= self.total_frames { return 1; }
        self.ref_counts[frame]
    }

    /// First-fit contiguous allocation. Reserved for non-shareable
    /// paths (DMA bounce buffers, etc.) where CoW semantics would
    /// only complicate ownership.
    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        if count == 0 { return None; }

        let mut i = 0;
        while i <= self.total_frames.saturating_sub(count) {
            let mut free_run = 0;

            for j in 0..count {
                if self.test_bit(i + j) {
                    break;
                }
                free_run += 1;
            }

            if free_run == count {
                for j in i..(i + count) {
                    self.force_set_bit(j);
                }
                return Some((i as u64) * FRAME_SIZE);
            } else {
                i += free_run + 1;
            }
        }
        None
    }
}

/// Lock-free facade around the global `ALLOCATOR`. Implementing the
/// `x86_64` `FrameAllocator` trait on a zero-sized type avoids
/// exposing the inner `MutexGuard` to callers, which would create
/// easy avenues for accidental deadlocks.
pub struct SystemFrameAllocator;

/// Acquires the global allocator guard. Equivalent to
/// `ALLOCATOR.lock()` but kept as a function so refactorings can
/// later add lock-ordering metadata in one place.
pub fn get_allocator() -> spin::MutexGuard<'static, BitmapFrameAllocator> {
    ALLOCATOR.lock()
}

unsafe impl PagingFrameAllocator<Size4KiB> for SystemFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let frame_address = get_allocator().allocate_frame()?;
            let hhdm_offset = crate::HHDM_REQUEST.response().expect("no HHDM").offset;
            let virt_addr = frame_address + hhdm_offset;
            // Zero out the freshly-allocated frame so callers don't
            // observe whatever data the boot-time memory map listed as
            // "free".
            unsafe {
                core::ptr::write_bytes(virt_addr as *mut u8, 0, 4096);
            }
            let phys_addr = PhysAddr::new(frame_address);
            Some(PhysFrame::containing_address(phys_addr))
        })
    }
}

/// Builds an isolated PML4 by cloning Limine's live table.
///
/// Copies every in-use entry from both halves. Limine leaves HHDM and
/// any user-mode mappings in the lower 256 entries; without copying
/// them the new PML4 cannot translate the offsets the kernel itself
/// uses to touch physical memory, which surfaces as cascading
/// `#GP`/`#DF`.
pub unsafe fn isolate_and_init_paging(
    physical_memory_offset: VirtAddr,
    allocator: &mut impl PagingFrameAllocator<Size4KiB>,
) -> OffsetPageTable<'static> {

    let (limine_pml4_frame, _) = Cr3::read();
    let limine_pml4_virt = physical_memory_offset + limine_pml4_frame.start_address().as_u64();
    let limine_pml4: &PageTable = &*(limine_pml4_virt.as_ptr());

    let new_pml4_frame = allocator.allocate_frame().expect("panic: no physical RAM for the new PML4");
    let new_pml4_phys = new_pml4_frame.start_address();
    let new_pml4_virt = physical_memory_offset + new_pml4_phys.as_u64();

    let new_pml4: &mut PageTable = &mut *(new_pml4_virt.as_mut_ptr());
    new_pml4.zero();

    // Copy the upper half (kernel space).
    for i in 256..512 {
        new_pml4[i] = limine_pml4[i].clone();
    }

    // Copy every non-empty lower-half entry Limine provisioned (notably
    // HHDM). The kernel will run from the upper half afterwards, so
    // the lower-half copies never collide with kernel mappings.
    for i in 0..256 {
        let entry = limine_pml4[i].clone();
        if !entry.is_unused() {
            new_pml4[i] = entry;
        }
    }

    Cr3::write(new_pml4_frame, Cr3Flags::empty());
    OffsetPageTable::new(new_pml4, physical_memory_offset)
}

// ----------------------------------------------------------------
// IRQ-safe CoW reference helpers
// ----------------------------------------------------------------

/// Bumps the reference count of `phys_addr`.
pub fn cow_reference_frame(phys_addr: u64) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().reference_frame(phys_addr);
    });
}

/// Decrements the reference count and releases the frame when it
/// reaches zero.
pub fn cow_deallocate_frame(phys_addr: u64) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().deallocate_frame(phys_addr);
    });
}

/// Reads the current reference count for the frame holding
/// `phys_addr`.
pub fn cow_get_ref_count(phys_addr: u64) -> u16 {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().get_ref_count(phys_addr)
    })
}

/// Translates a virtual address to the physical address the
/// currently active page table maps it to. Safe to call from the
/// page-fault handler.
pub fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    let hhdm_offset = crate::HHDM_REQUEST.response()
        .expect("fatal: bootloader did not provide HHDM").offset;
    let phys_mem_offset = VirtAddr::new(hhdm_offset);

    let (pml4_frame, _) = Cr3::read();
    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr()) };

    let mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };
    mapper.translate_addr(addr)
}

/// Resolves a Copy-on-Write page fault.
///
/// Promotes the page to writable if the caller is its sole owner.
/// Otherwise clones the underlying frame, reattaches it to the
/// faulting address, and decrements the previous frame's reference
/// count so the CoW bookkeeping remains consistent.
///
/// Returns `true` when the page fault is repaired and the CPU can
/// retry the faulting instruction.
pub fn resolve_cow_fault(fault_addr: VirtAddr) -> bool {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame};

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let phys_mem_offset = VirtAddr::new(hhdm_offset);

    let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    let page = Page::<Size4KiB>::containing_address(fault_addr);

    let phys_addr = match mapper.translate_addr(fault_addr) {
        Some(addr) => addr,
        None => return false,
    };

    let current_flags = match mapper.translate(fault_addr) {
        x86_64::structures::paging::mapper::TranslateResult::Mapped { flags, .. } => flags,
        _ => return false,
    };

    let ref_count = cow_get_ref_count(phys_addr.as_u64());
    if ref_count == 0 { return false; }

    // Sole owner: just flip the page to writable.
    if ref_count == 1 {
        unsafe {
            mapper.update_flags(page, current_flags | PageTableFlags::WRITABLE)
                .expect("failed to update CoW flags")
                .flush();
        }
        return true;
    }

    let new_frame_addr = match get_allocator().allocate_frame() {
        Some(addr) => addr,
        None => return false,
    };
    let new_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(new_frame_addr));

    unsafe {
        let src_ptr = (phys_mem_offset + phys_addr.as_u64()).as_ptr::<u8>();
        let dst_ptr = (phys_mem_offset + new_frame_addr).as_mut_ptr::<u8>();
        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 4096);

        let (_, flush) = mapper.unmap(page).expect("failed to unmap CoW page");
        flush.flush();

        let mut allocator = SystemFrameAllocator;
        mapper.map_to(page, new_frame, current_flags | PageTableFlags::WRITABLE, &mut allocator)
            .expect("failed to remap CoW clone")
            .flush();
    }

    cow_deallocate_frame(phys_addr.as_u64());
    true
}

/// Maps a single page in a remote PML4 without switching CR3 — used
/// by the ELF loader to set up a user-mode address space.
///
/// On success the page is allocated, zero-filled, and inserted in the
/// target page table with `PRESENT | WRITABLE | USER_ACCESSIBLE`
/// flags.
///
/// # Errors
///
/// Returns [`crate::core::error::KernelError::Memory`] when either
/// the frame allocator is exhausted
/// (`MemoryError::OutOfFrames`) or the mapping collides with an
/// existing entry (`MemoryError::InvalidMapping`).
pub fn allocate_and_map_user_page(
    target_pml4: x86_64::structures::paging::PhysFrame,
    virtual_address: x86_64::VirtAddr
) -> Result<(), crate::core::error::KernelError> {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, OffsetPageTable, PageTable, Size4KiB};

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    // Rebuild the mapper against the remote PML4 instead of reading CR3.
    let pml4_virt = phys_mem_offset + target_pml4.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr() as *mut PageTable) };
    let mut mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    let page = Page::<Size4KiB>::containing_address(virtual_address);

    // Guard against overlapping ELF segments: leave the existing
    // mapping intact and report success without re-allocating.
    if mapper.translate_page(page).is_ok() {
        return Ok(());
    }

    let frame_addr = crate::mm::memory::get_allocator()
        .allocate_frame()
        .ok_or(crate::core::error::KernelError::Memory(
            crate::core::error::MemoryError::OutOfFrames
        ))?;
    let frame = PhysFrame::containing_address(x86_64::PhysAddr::new(frame_addr));

    let flags = PageTableFlags::PRESENT
              | PageTableFlags::WRITABLE
              | PageTableFlags::USER_ACCESSIBLE;

    let mut allocator = crate::mm::memory::SystemFrameAllocator;

    unsafe {
        mapper.map_to(page, frame, flags, &mut allocator)
            .map_err(|_| crate::core::error::KernelError::Memory(
                crate::core::error::MemoryError::InvalidMapping
            ))?
            .flush();

        let hhdm_ptr = (phys_mem_offset + frame_addr).as_mut_ptr::<u8>();
        core::ptr::write_bytes(hhdm_ptr, 0, 4096);
    }

    Ok(())
}

/// Translates a virtual address inside an arbitrary remote PML4.
/// Used by the ELF loader to inspect user-space mappings before
/// attaching them to the running address space.
pub fn translate_in_pml4(
    target_pml4: x86_64::structures::paging::PhysFrame,
    virtual_address: x86_64::VirtAddr
) -> Option<x86_64::PhysAddr> {
    use x86_64::structures::paging::{OffsetPageTable, PageTable};

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    let pml4_virt = phys_mem_offset + target_pml4.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr() as *mut PageTable) };
    let mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    mapper.translate_addr(virtual_address)
}

/// Walks the page-table tree of a defunct task and releases every
/// physical frame owned by the user half (entries 0-255). The kernel
/// half (entries 256-511) is intentionally never touched.
pub unsafe fn destroy_user_address_space(pml4_frame: x86_64::structures::paging::PhysFrame) {
    use x86_64::structures::paging::{PageTable, PageTableFlags};

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    let pml4 = &mut *(pml4_virt.as_mut_ptr() as *mut PageTable);

    for p4_idx in 0..256 {
        let p4_entry = &pml4[p4_idx];
        if !p4_entry.is_unused() && p4_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {

            let pdpt_virt = phys_mem_offset + p4_entry.addr().as_u64();
            let pdpt = &mut *(pdpt_virt.as_mut_ptr() as *mut PageTable);

            for p3_idx in 0..512 {
                let p3_entry = &pdpt[p3_idx];
                if !p3_entry.is_unused() && p3_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {

                    // A 1 GiB huge page: free the frame and stop descending.
                    if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                        cow_deallocate_frame(p3_entry.addr().as_u64());
                        pdpt[p3_idx].set_unused();
                        continue;
                    }

                    let pd_virt = phys_mem_offset + p3_entry.addr().as_u64();
                    let pd = &mut *(pd_virt.as_mut_ptr() as *mut PageTable);

                    for p2_idx in 0..512 {
                        let p2_entry = &pd[p2_idx];
                        if !p2_entry.is_unused() && p2_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {

                            // A 2 MiB huge page: free the frame and stop descending.
                            if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                                cow_deallocate_frame(p2_entry.addr().as_u64());
                                pd[p2_idx].set_unused();
                                continue;
                            }

                            let pt_virt = phys_mem_offset + p2_entry.addr().as_u64();
                            let pt = &mut *(pt_virt.as_mut_ptr() as *mut PageTable);

                            for p1_idx in 0..512 {
                                let p1_entry = &pt[p1_idx];
                                if !p1_entry.is_unused() && p1_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {

                                    let phys_frame = p1_entry.addr().as_u64();
                                    cow_deallocate_frame(phys_frame);

                                    pt[p1_idx].set_unused();
                                }
                            }
                            // Done with this level-1 page table; release the frame that holds it.
                            cow_deallocate_frame(p2_entry.addr().as_u64());
                            pd[p2_idx].set_unused();
                        }
                    }
                    // Liberamos la tabla nivel 2
                    cow_deallocate_frame(p3_entry.addr().as_u64());
                    pdpt[p3_idx].set_unused();
                }
            }
            // Liberamos la tabla nivel 3
            cow_deallocate_frame(p4_entry.addr().as_u64());
            pml4[p4_idx].set_unused();
        }
    }

    // Finally release the master PML4 frame itself.
    cow_deallocate_frame(pml4_frame.start_address().as_u64());
}

/// Allocates a new PML4 for a user task and seeds it with the kernel
/// half plus any non-user mappings Limine provisioned (HHDM,
/// framebuffer). User-mode entries are deliberately excluded so the
/// task cannot reach other processes' address space.
pub fn create_isolated_pml4() -> Option<x86_64::structures::paging::PhysFrame> {
    use x86_64::structures::paging::{PageTable, PageTableFlags, FrameAllocator};
    use x86_64::registers::control::Cr3;

    let mut allocator = SystemFrameAllocator;
    let new_frame = allocator.allocate_frame()?;

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let phys_offset = x86_64::VirtAddr::new(hhdm_offset);
    let new_pml4_virt = phys_offset + new_frame.start_address().as_u64();
    let new_pml4 = unsafe { &mut *(new_pml4_virt.as_mut_ptr() as *mut PageTable) };

    // Start from a zeroed table.
    new_pml4.zero();

    let (current_pml4_frame, _) = Cr3::read();
    let current_pml4_virt = phys_offset + current_pml4_frame.start_address().as_u64();
    let current_pml4 = unsafe { &*(current_pml4_virt.as_ptr() as *const PageTable) };

    // Copy the upper half verbatim (kernel space).
    for i in 256..512 {
        new_pml4[i] = current_pml4[i].clone();
    }

    // Lower half filter: keep only kernel-side entries (e.g. HHDM,
    // framebuffer) and discard anything marked USER_ACCESSIBLE.
    for i in 0..256 {
        let entry = current_pml4[i].clone();
        if !entry.is_unused() && !entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
            new_pml4[i] = entry;
        }
    }

    Some(new_frame)
}

/// Pretty-prints the four-level translation chain for a virtual
/// address, useful when debugging ELF loads or CoW faults.
pub fn debug_page_tables(pml4_frame: x86_64::structures::paging::PhysFrame, vaddr: u64) {
    use x86_64::structures::paging::PageTable;

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("no HHDM").offset;

    let p4_idx = (vaddr >> 39) & 0x1FF;
    let p3_idx = (vaddr >> 30) & 0x1FF;
    let p2_idx = (vaddr >> 21) & 0x1FF;
    let p1_idx = (vaddr >> 12) & 0x1FF;

    let pml4_virt = pml4_frame.start_address().as_u64() + hhdm_offset;
    let pml4 = unsafe { &*(pml4_virt as *const PageTable) };

    crate::println!("--- DEBUG MMU for 0x{:X} ---", vaddr);

    let p4_entry = &pml4[p4_idx as usize];
    crate::println!("PML4[{}] -> Present: {}, Flags: {:?}", p4_idx, !p4_entry.is_unused(), p4_entry.flags());
    if p4_entry.is_unused() { return; }

    let pdpt_virt = p4_entry.addr().as_u64() + hhdm_offset;
    let pdpt = unsafe { &*(pdpt_virt as *const PageTable) };
    let p3_entry = &pdpt[p3_idx as usize];
    crate::println!("PDPT[{}] -> Present: {}, Flags: {:?}", p3_idx, !p3_entry.is_unused(), p3_entry.flags());
    if p3_entry.is_unused() { return; }

    let pd_virt = p3_entry.addr().as_u64() + hhdm_offset;
    let pd = unsafe { &*(pd_virt as *const PageTable) };
    let p2_entry = &pd[p2_idx as usize];
    crate::println!("PD[{}]   -> Present: {}, Flags: {:?}", p2_idx, !p2_entry.is_unused(), p2_entry.flags());
    if p2_entry.is_unused() { return; }

    let pt_virt = p2_entry.addr().as_u64() + hhdm_offset;
    let pt = unsafe { &*(pt_virt as *const PageTable) };
    let p1_entry = &pt[p1_idx as usize];
    crate::println!("PT[{}]   -> Present: {}, Flags: {:?}", p1_idx, !p1_entry.is_unused(), p1_entry.flags());
    crate::println!("-----------------------------");
}

/// Returns `true` when `addr` is mapped in the active page tables
/// with the user bit set, walking the four levels manually with
/// volatile reads (no `OffsetPageTable` construction in the hot path).
pub fn is_user_page_mapped(addr: x86_64::VirtAddr) -> bool {
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("fatal: no HHDM").offset;
    let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let p4_phys = pml4_frame.start_address().as_u64();

    // Read 8 bytes from physical memory via the HHDM window,
    // bypassing compiler assumptions for MMIO-style paging reads.
    let read_entry = |phys_addr: u64, index: usize| -> u64 {
        let virt = phys_addr + hhdm_offset + (index as u64 * 8);
        unsafe { core::ptr::read_volatile(virt as *const u64) }
    };

    const PRESENT: u64 = 1 << 0;
    const USER: u64 = 1 << 2;
    const HUGE: u64 = 1 << 7;
    const PHYS_MASK: u64 = 0x000FFFFF_FFFFF000;

    // Level 4
    let p4_entry = read_entry(p4_phys, usize::from(addr.p4_index()));
    if p4_entry & PRESENT == 0 { return false; }

    // Level 3
    let p3_phys = p4_entry & PHYS_MASK;
    let p3_entry = read_entry(p3_phys, usize::from(addr.p3_index()));
    if p3_entry & PRESENT == 0 { return false; }
    if p3_entry & HUGE != 0 { return (p3_entry & USER) != 0; }

    // Level 2
    let p2_phys = p3_entry & PHYS_MASK;
    let p2_entry = read_entry(p2_phys, usize::from(addr.p2_index()));
    if p2_entry & PRESENT == 0 { return false; }
    if p2_entry & HUGE != 0 { return (p2_entry & USER) != 0; }

    // Level 1
    let p1_phys = p2_entry & PHYS_MASK;
    let p1_entry = read_entry(p1_phys, usize::from(addr.p1_index()));
    if p1_entry & PRESENT == 0 { return false; }

    (p1_entry & USER) != 0
}