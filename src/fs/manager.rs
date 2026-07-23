// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/fs/manager.rs

use alloc::sync::Arc;
use crate::fs::BlockDevice;
use crate::fs::partition::mbr::Mbr;
use crate::fs::vfs::VNode;

/// Recibe un disco físico detectado por un driver (AHCI, NVMe, etc.)
/// y se encarga de analizar sus particiones para montarlas en el VFS.
pub fn process_disk(disk: Arc<dyn BlockDevice>) {
    match Mbr::read_from(disk.clone()) {
        Ok(mbr) => {
            if mbr.is_gpt_protective() {
                crate::serial_println!("[VFS] -> Disco GPT detectado. Omitiendo MBR.");
            } else {
                crate::serial_println!("[VFS] -> ¡Firma MBR confirmada! Analizando tabla de particiones...");
                
                for (part_idx, part) in mbr.partitions.iter().enumerate() {
                    if part.is_empty() { continue; }

                    crate::serial_println!(
                        "[VFS] -> Slot {}: Tipo {:#04x} | Inicio LBA: {}", 
                        part_idx, part.partition_type, { part.start_lba }
                    );

                    if part.is_fat() {
                        crate::serial_println!("[VFS] -> ¡Partición FAT detectada! Procediendo al montaje...");
                        match crate::fs::fat::volume::FatVolume::mount(disk.clone(), part.start_lba as u64) {
                            Ok(fat) => {
                                fat.debug_info();
                                fat.list_root_dir();
                            }
                            Err(e) => crate::serial_println!("[VFS] -> Error montando FAT: {}", e),
                        }

                    } else if part.is_linux() {
                        crate::serial_println!("[VFS] -> ¡Partición Linux (Ext4) detectada! Procediendo al montaje...");
                        
                        match crate::fs::ext4::volume::Ext4Volume::mount(disk.clone(), part.start_lba as u64) {
                            Ok(ext4_vol) => {
                                ext4_vol.debug_info();
                                
                                let shared_vol = Arc::new(ext4_vol);
                                let root_node = crate::fs::ext4::Ext4Node::new_root(shared_vol.clone());
                                
                                crate::serial_println!("\n=== INICIANDO CREACIÓN DE ARCHIVO DESDE CERO ===");
                                crate::serial_println!("[VFS] -> Solicitando espacio libre...");
                                
                                if let Ok(nuevo_inodo) = shared_vol.allocate_inode() {
                                    if let Ok(nuevo_bloque) = shared_vol.allocate_block() {
                                        
                                        // 1. ESCRIBIMOS LA DATA FÍSICA
                                        let mi_texto = "¡Hola Mundo! Este archivo fue creado, mapeado y escrito 100% por NWIN OS.";
                                        crate::serial_println!("[VFS] -> Quemando texto en el Bloque Físico {}...", nuevo_bloque);
                                        let _ = shared_vol.write_data_to_block(nuevo_bloque, mi_texto.as_bytes());
                                        
                                        // 2. CONSTRUIMOS Y GUARDAMOS EL INODO
                                        crate::serial_println!("[VFS] -> Estructurando metadatos en Inodo {}...", nuevo_inodo);
                                        let inode_obj = shared_vol.build_file_inode(mi_texto.len() as u32, nuevo_bloque);
                                        if let Err(e) = shared_vol.write_inode(nuevo_inodo, &inode_obj) {
                                            crate::serial_println!("[VFS] -> ERROR guardando Inodo: {}", e);
                                        }

                                        // 3. INYECTAMOS EL ARCHIVO EN LA RAÍZ (/)
                                        let nombre_archivo = "nwin_core.txt";
                                        crate::serial_println!("[VFS] -> Enlazando '{}' en el directorio raíz...", nombre_archivo);
                                        match root_node.add_entry(nombre_archivo, nuevo_inodo) {
                                            Ok(_) => crate::serial_println!("[VFS] -> ¡ÉXITO ABSOLUTO! El archivo ha nacido formalmente."),
                                            Err(e) => crate::serial_println!("[VFS] -> ERROR ENLAZANDO: {}", e),
                                        }

                                        // ==========================================
                                        // 4. ¡LA PRUEBA DEFINITIVA DE POSIX!
                                        // ==========================================
                                        crate::serial_println!("\n=== LEYENDO EL ARCHIVO RECIÉN CREADO ===");
                                        if let Some(file_node) = root_node.lookup(nombre_archivo) {
                                            crate::serial_println!("[VFS] -> ¡El VFS encontró a '{}' existiendo en el disco!", nombre_archivo);
                                            crate::serial_println!("[VFS] -> Tamaño oficial: {} bytes", file_node.get_size());
                                            
                                            let mut read_buf = alloc::vec![0u8; file_node.get_size()];
                                            let bytes_read = file_node.read(0, &mut read_buf);
                                            
                                            if let Ok(texto_leido) = core::str::from_utf8(&read_buf[..bytes_read]) {
                                                crate::serial_println!("[VFS] -> LECTURA DIRECTA: {}", texto_leido);
                                            }
                                        } else {
                                            crate::serial_println!("[VFS] -> Algo falló, el archivo no aparece en el index.");
                                        }
                                    }
                                }
                            }
                            Err(e) => crate::serial_println!("[VFS] -> Error montando Ext4: {}", e),
                        }
                    } else {
                        crate::serial_println!("[VFS] -> Partición desconocida detectada (Tipo: {:#04x})", part.partition_type);
                    }
                }
            }
        }
        Err(e) => {
            crate::serial_println!("[VFS] Error leyendo MBR: {}", e);
        }
    }
}