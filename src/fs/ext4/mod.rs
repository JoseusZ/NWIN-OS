// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext2/3/4 filesystem driver.
//!
//! Exposes ext4 volumes through the kernel's [`VNode`] abstraction:
//! [`Ext4Node`] handles extent-tree-based block reads and directory
//! entry iteration, while the supporting submodules own the on-disk
//! structures ([`super_block`], [`inode`], [`extents`], [`block_group`],
//! [`dir_entry`]) and the high-level volume operations ([`volume`]).

pub mod super_block;
pub mod inode;
pub mod extents;
pub mod volume;
pub mod block_group;
pub mod dir_entry;

use alloc::sync::Arc;
use crate::core::error::FsError;
use crate::fs::vfs::VNode;
use crate::fs::ext4::volume::Ext4Volume;
use crate::fs::ext4::inode::Ext4Inode;
use crate::fs::ext4::dir_entry::Ext4DirEntryHeader;

pub struct Ext4Node {
    volume: Arc<Ext4Volume>,
    inode_num: u32,
    inode: Ext4Inode,
    is_directory: bool,
    size: usize,
}

impl Ext4Node {
    /// Builds the root node of Ext4 (Inode 2) by reading it directly from the disk.
    pub fn new_root(volume: Arc<Ext4Volume>) -> Self {
        let inode_num = 2; // Inode 2 is always the root directory in Linux.

        let inode = volume.read_inode(inode_num).unwrap_or_else(|_| unsafe { core::mem::zeroed() });

        let is_dir = inode.is_directory();
        let size = inode.size() as usize;

        Self {
            volume,
            inode_num,
            inode,
            is_directory: is_dir,
            size,
        }
    }

    /// Getter to access the inode number while preserving encapsulation (this removes the warning).
    pub fn inode_num(&self) -> u32 {
        self.inode_num
    }


    /// Adds a new entry to this directory by shrinking the padding of the last entry.
    ///
    /// **Phase 3.3:** Migrated to `Result<(), FsError>`. Mapping rules:
    /// - `!is_directory` → `FsError::CorruptedDirectory` (structural VNode invariant).
    /// - `logical_to_physical_sector` fails → `FsError::CorruptedDirectory` (inode with
    ///   corrupted or empty extents for a directory).
    /// - `read_block` / `write_block` → `FsError::BlockRead` / `FsError::BlockWrite`.
    /// - No padding space → `FsError::NoSpace`.
    /// - `rec_len == 0` or overflow → `FsError::CorruptedDirectory`.
    pub fn add_entry(&self, name: &str, inode_num: u32) -> Result<(), FsError> {
        if !self.is_directory { return Err(FsError::CorruptedDirectory); }

        let block_size = self.volume.super_block.block_size() as usize;
        let mut buffer = alloc::vec![0u8; block_size];

        // 1. Locate the physical sector of the directory block (logical block 0 of Inode 2)
        let physical_sector = self.volume.logical_to_physical_sector(&self.inode, 0)
            .ok_or(FsError::CorruptedDirectory)?;

        // 2. Read the full block (4096 bytes = 8 sectors)
        for i in 0..(block_size / 512) {
            self.volume.device
                .read_block(physical_sector + i as u64, &mut buffer[i * 512 .. (i + 1) * 512])
                .map_err(|_| FsError::BlockRead)?;
        }

        // 3. Walk the records until we find the last one (the one that owns the trailing padding)
        let mut offset = 0;
        loop {
            let entry_ptr = unsafe { buffer.as_ptr().add(offset) as *const Ext4DirEntryHeader };
            let rec_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*entry_ptr).rec_len)) } as usize;
            let name_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*entry_ptr).name_len)) } as usize;

            // If this record spans exactly to the end of the 4096-byte block... it's the last one!
            if offset + rec_len == block_size {

                // Compute the actual size taken by this record
                let real_len = 8 + name_len;
                let aligned_len = (real_len + 3) & !3; // Align to multiples of 4 bytes

                let space_left = rec_len - aligned_len;
                let new_entry_real = 8 + name.len();
                let new_entry_aligned = (new_entry_real + 3) & !3;

                if space_left < new_entry_aligned {
                    return Err(FsError::NoSpace);
                }

                // A. Shrink the current entry by editing its rec_len in RAM
                unsafe {
                    let entry_mut = &mut *(buffer.as_mut_ptr().add(offset) as *mut Ext4DirEntryHeader);
                    core::ptr::write_unaligned(core::ptr::addr_of_mut!(entry_mut.rec_len), aligned_len as u16);
                }

                let new_offset = offset + aligned_len;

                // B. Build our new entry byte by byte using pointers for maximum safety
                unsafe {
                    let new_entry_ptr = buffer.as_mut_ptr().add(new_offset);

                    // Inode (4 bytes)
                    core::ptr::write_unaligned(new_entry_ptr as *mut u32, inode_num);
                    // Rec_len (2 bytes, offset 4) - takes the entire leftover padding
                    core::ptr::write_unaligned(new_entry_ptr.add(4) as *mut u16, space_left as u16);
                    // Name_len (1 byte, offset 6)
                    core::ptr::write_unaligned(new_entry_ptr.add(6) as *mut u8, name.len() as u8);
                    // File_type (1 byte, offset 7) -> 1 = Regular file
                    core::ptr::write_unaligned(new_entry_ptr.add(7) as *mut u8, 1);

                    // C. Copy the file name (starts at offset 8)
                    core::ptr::copy_nonoverlapping(name.as_ptr(), new_entry_ptr.add(8), name.len());
                }
                break;
            }

            offset += rec_len;
            if offset >= block_size || rec_len == 0 { return Err(FsError::CorruptedDirectory); }
        }

        // 4. Burn the updated directory back to the disk
        for i in 0..(block_size / 512) {
            self.volume.device
                .write_block(physical_sector + i as u64, &buffer[i * 512 .. (i + 1) * 512])
                .map_err(|_| FsError::BlockWrite)?;
        }

        Ok(())
    }

}

