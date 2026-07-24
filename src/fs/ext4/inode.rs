// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext4 inode structures and accessors.

/// On-disk ext4 inode (128 bytes in the modern ext4 layout), packed
/// to match the layout inside an inode table.
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4Inode {
    /// File mode: bit 15..12 holds the file type
    /// (`0x4000` = directory, `0x8000` = regular file, …).
    pub i_mode: u16,
    pub i_uid: u16,
    /// Low 32 bits of the file size in bytes.
    pub i_size_lo: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks_lo: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    /// Root of the on-disk extent tree (or the legacy direct/indirect
    /// block pointers for older revisions).
    pub i_block: [u8; 60],
    pub i_generation: u32,
    pub i_file_acl_lo: u32,
    /// High 32 bits of the file size in bytes (needed for files
    /// larger than 4 GiB).
    pub i_size_hi: u32,
    pub i_obso_faddr: u32,
    // The remaining `osd2` and `extra_isize` fields are omitted to
    // keep the MVP inode definition small.
}

impl Ext4Inode {
    /// Returns whether this inode represents a directory, as derived
    /// from the Unix-style file-type bits of `i_mode`.
    pub fn is_directory(&self) -> bool {
        let mode = { self.i_mode };
        (mode & 0xF000) == 0x4000
    }

    /// Returns the full 64-bit file size in bytes reconstructed from
    /// `i_size_lo` and `i_size_hi`.
    pub fn size(&self) -> u64 {
        let lo = { self.i_size_lo } as u64;
        let hi = { self.i_size_hi } as u64;
        (hi << 32) | lo
    }
}