// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Virtual File System: VFS tree, file-descriptor table, manager,
//! partition probes and the FAT / ext2-4 drivers.

pub mod ext4;
pub mod fat;
pub mod fd;
pub mod manager;
pub mod partition;
pub mod vfs;

/// Loads the initramfs TAR module supplied by Limine and uses it to
/// build the in-memory VFS tree.
///
/// Logs an error to the serial console if the module is missing or
/// Limine ignored the `ModulesRequest`.
pub fn init_filesystem() {
    if let Some(modules_response) = crate::MODULES_REQUEST.response() {
        if let Some(module) = modules_response.modules().first() {
            let module_ptr = *module as *const _ as *const u64;
            let tar_ptr = unsafe { *module_ptr.add(1) as *const u8 };
            let tar_size = unsafe { *module_ptr.add(2) as usize };

            crate::fs::vfs::init(tar_ptr, tar_size);
            crate::fs::vfs::build_vfs_tree_from_tar();

            crate::println!("[OK] VFS initialised. Directory tree mounted ({} bytes).", tar_size);
        } else {
            crate::println!("[ERROR] No TAR module detected in memory.");
        }
    } else {
        crate::println!("[ERROR] Limine ignored the modules request.");
    }
}

/// VFS-facing block-device trait.
///
/// Implementors return a typed [`crate::drivers::block::DriverError`]
/// so the VFS can propagate hardware faults without losing
/// information and can attach telemetry on real hardware. The
/// translation from driver error to filesystem error is performed
/// by `From<DriverError> for FsError` declared in
/// `crate::core::error`, so `device.read_block(...)?` keeps working
/// throughout the migrated FS code.
pub trait BlockDevice: Send + Sync {
    /// Reads a single logical block (sector) into `buffer`.
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), crate::drivers::block::DriverError>;

    /// Writes the contents of `buf` to a single logical block (sector).
    fn write_block(&self, lba: u64, buf: &[u8]) -> Result<(), crate::drivers::block::DriverError>;
}

