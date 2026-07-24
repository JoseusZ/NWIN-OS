// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext4 extent-tree structures.

/// Header at the start of every level of an ext4 extent tree
/// (root, internal nodes and leaves all share this layout).
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4ExtentHeader {
    /// Magic number; must equal `0xF30A`.
    pub eh_magic: u16,
    /// Number of valid entries that follow this header.
    pub eh_entries: u16,
    /// Maximum number of entries this node can hold.
    pub eh_max: u16,
    /// Depth of the tree: `0` means this node points directly at
    /// data extents (a leaf).
    pub eh_depth: u16,
    pub eh_generation: u32,
}

/// Single extent entry; describes a contiguous run of logical
/// blocks mapped onto a contiguous run of physical blocks.
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4Extent {
    /// First logical block covered by this extent.
    pub ee_block: u32,
    /// Number of contiguous blocks mapped by this extent.
    pub ee_len: u16,
    /// High 16 bits of the starting physical block.
    pub ee_start_hi: u16,
    /// Low 32 bits of the starting physical block.
    pub ee_start_lo: u32,
}