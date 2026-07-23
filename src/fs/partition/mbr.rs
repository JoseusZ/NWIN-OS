// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Master Boot Record parser: constants, on-disk structures and the
//! reader that turns sector 0 into an [`Mbr`].
//!
//! All errors raised here flow through [`FsError`], which converts
//! into [`KernelError::Fs`] at the call sites.

use crate::fs::BlockDevice;
use crate::core::error::FsError;
use alloc::sync::Arc;

// =====================================================================
// PARTITION TYPE CONSTANTS
// =====================================================================
pub const PART_TYPE_EMPTY: u8 = 0x00;
pub const PART_TYPE_FAT16_CHS: u8 = 0x06;
pub const PART_TYPE_FAT32_CHS: u8 = 0x0B;
pub const PART_TYPE_FAT32_LBA: u8 = 0x0C;
pub const PART_TYPE_FAT16: u8 = 0x0E;
pub const PART_TYPE_LINUX: u8 = 0x83; // ext2/3/4 native partition type
pub const PART_TYPE_GPT_PROTECTIVE: u8 = 0xEE; // UEFI GPT marker in a protective MBR

// =====================================================================
// MBR STRUCTURES
// =====================================================================

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct PartitionEntry {
    pub bootable: u8,
    pub start_chs: [u8; 3],
    pub partition_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,     // first sector of the filesystem inside this partition
    pub total_sectors: u32,
}

impl PartitionEntry {
    /// Returns whether this partition table slot is unused.
    pub fn is_empty(&self) -> bool {
        self.partition_type == PART_TYPE_EMPTY
    }

    /// Returns whether this partition holds a FAT12/16/32 filesystem.
    pub fn is_fat(&self) -> bool {
        self.partition_type == PART_TYPE_FAT32_CHS
        || self.partition_type == PART_TYPE_FAT32_LBA
        || self.partition_type == PART_TYPE_FAT16_CHS
        || self.partition_type == PART_TYPE_FAT16
    }


    /// Returns whether this partition holds a Linux native filesystem
    /// (ext2/3/4) recognised by its 0x83 MBR type code.
    pub fn is_linux(&self) -> bool {
        self.partition_type == PART_TYPE_LINUX
    }
}

#[derive(Debug)]
pub struct Mbr {
    pub partitions: [PartitionEntry; 4],
}

impl Mbr {
    /// Reads sector 0 of `disk`, verifies the 0xAA55 boot signature,
    /// and decodes the four-entry partition table.
    ///
    /// Returns [`FsError::BlockRead`] when the underlying block device
    /// cannot serve the sector, or [`FsError::BadMagic`] when the
    /// signature at bytes 510/511 is missing.
    pub fn read_from(disk: Arc<dyn BlockDevice>) -> Result<Self, FsError> {
        let mut buffer = [0u8; 512];

        // The `BlockDevice::read_block` trait still returns
        // `Result<(), &'static str>` at the driver layer, so we lift
        // the failure into the kernel's typed error space here.
        disk.read_block(0, &mut buffer).map_err(|_| FsError::BlockRead)?;

        // Boot signature: little-endian word 0xAA55 at bytes 510/511.
        if buffer[510] != 0x55 || buffer[511] != 0xAA {
            // Reconstruct the word as it lives on disk so an operator
            // can compare it byte-for-byte with the original medium.
            let found = u16::from_le_bytes([buffer[510], buffer[511]]);
            return Err(FsError::BadMagic {
                expected: 0xAA55,
                found,
            });
        }

        // The partition table starts exactly at byte 446 of sector 0.
        let mut partitions: [PartitionEntry; 4] = unsafe { core::mem::zeroed() };

        for i in 0..4 {
            let offset = 446 + (i * 16);
            let entry_bytes = &buffer[offset..offset + 16];

            // Copy the raw bytes into our packed Rust struct.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry_bytes.as_ptr(),
                    &mut partitions[i] as *mut PartitionEntry as *mut u8,
                    16
                );
            }
        }

        Ok(Mbr { partitions })
    }

    /// Returns whether the disk uses the modern GPT scheme.
    ///
    /// The UEFI spec mandates that a GPT disk carry a single 0xEE
    /// entry covering the whole device in its protective MBR; the
    /// kernel then ignores the legacy table and reads the GPT
    /// headers starting at LBA 1.
    pub fn is_gpt_protective(&self) -> bool {
        !self.partitions[0].is_empty() && self.partitions[0].partition_type == PART_TYPE_GPT_PROTECTIVE
    }
}