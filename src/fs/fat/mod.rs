// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FAT12/16/32 filesystem driver.
//!
//! Exposes FAT volumes through the kernel's [`VNode`] abstraction:
//! [`FatNode`] handles cluster-to-sector translation and directory
//! iteration, while [`bpb`] owns the BIOS Parameter Block parsing
//! and [`volume`] owns the higher-level volume operations.

pub mod bpb;
pub mod volume;

use alloc::sync::Arc;
use alloc::string::String;
use crate::fs::vfs::VNode;
use crate::fs::fat::volume::FatVolume;
use crate::fs::fat::bpb::{DirectoryEntry, FatType};

/// A single node inside a FAT filesystem (file or directory).
///
/// The driver keeps the node lightweight: the actual on-disk layout
/// lives in the shared [`FatVolume`], while each `FatNode` only
/// remembers the starting cluster and a few flags.
pub struct FatNode {
    /// Shared handle to the mounted volume.
    volume: Arc<FatVolume>,
    /// First cluster of this entry on disk. `0` means "root
    /// directory" for FAT12/16, where the root lives in a fixed
    /// region rather than the cluster chain.
    start_cluster: u32,
    /// Whether this entry is a directory.
    is_directory: bool,
    /// Size in bytes (only meaningful for regular files).
    size: usize,
}

impl FatNode {
    /// Builds the root node of a freshly mounted FAT volume.
    pub fn new_root(volume: Arc<FatVolume>) -> Self {
        Self {
            volume,
            start_cluster: 0,
            is_directory: true,
            size: 0,
        }
    }
}

impl VNode for FatNode {
    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn get_size(&self) -> usize {
        self.size
    }

    fn lookup(&self, name: &str) -> Option<Arc<dyn VNode>> {
        if !self.is_directory { return None; }

        let mut buffer = [0u8; 512];

        let target_sector = if self.start_cluster == 0 {
            self.volume.root_dir_sector()
        } else {
            self.volume.cluster_to_sector(self.start_cluster)
        };

        if self.volume.device.read_block(target_sector, &mut buffer).is_err() {
            return None;
        }

        let entries = unsafe {
            core::slice::from_raw_parts(buffer.as_ptr() as *const DirectoryEntry, 16)
        };

        for entry in entries {
            // 0x00 marks the end of the directory entries.
            if entry.name[0] == 0x00 { break; }
            // Skip deleted (0xE5), LFN (0x0F) and volume label (0x08) entries.
            if entry.name[0] == 0xE5 || entry.attributes == 0x0F || (entry.attributes & 0x08) != 0 { continue; }

            // Reconstruct the 8.3 name from the FAT slot.
            let mut current_name = String::new();
            for i in 0..8 { if entry.name[i] != b' ' { current_name.push(entry.name[i] as char); } }
            if entry.name[8] != b' ' {
                current_name.push('.');
                for i in 8..11 { if entry.name[i] != b' ' { current_name.push(entry.name[i] as char); } }
            }

            if current_name.eq_ignore_ascii_case(name) {
                let is_dir = (entry.attributes & 0x10) != 0;
                let cluster = ((entry.first_cluster_high as u32) << 16) | (entry.first_cluster_low as u32);

                return Some(Arc::new(FatNode {
                    volume: self.volume.clone(),
                    start_cluster: cluster,
                    is_directory: is_dir,
                    size: { entry.file_size } as usize,
                }));
            }
        }
        None
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        if self.is_directory || offset >= self.size || self.start_cluster == 0 || buf.is_empty() {
            return 0;
        }

        let bytes_to_read = core::cmp::min(buf.len(), self.size - offset);

        let (bytes_per_sector, sectors_per_cluster) = match &self.volume.fat_type {
            FatType::Fat16(bs) => ({ bs.bytes_per_sector } as usize, { bs.sectors_per_cluster } as usize),
            FatType::Fat32(bs) => ({ bs.bytes_per_sector } as usize, { bs.sectors_per_cluster } as usize),
        };
        let bytes_per_cluster = bytes_per_sector * sectors_per_cluster;

        let mut current_cluster = self.start_cluster;
        let clusters_to_skip = offset / bytes_per_cluster;
        let mut offset_in_cluster = offset % bytes_per_cluster;

        for _ in 0..clusters_to_skip {
            if let Some(next) = self.volume.next_cluster(current_cluster) {
                current_cluster = next;
            } else {
                return 0;
            }
        }

        let mut bytes_copied = 0;
        let mut sector_buffer = [0u8; 512];

        while bytes_copied < bytes_to_read {
            let sector_in_cluster = offset_in_cluster / bytes_per_sector;
            let offset_in_sector = offset_in_cluster % bytes_per_sector;

            let target_sector = self.volume.cluster_to_sector(current_cluster) + (sector_in_cluster as u64);

            if self.volume.device.read_block(target_sector, &mut sector_buffer).is_err() {
                break;
            }

            let remaining_in_sector = bytes_per_sector - offset_in_sector;
            let remaining_to_read = bytes_to_read - bytes_copied;
            let chunk_size = core::cmp::min(remaining_in_sector, remaining_to_read);

            buf[bytes_copied .. bytes_copied + chunk_size]
                .copy_from_slice(&sector_buffer[offset_in_sector .. offset_in_sector + chunk_size]);

            bytes_copied += chunk_size;
            offset_in_cluster += chunk_size;

            if offset_in_cluster == bytes_per_cluster {
                offset_in_cluster = 0;

                if let Some(next) = self.volume.next_cluster(current_cluster) {
                    current_cluster = next;
                } else {
                    break;
                }
            }
        }

        bytes_copied
    }
}