impl VNode for Ext4Node {
    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn get_size(&self) -> usize {
        self.size
    }

    /// Searches a file by name inside this directory.
    fn lookup(&self, name: &str) -> Option<Arc<dyn VNode>> {
        if !self.is_directory { return None; }

        // Probe 1: Confirm we are at the correct inode and inspect the size to read.
        crate::serial_println!("[EXT4-DEBUG] Starting lookup of '{}' in Inode {}. Size to read: {} bytes", name, self.inode_num, self.size);

        let mut buffer = alloc::vec![0u8; self.size];
        let bytes_read = self.read(0, &mut buffer);

        // Probe 2: Confirm whether the Extents DMA engine managed to read the physical data.
        crate::serial_println!("[EXT4-DEBUG] The DMA engine returned {} bytes of data.", bytes_read);

        if bytes_read == 0 {
            // If it returned 0, read the Extent Header to figure out why the physical translation failed.
            let header_ptr = self.inode.i_block.as_ptr() as *const crate::fs::ext4::extents::Ext4ExtentHeader;
            let magic = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header_ptr).eh_magic)) };
            crate::serial_println!("[EXT4-DEBUG] FATAL ERROR: No data was read. Extent Magic: {:#06x} (expected 0xf30a).", magic);
            return None;
        }

        let mut local_offset = 0;
        while local_offset < bytes_read {
            let entry_ptr = unsafe {
                buffer.as_ptr().add(local_offset) as *const Ext4DirEntryHeader
            };
            let entry = unsafe { &*entry_ptr };

            // Copy fields into local variables to avoid alignment problems.
            let rec_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.rec_len)) } as usize;
            let inode = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.inode)) };
            let name_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.name_len)) } as usize;

            if rec_len == 0 {
                crate::serial_println!("[EXT4-DEBUG] -> Loop broken preventively: rec_len is 0 at offset {}", local_offset);
                break;
            }

            if inode != 0 {
                // Safety against memory overflow caused by disk corruption.
                if local_offset + 8 + name_len > bytes_read {
                    crate::serial_println!("[EXT4-DEBUG] -> WARNING: Entry overflow at offset {}", local_offset);
                    break;
                }

                let name_ptr = unsafe {
                    buffer.as_ptr().add(local_offset + core::mem::size_of::<Ext4DirEntryHeader>())
                };
                let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };

                if let Ok(entry_name) = core::str::from_utf8(name_slice) {
                    // Probe 3: Print EVERY file the kernel can see on disk.
                    crate::serial_println!("[EXT4-DEBUG] -> Seen on disk: '{}' (Inode: {}, Offset: {})", entry_name, inode, local_offset);

                    if entry_name == name {
                        crate::serial_println!("[EXT4-DEBUG] -> MATCH! Requesting Inode {} from the AHCI driver...", inode);
                        if let Ok(child_inode) = self.volume.read_inode(inode) {
                            crate::serial_println!("[EXT4-DEBUG] -> Inode {} successfully loaded into RAM.", inode);
                            return Some(Arc::new(Ext4Node {
                                volume: self.volume.clone(),
                                inode_num: inode,
                                inode: child_inode,
                                is_directory: child_inode.is_directory(),
                                size: child_inode.size() as usize,
                            }));
                        } else {
                            crate::serial_println!("[EXT4-DEBUG] -> ERROR: volume.read_inode({}) failed.", inode);
                        }
                    }
                }
            }

            local_offset += rec_len;
        }

        None
    }

    /// Reads raw bytes from the disk into memory using the Extent Tree.
    fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.size || buf.is_empty() { 
        return 0; 
    }

        let block_size = self.volume.super_block.block_size() as usize;
        let bytes_to_read = core::cmp::min(buf.len(), self.size - offset);
        
        let mut bytes_copied = 0;
        let mut sector_buffer = [0u8; 512]; 
        
        let mut current_offset = offset;

        while bytes_copied < bytes_to_read {
            let logical_block = (current_offset / block_size) as u32;
            let offset_in_block = current_offset % block_size;
            
            let sector_in_block = (offset_in_block / 512) as u64;
            let offset_in_sector = current_offset % 512;
            
            if let Some(base_lba) = self.volume.logical_to_physical_sector(&self.inode, logical_block) {
                let target_sector = base_lba + sector_in_block;

                if self.volume.device.read_block(target_sector, &mut sector_buffer).is_err() {
                    break; 
                }

                let remaining_in_sector = 512 - offset_in_sector;
                let remaining_to_read = bytes_to_read - bytes_copied;
                let chunk_size = core::cmp::min(remaining_in_sector, remaining_to_read);

                buf[bytes_copied .. bytes_copied + chunk_size]
                    .copy_from_slice(&sector_buffer[offset_in_sector .. offset_in_sector + chunk_size]);

                bytes_copied += chunk_size;
                current_offset += chunk_size;
            } else {
                break;
            }
        }

        bytes_copied
    }

    
} 