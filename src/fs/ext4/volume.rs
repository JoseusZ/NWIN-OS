// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext4 volume operations.
//!
//! Owns the mounted [`Ext4Volume`] and exposes the high-level
//! filesystem operations: mount-time validation, inodes and blocks
//! allocation, read/write of inodes and data blocks, and the
//! extent-tree based logical-to-physical translation.

use alloc::sync::Arc;
use crate::fs::BlockDevice;
use crate::core::error::FsError;
use super::super_block::Ext4SuperBlock;
use crate::fs::ext4::inode::Ext4Inode;
use crate::fs::ext4::extents::{Ext4ExtentHeader, Ext4Extent};

/// In-memory representation of a mounted ext4 volume.
#[allow(dead_code)]
pub struct Ext4Volume {
    pub device: Arc<dyn BlockDevice>,
    pub start_lba: u64,
    pub super_block: Ext4SuperBlock,
}

impl Ext4Volume {
    /// Reads the superblock and validates that the partition hosts an ext4 filesystem.
    ///
    /// **Phase 3.3:** Migrated from `Result<Self, &'static str>` to
    /// `Result<Self, FsError>`. The ext4 superblock magic (`0xEF53`
    /// little-endian in `s_magic`) is validated by returning
    /// `FsError::BadMagic { expected: 0xEF53, found }` to keep parity
    /// with Phases 3.1 (MBR) and 3.2 (FAT). Call sites (`manager.rs`,
    /// `ext4/mod.rs`) keep compiling without changes because `FsError`
    /// implements `Display` since Phase 1.7.
    pub fn mount(device: Arc<dyn BlockDevice>, start_lba: u64) -> Result<Self, FsError> {
        let mut buffer = [0u8; 1024]; // Read a little more to safely cover the superblock


        // The ext4 superblock starts at offset 1024 (byte 1024) from the start of the partition.
        // If the LBA starts at `start_lba`, the superblock lives at `start_lba + 2` (assuming 512-byte sectors).
        let super_block_sector = start_lba + 2;

        // Raw read: same convention as MBR/FAT. The driver layer
        // (`BlockDevice::read_block`) still returns `&'static str`;
        // we map it to `FsError::BlockRead` so primitive error types
        // never escape this module.
        device
            .read_block(super_block_sector, &mut buffer[0..512])
            .map_err(|_| FsError::BlockRead)?;
        // The superblock may straddle a sector boundary, so we read a second sector for safety.
        device
            .read_block(super_block_sector + 1, &mut buffer[512..1024])
            .map_err(|_| FsError::BlockRead)?;

        let mut super_block: Ext4SuperBlock = unsafe { core::mem::zeroed() };
        unsafe {
            // The superblock starts at byte 0 of sector 2 (offset 1024 from the start of the partition).
            core::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                &mut super_block as *mut Ext4SuperBlock as *mut u8,
                core::mem::size_of::<Ext4SuperBlock>()
            );
        }

        // Validate the ext4 superblock magic.
        // The magic word lives in little-endian on disk, so `s_magic`
        // (u16 LE) holds the native value 0xEF53 when read correctly.
        // The raw value found is preserved for diagnostics.
        if super_block.s_magic != 0xEF53 {
            return Err(FsError::BadMagic {
                expected: 0xEF53,
                found: { super_block.s_magic },
            });
        }

