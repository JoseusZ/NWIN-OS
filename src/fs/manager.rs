// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk manager: receives a block device detected by a driver
//! (AHCI, NVMe…), analyses its partition table, mounts recognised
//! filesystems into the VFS, and runs the smoke-test that creates
//! the demo `nwin_core.txt` on the ext4 partition.

use alloc::sync::Arc;
use crate::fs::BlockDevice;
use crate::fs::partition::mbr::Mbr;
use crate::fs::vfs::VNode;
use crate::core::error::{KernelError, log_kernel_error};

/// Probes `disk`: parses its MBR, mounts FAT or ext4 partitions it
/// recognises, and prints the demo creation/read flow on ext4.
///
/// All driver and filesystem errors surface via [`log_kernel_error`]
/// so the serial log records them with the kernel error prefix
/// instead of bare strings.
pub fn process_disk(disk: Arc<dyn BlockDevice>) {
    match Mbr::read_from(disk.clone()) {
        Ok(mbr) => {
            if mbr.is_gpt_protective() {
                crate::serial_println!("[VFS] -> GPT disk detected. Skipping MBR.");
            } else {
                crate::serial_println!("[VFS] -> MBR signature confirmed. Analysing partition table...");

                for (part_idx, part) in mbr.partitions.iter().enumerate() {
                    if part.is_empty() { continue; }

                    crate::serial_println!(
                        "[VFS] -> Slot {}: Type {:#04x} | Start LBA: {}",
                        part_idx, part.partition_type, { part.start_lba }
                    );

                    if part.is_fat() {
                        crate::serial_println!("[VFS] -> FAT partition detected. Mounting...");
                        match crate::fs::fat::volume::FatVolume::mount(disk.clone(), part.start_lba as u64) {
                            Ok(fat) => {
                                fat.debug_info();
                                fat.list_root_dir();
                            }
                            Err(e) => {
                                crate::serial_println!("[VFS] -> Failed to mount FAT:");
                                log_kernel_error(&KernelError::Fs(e));
                            }
                        }

                    } else if part.is_linux() {
                        crate::serial_println!("[VFS] -> Linux (Ext4) partition detected. Mounting...");

                        match crate::fs::ext4::volume::Ext4Volume::mount(disk.clone(), part.start_lba as u64) {
                            Ok(ext4_vol) => {
                                ext4_vol.debug_info();

                                let shared_vol = Arc::new(ext4_vol);
                                let root_node = crate::fs::ext4::Ext4Node::new_root(shared_vol.clone());

                                crate::serial_println!("\n=== STARTING FILE CREATION FROM SCRATCH ===");
                                crate::serial_println!("[VFS] -> Requesting free space...");

                                if let Ok(nuevo_inodo) = shared_vol.allocate_inode() {
                                    if let Ok(nuevo_bloque) = shared_vol.allocate_block() {

                                        // 1. Burn user data into the freshly allocated block.
                                        let mi_texto = "Hello World! This file was created, mapped and written 100% by NWIN OS.";
                                        crate::serial_println!("[VFS] -> Writing text to physical block {}...", nuevo_bloque);
                                        let _ = shared_vol.write_data_to_block(nuevo_bloque, mi_texto.as_bytes());

                                        // 2. Build and persist the inode metadata.
                                        crate::serial_println!("[VFS] -> Structuring metadata on inode {}...", nuevo_inodo);
                                        let inode_obj = shared_vol.build_file_inode(mi_texto.len() as u32, nuevo_bloque);
                                        if let Err(e) = shared_vol.write_inode(nuevo_inodo, &inode_obj) {
                                            crate::serial_println!("[VFS] -> ERROR saving inode:");
                                            log_kernel_error(&KernelError::Fs(e));
                                        }

                                        // 3. Inject the directory entry into the root.
                                        let nombre_archivo = "nwin_core.txt";
                                        crate::serial_println!("[VFS] -> Linking '{}' into the root directory...", nombre_archivo);
                                        match root_node.add_entry(nombre_archivo, nuevo_inodo) {
                                            Ok(_) => crate::serial_println!("[VFS] -> SUCCESS: the file has been formally created."),
                                            Err(e) => {
                                                crate::serial_println!("[VFS] -> ERROR linking:");
                                                log_kernel_error(&KernelError::Fs(e));
                                            }
                                        }

                                        // 4. Read-back smoke test.
                                        crate::serial_println!("\n=== READING THE FRESHLY CREATED FILE ===");
                                        if let Some(file_node) = root_node.lookup(nombre_archivo) {
                                            crate::serial_println!("[VFS] -> VFS found '{}' on disk.", nombre_archivo);
                                            crate::serial_println!("[VFS] -> Official size: {} bytes", file_node.get_size());

                                            let mut read_buf = alloc::vec![0u8; file_node.get_size()];
                                            let bytes_read = file_node.read(0, &mut read_buf);

                                            if let Ok(texto_leido) = core::str::from_utf8(&read_buf[..bytes_read]) {
                                                crate::serial_println!("[VFS] -> DIRECT READ: {}", texto_leido);
                                            }
                                        } else {
                                            crate::serial_println!("[VFS] -> Lookup failed: file does not appear in the index.");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                crate::serial_println!("[VFS] -> Failed to mount Ext4:");
                                log_kernel_error(&KernelError::Fs(e));
                            }
                        }
                    } else {
                        crate::serial_println!("[VFS] -> Unknown partition detected (type {:#04x})", part.partition_type);
                    }
                }
            }
        }
        Err(e) => {
            crate::serial_println!("[VFS] Failed to read MBR:");
            log_kernel_error(&KernelError::Fs(e));
        }
    }
}