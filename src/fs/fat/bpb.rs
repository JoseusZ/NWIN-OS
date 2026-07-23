// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FAT BIOS Parameter Block (BPB) structures.
//!
//! Pure data definitions: the boot sector layouts for FAT12/16/32
//! and the 32-byte directory entry. Every field is `packed` to match
//! the on-disk byte order exactly. The runtime logic that reads
//! these sectors lives in [`crate::fs::fat::volume`].

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Fat16BootSector {
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub table_count: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media_type: u8,
    pub table_size_16: u16,
    pub sectors_per_track: u16,
    pub head_side_count: u16,
    pub hidden_sector_count: u32,
    pub total_sectors_32: u32,

    // FAT16-only extension fields.
    pub drive_number: u8,
    pub reserved_1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fat_type_label: [u8; 8],
}

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Fat32BootSector {
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub table_count: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media_type: u8,
    pub table_size_16: u16,
    pub sectors_per_track: u16,
    pub head_side_count: u16,
    pub hidden_sector_count: u32,
    pub total_sectors_32: u32,

    // FAT32-only extension fields.
    pub table_size_32: u32,
    pub extended_flags: u16,
    pub fat_version: u16,
    pub root_cluster: u32,
    pub fat_info: u16,
    pub backup_bs_sector: u16,
    pub reserved_0: [u8; 12],
    pub drive_number: u8,
    pub reserved_1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fat_type_label: [u8; 8],
}

/// One 32-byte FAT directory entry (8.3 format).
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct DirectoryEntry {
    /// 8.3 short name: 8 bytes for the basename, 3 bytes for the extension.
    pub name: [u8; 11],
    /// Bitfield of FAT attributes (read-only, hidden, directory, …).
    pub attributes: u8,
    pub reserved: u8,
    pub creation_time_tenths: u8,
    pub creation_time: u16,
    pub creation_date: u16,
    pub last_access_date: u16,
    /// High 16 bits of the first cluster number.
    pub first_cluster_high: u16,
    pub write_time: u16,
    pub write_date: u16,
    /// Low 16 bits of the first cluster number.
    pub first_cluster_low: u16,
    /// File size in bytes; `0` for directories.
    pub file_size: u32,
}

/// Discriminated union of the two FAT variants we support.
#[derive(Debug)]
pub enum FatType {
    Fat16(Fat16BootSector),
    Fat32(Fat32BootSector),
}