        Ok(Ext4Volume {
            device,
            start_lba,
            super_block,
        })
    }

    /// Returns the block number that holds the Block Group Descriptor
    /// table for the volume.
    ///
    /// When the block size is `1024` the superblock is block `1` and
    /// the BGD lives at block `2`; for larger block sizes the
    /// superblock fits inside block `0` and the BGD moves to block `1`.
    pub fn bgd_block(&self) -> u64 {
        if self.super_block.block_size() == 1024 {
            2
        } else {
            1
        }
    }

    /// Reserves the first free inode from Block Group 0.
    ///
    /// **Phase 3.3:** Migrated to `Result<u32, FsError>`. The
    /// propagation of `BlockDevice::read_block`/`write_block` errors
    /// maps to `FsError::BlockRead`/`BlockWrite`. Exhaustion of the
    /// inode bitmap is translated to `FsError::OutOfInodes`.
    pub fn allocate_inode(&self) -> Result<u32, FsError> {
        let block_size = self.super_block.block_size();

        // For our MVP, we look for free space in Block Group 0.
        let group_index = 0;

        // 1. Read the Block Group Descriptor (BGD) to know where the bitmap is stored.
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device
            .read_block(bgd_sector_start, &mut bgd_buffer)
            .map_err(|_| FsError::BlockRead)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };

        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 2. Physically read the Inode Bitmap into memory.
        let bitmap_block = { bgd.bg_inode_bitmap_lo } as u64;
        let bitmap_sector = self.start_lba + (bitmap_block * (block_size / 512));

        // Read the first sector of the bitmap (512 bytes = tracks 4096 inodes, enough for the MVP).
        let mut bitmap_buffer = [0u8; 512];
        self.device
            .read_block(bitmap_sector, &mut bitmap_buffer)
            .map_err(|_| FsError::BlockRead)?;

        // 3. Scan the bytes looking for a bit that is '0'.
        for (byte_index, byte) in bitmap_buffer.iter_mut().enumerate() {
            if *byte != 0xFF { // If the byte is not 11111111 (255), it has at least one free '0' bit.
                for bit_index in 0..8 {
                    if (*byte & (1 << bit_index)) == 0 {
                        // WE FOUND A FREE INODE!

                        // 4. Mark it as occupied (flip 0 to 1 with a bitwise OR).
                        *byte |= 1 << bit_index;

                        // 5. FIRE THE WRITE DMA! Persist the updated bitmap to disk.
                        self.device
                            .write_block(bitmap_sector, &bitmap_buffer)
                            .map_err(|_| FsError::BlockWrite)?;

                        // 6. Compute the real inode number (1-based in ext4).
                        let inode_num = (byte_index * 8 + bit_index + 1) as u32;

                        crate::serial_println!("[EXT4-ALLOC] Inode successfully reserved on disk: {}", inode_num);
                        return Ok(inode_num);
                    }
                }
            }
        }

        Err(FsError::OutOfInodes)
    }

    /// Looks up a free physical data block, marks it as occupied and
    /// persists the change to disk.
    ///
    /// **Phase 3.3:** Migrated to `Result<u32, FsError>`. Same
    /// mapping rules as `allocate_inode`: `read_block` → `BlockRead`,
    /// `write_block` → `BlockWrite`, exhaustion → `OutOfBlocks`.
    pub fn allocate_block(&self) -> Result<u32, FsError> {
        let block_size = self.super_block.block_size();
        let group_index = 0; // For the MVP we keep operating on Group 0

        // 1. Read the Block Group Descriptor (BGD) to find the Block Bitmap.
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device
            .read_block(bgd_sector_start, &mut bgd_buffer)
            .map_err(|_| FsError::BlockRead)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };

        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 2. Physically locate the Block Bitmap.
        let bitmap_block = { bgd.bg_block_bitmap_lo } as u64;
        let bitmap_sector = self.start_lba + (bitmap_block * (block_size / 512));

        // Read the DMA sector.
        let mut bitmap_buffer = [0u8; 512];
        self.device
            .read_block(bitmap_sector, &mut bitmap_buffer)
            .map_err(|_| FsError::BlockRead)?;

        // 3. Scan looking for a free 4096-byte block (bit == 0).
        for (byte_index, byte) in bitmap_buffer.iter_mut().enumerate() {
            if *byte != 0xFF { // If it is not full of '1's
                for bit_index in 0..8 {
                    if (*byte & (1 << bit_index)) == 0 {
                        // WE FOUND A FREE BLOCK!

                        // 4. Flip the 0 to 1 (reserved).
                        *byte |= 1 << bit_index;

                        // 5. FIRE THE DMA! Burn the updated bitmap to the magnetic disk.
                        self.device
                            .write_block(bitmap_sector, &bitmap_buffer)
                            .map_err(|_| FsError::BlockWrite)?;

                        // 6. The bit index represents the logical block number inside the group.
                        // In modern ext4 (with 4K blocks), bit 0 maps to logical block 0.
                        let block_num = (byte_index * 8 + bit_index) as u32;

                        crate::serial_println!("[EXT4-ALLOC] Physical data block reserved at: {}", block_num);
                        return Ok(block_num);
                    }
                }
            }
        }

        Err(FsError::OutOfBlocks)
    }

    /// Reads a specific inode directly from disk using AHCI (DMA).
    ///
    /// **Phase 3.3:** Migrated to `Result<Ext4Inode, FsError>`. The
    /// inode-range validation is translated to
    /// `FsError::CorruptedDirectory` (an out-of-range inode is a
    /// structural FS inconsistency, not an exhausted bitmap). The
    /// two `read_block` calls (BGD + inode) are funnelled into
    /// `FsError::BlockRead`.
    pub fn read_inode(&self, inode_num: u32) -> Result<Ext4Inode, FsError> {
        if inode_num < 1 || inode_num > self.super_block.s_inodes_count {
            return Err(FsError::CorruptedDirectory);
        }

        let block_size = self.super_block.block_size();
        let inodes_per_group = self.super_block.s_inodes_per_group;
        let inode_size = 256; // Modern standard size

        // 1. ext4 math: which Block Group does this inode belong to?
        let group_index = (inode_num - 1) / inodes_per_group;
        let inode_index_in_group = (inode_num - 1) % inodes_per_group;

        // 2. Read the Block Group Descriptor (BGD) table.
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device
            .read_block(bgd_sector_start, &mut bgd_buffer)
            .map_err(|_| FsError::BlockRead)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };

        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 3. Locate the Inode Table physically on disk (corrected absolute math).
        let inode_table_block = { bgd.bg_inode_table_lo } as u64;
        let byte_offset_in_table = (inode_index_in_group as u64) * inode_size;

        let absolute_byte_offset = (inode_table_block * block_size) + byte_offset_in_table;

        // This is where the variables the warning complained about are finally consumed.
        let target_sector = self.start_lba + (absolute_byte_offset / 512);
        let offset_in_sector = (absolute_byte_offset % 512) as usize;

        // 4. Fire the DMA and move into memory (target_sector is consumed here).
        let mut inode_buffer = [0u8; 512];
        self.device
            .read_block(target_sector, &mut inode_buffer)
            .map_err(|_| FsError::BlockRead)?;

        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        unsafe {
            // And here we consume offset_in_sector to grab the exact inode inside the 512 bytes.
            core::ptr::copy_nonoverlapping(
                inode_buffer.as_ptr().add(offset_in_sector),
                &mut inode as *mut Ext4Inode as *mut u8,
                core::mem::size_of::<Ext4Inode>()
            );
        }

        Ok(inode)

    }

    /// Writes the data of a file into a reserved physical block.
    ///
    /// **Phase 3.3:** Migrated to `Result<(), FsError>`. A buffer
    /// that overflows the block size is translated to
    /// `FsError::CorruptedDirectory` (a size inconsistent with the
    /// FS geometry); the DMA writes are translated to
    /// `FsError::BlockWrite`.
    pub fn write_data_to_block(&self, block_num: u32, data: &[u8]) -> Result<(), FsError> {
        let block_size = self.super_block.block_size() as usize;

        if data.len() > block_size {
            return Err(FsError::CorruptedDirectory);
        }

        // 1. Compute the starting physical sector (base LBA + block offset).
        let start_sector = self.start_lba + (block_num as u64 * (block_size as u64 / 512));

        // 2. Prepare a clean 4096-byte mould (zero-filled).
        let mut block_buffer = alloc::vec![0u8; block_size];

        // 3. Copy the actual data at the start of the mould.
        block_buffer[..data.len()].copy_from_slice(data);

        // 4. Write the physical block sector by sector (8 sectors of 512 bytes).
        for i in 0..(block_size / 512) {
            let sector_lba = start_sector + i as u64;
            let offset = i * 512;

            // Extract the corresponding 512-byte slice.
            let sector_slice = &block_buffer[offset .. offset + 512];

            // Fire the DMA at the disk laser!
            self.device
                .write_block(sector_lba, sector_slice)
                .map_err(|_| FsError::BlockWrite)?;
        }

        crate::serial_println!("[EXT4] Data successfully burned into Physical Block {}", block_num);
        Ok(())
    }

    /// Translates a logical block of an inode into the real physical sector (LBA).
    pub fn logical_to_physical_sector(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        let block_size = self.super_block.block_size();

        // 1. Interpret the first 12 bytes of the i_block array as the tree header.
        let header = unsafe { &*(inode.i_block.as_ptr() as *const Ext4ExtentHeader) };

        // 0xF30A is the universal magic signature of an Extent Header in Linux.
        if header.eh_magic != 0xF30A {
            return None;
        }

        // 2. For this phase, we process direct leaves (depth 0).
        // Massive or heavily fragmented files use depth > 0, but this covers the vast majority.
        if header.eh_depth == 0 {
            // Skip the 12 bytes of the header to read the extents array.
            let entries_ptr = unsafe {
                inode.i_block.as_ptr().add(core::mem::size_of::<Ext4ExtentHeader>()) as *const Ext4Extent
            };
            let entries = unsafe {
                core::slice::from_raw_parts(entries_ptr, header.eh_entries as usize)
            };

            // 3. Find which extent contains the logical block the VFS is asking for.
            for extent in entries {
                let start_logic = { extent.ee_block };
                let len = { extent.ee_len } as u32;

                if logical_block >= start_logic && logical_block < start_logic + len {
                    // Compute the offset inside this contiguous extent.
                    let offset_in_extent = logical_block - start_logic;

                    // Combine the high and low halves of the physical address.
                    let physical_block = (({ extent.ee_start_hi } as u64) << 32) | ({ extent.ee_start_lo } as u64);
                    let target_physical_block = physical_block + offset_in_extent as u64;

                    // Convert the Linux physical block (e.g. 4096 bytes) into AHCI sectors (512 bytes).
                    return Some(self.start_lba + (target_physical_block * (block_size / 512)));
                }
            }
        }
        None
    }

    pub fn debug_info(&self) {
        crate::serial_println!("=== EXT4 VOLUME INFO ===");
        crate::serial_println!("-> TOTAL INODES: {}", { self.super_block.s_inodes_count });
        crate::serial_println!("-> BLOCK SIZE: {} bytes", self.super_block.block_size());
        crate::serial_println!("=========================");

        // 4. Probe allocation + write end-to-end.
        crate::serial_println!("[VFS] -> Requesting disk space for a new file...");

        if let Ok(_nuevo_inodo) = self.allocate_inode() {
            if let Ok(nuevo_bloque) = self.allocate_block() {

                // The moment of truth!
                let mi_texto = "This file was created and written 100% from my own Rust kernel. NWIN OS lives!";

                crate::serial_println!("[VFS] -> Sending data via DMA to block {}...", nuevo_bloque);
                match self.write_data_to_block(nuevo_bloque, mi_texto.as_bytes()) {
                    Ok(_) => crate::serial_println!("[VFS] -> SUCCESS! The data now lives on the magnetic hard disk."),
                    Err(e) => crate::serial_println!("[VFS] -> WRITE ERROR: {}", e),
                }
            }
        }
    }

    /// Creates a blank inode configured as a regular file (text/binary).
    pub fn build_file_inode(&self, file_size: u32, physical_block: u32) -> Ext4Inode {
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };

        inode.i_mode = 0x81A4; // 0x8000 (Regular File) + 0x01A4 (Permissions 644 rw-r--r--)
        inode.i_uid = 0;       // Root
        inode.i_size_lo = file_size;
        inode.i_links_count = 1;
        inode.i_blocks_lo = 8; // 8 sectors of 512 bytes = 1 ext4 block (4096 bytes)
        inode.i_flags = 0x80000; // EXTENTS_FL: Fundamental. Tells ext4 we use the modern extent tree.

        // BUILDING THE EXTENT TREE (inside the 60 bytes of i_block)
        let header = crate::fs::ext4::extents::Ext4ExtentHeader {
            eh_magic: 0xF30A,
            eh_entries: 1, // We have 1 data block
            eh_max: 4,     // The inode has room for 4 branches
            eh_depth: 0,   // Depth 0 (points directly at the physical data)
            eh_generation: 0,
        };

        let extent = crate::fs::ext4::extents::Ext4Extent {
            ee_block: 0, // Logical block 0 (the start of the file)
            ee_len: 1,   // Spans 1 consecutive block
            ee_start_hi: 0,
            ee_start_lo: physical_block, // HERE IS WHERE WE CONNECT THE INODE TO YOUR RESERVED BLOCK!
        };

        unsafe {
            let i_block_ptr = inode.i_block.as_mut_ptr() as *mut u8;

            // Inject the Header
            core::ptr::copy_nonoverlapping(
                &header as *const _ as *const u8,
                i_block_ptr,
                core::mem::size_of::<crate::fs::ext4::extents::Ext4ExtentHeader>()
            );
            // Inject the Extent right after the Header
            core::ptr::copy_nonoverlapping(
                &extent as *const _ as *const u8,
                i_block_ptr.add(core::mem::size_of::<crate::fs::ext4::extents::Ext4ExtentHeader>()),
                core::mem::size_of::<crate::fs::ext4::extents::Ext4Extent>()
            );
        }

        inode
    }
