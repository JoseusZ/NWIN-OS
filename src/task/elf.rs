// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal 64-bit ELF loader: parses the program header table of an
//! in-memory executable and maps every `PT_LOAD` segment into the
//! given target PML4. The entry point is returned so the caller can
//! spawn a Ring 3 task that starts there.

use x86_64::VirtAddr;
use x86_64::structures::paging::PhysFrame;

/// Errors that can be produced by the ELF loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// The supplied slice is smaller than the ELF header.
    TooSmall,
    /// The leading `\x7FELF` magic does not match.
    InvalidMagicNumber,
    /// The `EI_CLASS` byte is not `2` (ELF64).
    Not64Bit,
    /// At least one `PT_LOAD` segment could not be mapped.
    MemoryMappingFailed,
}

// `core::fmt::Display` + `core::error::Error` for `ElfError`.
//
// Required so that `FsError::source()` and `SystemError::source()`
// (in `src/core/error.rs`) can return `Some(&ElfError)` as
// `&dyn core::error::Error`, and so that `Display` for `FsError`
// can format the `Elf(inner)` variant with `{}` instead of `{:?}`.
//
// Since `ElfError` does not wrap nested errors (its variants are
// unit), `source()` always returns `None`.

impl core::fmt::Display for ElfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ElfError::TooSmall => write!(f, "ELF binary too small"),
            ElfError::InvalidMagicNumber => write!(f, "Invalid ELF magic number"),
            ElfError::Not64Bit => write!(f, "ELF is not 64-bit"),
            ElfError::MemoryMappingFailed => write!(f, "Failed to map memory for ELF segment"),
        }
    }
}

impl core::error::Error for ElfError {}

/// Parses an ELF in memory and maps its segments into a SPECIFIC
/// PML4. Returns the entry-point virtual address on success.
pub fn load_elf(elf_slice: &[u8], target_pml4: PhysFrame) -> Result<u64, ElfError> {
    if elf_slice.len() < 64 { return Err(ElfError::TooSmall); }
    if elf_slice[0..4] != [0x7F, b'E', b'L', b'F'] { return Err(ElfError::InvalidMagicNumber); }
    if elf_slice[4] != 2 { return Err(ElfError::Not64Bit); }

    let elf_ptr = elf_slice.as_ptr();
    let entry_point = unsafe { core::ptr::read_unaligned(elf_ptr.add(0x18) as *const u64) };
    let ph_offset = unsafe { core::ptr::read_unaligned(elf_ptr.add(0x20) as *const u64) };
    let ph_entries = unsafe { core::ptr::read_unaligned(elf_ptr.add(0x38) as *const u16) };
    let ph_entry_size = unsafe { core::ptr::read_unaligned(elf_ptr.add(0x36) as *const u16) };

    let mut mapping_success = true;

    // We do NOT touch CR3 here: we stay safe in the kernel address space.
    for i in 0..ph_entries {
        let ph_ptr = unsafe { elf_ptr.add(ph_offset as usize + (i * ph_entry_size) as usize) };
        let p_type = unsafe { core::ptr::read_unaligned(ph_ptr as *const u32) };

        if p_type == 1 { // PT_LOAD: segment to load
            let p_offset = unsafe { core::ptr::read_unaligned(ph_ptr.add(0x08) as *const u64) };
            let p_vaddr = unsafe { core::ptr::read_unaligned(ph_ptr.add(0x10) as *const u64) };
            let p_filesz = unsafe { core::ptr::read_unaligned(ph_ptr.add(0x20) as *const u64) };
            let p_memsz = unsafe { core::ptr::read_unaligned(ph_ptr.add(0x28) as *const u64) };

            if p_memsz == 0 { continue; }

            // Remote page allocation in the target PML4
            let start_page = p_vaddr & !0xFFF;
            let end_page = (p_vaddr + p_memsz + 0xFFF) & !0xFFF;
            let mut current_page = start_page;

            while current_page < end_page {
                let page_addr = VirtAddr::new(current_page);
                if crate::mm::memory::allocate_and_map_user_page(target_pml4, page_addr).is_err() {
                    mapping_success = false;
                }
                current_page += 0x1000;
            }

            // Page-fragmented copy from the ELF slice into the target's physical
            // pages via the HHDM (higher-half direct map).
            if p_filesz > 0 && p_vaddr != 0 {
                let hhdm_offset = crate::HHDM_REQUEST.response().expect("No HHDM").offset;

                let mut remaining = p_filesz as usize;
                let mut file_offset = p_offset as usize;
                let mut current_vaddr = p_vaddr;

                while remaining > 0 {
                    let page_base = current_vaddr & !0xFFF;
                    let offset_in_page = (current_vaddr % 0x1000) as usize;

                    // Bytes that fit in the current page without crossing the boundary.
                    let bytes_to_copy = core::cmp::min(remaining, 0x1000 - offset_in_page);

                    // Resolve the remote physical address through the target PML4.
                    if let Some(phys_addr) = crate::mm::memory::translate_in_pml4(target_pml4, VirtAddr::new(page_base)) {
                        let hhdm_ptr = (hhdm_offset + phys_addr.as_u64() + offset_in_page as u64) as *mut u8;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                elf_ptr.add(file_offset),
                                hhdm_ptr,
                                bytes_to_copy
                            );
                        }
                    } else {
                        mapping_success = false;
                    }

                    current_vaddr += bytes_to_copy as u64;
                    file_offset += bytes_to_copy;
                    remaining -= bytes_to_copy;
                }
            }
        }
    }    
    
    if !mapping_success {
        return Err(ElfError::MemoryMappingFailed);
    }

    Ok(entry_point)
}

