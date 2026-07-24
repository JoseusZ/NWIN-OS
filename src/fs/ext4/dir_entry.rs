// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ext4 directory-entry structures.

/// Fixed-size header at the start of every ext4 directory entry
/// inside a directory inode's data blocks.
#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4DirEntryHeader {
    /// Inode number this entry points at; `0` means the slot is
    /// unused.
    pub inode: u32,
    /// Total length of this record in bytes (used to jump to the
    /// next entry in the same directory block).
    pub rec_len: u16,
    /// Actual length of the file name that follows the header.
    pub name_len: u8,
    /// File type: `1` = regular file, `2` = directory, …
    pub file_type: u8,
}

// Nota: El texto del nombre sigue inmediatamente después de estos 8 bytes en la memoria.