/// Writes an Inode (`read-modify-write`) preserving the rest
    /// of the 512-byte sector that contains it.
    ///
    /// **Phase 3.3:** Migrated to `Result<(), FsError>`. Same rules
    /// as `read_inode`: invalid range → `CorruptedDirectory`,
    /// `read_block` → `BlockRead`, `write_block` → `BlockWrite`.
    pub fn write_inode(&self, inode_num: u32, inode: &Ext4Inode) -> Result<(), FsError> {
        if inode_num < 1 || inode_num > self.super_block.s_inodes_count {
            return Err(FsError::CorruptedDirectory);
        }

        let block_size = self.super_block.block_size();
        let inodes_per_group = self.super_block.s_inodes_per_group;
        let inode_size = 256;

        // 1. Math to locate the inode.
        let group_index = (inode_num - 1) / inodes_per_group;
        let inode_index_in_group = (inode_num - 1) % inodes_per_group;

        // 2. Read the Block Group Descriptor (BGD) table.
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device
            .read_block(bgd_sector_start, &mut bgd_buffer)
            .map_err(|_| FsError::BlockRead)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };
        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 3. Locate the exact sector of the Inode.
        let inode_table_block = { bgd.bg_inode_table_lo } as u64;
        let byte_offset_in_table = (inode_index_in_group as u64) * inode_size;
        let absolute_byte_offset = (inode_table_block * block_size) + byte_offset_in_table;

        let target_sector = self.start_lba + (absolute_byte_offset / 512);
        let offset_in_sector = (absolute_byte_offset % 512) as usize;

        // 4. READ-MODIFY-WRITE (Traer sector, modificar memoria, quemar sector)
        let mut sector_buffer = [0u8; 512];
        self.device
            .read_block(target_sector, &mut sector_buffer)
            .map_err(|_| FsError::BlockRead)?;

        unsafe {
            core::ptr::copy_nonoverlapping(
                inode as *const Ext4Inode as *const u8,
                sector_buffer.as_mut_ptr().add(offset_in_sector),
                core::mem::size_of::<Ext4Inode>()
            );
        }

        self.device
            .write_block(target_sector, &sector_buffer)
            .map_err(|_| FsError::BlockWrite)?;

        crate::serial_println!("[EXT4] Metadata updated on disk for Inode {}", inode_num);
        Ok(())
    }

}