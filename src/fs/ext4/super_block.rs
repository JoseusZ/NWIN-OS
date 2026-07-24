// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext4 super-block structures and helpers.

/// On-disk ext4 super-block, packed to match the layout at offset
/// 1024 of any ext2/3/4 partition.
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4SuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count_lo: u32,
    pub s_r_blocks_count_lo: u32,
    pub s_free_blocks_count_lo: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    /// `block_size = 1024 << s_log_block_size` (so `0` → 1024 B,
    /// `1` → 2048 B, `2` → 4096 B, …).
    pub s_log_block_size: u32,
    pub s_log_cluster_size: u32,
    pub s_blocks_per_group: u32,
    pub s_clusters_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    /// Must always equal `0xEF53`; the canonical ext2/3/4 magic.
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // ext4 defines many more fields; the first 84 bytes above are
    // the minimum required to mount the filesystem.
}

impl Ext4SuperBlock {
    /// Returns the block size in bytes (typically `4096`).
    pub fn block_size(&self) -> u64 {
        let log_size = { self.s_log_block_size };
        1024 << log_size
    }
}