// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/fs/ext4/mod.rs

pub mod super_block;
pub mod inode;
pub mod extents;
pub mod volume;
pub mod block_group;
pub mod dir_entry;

use alloc::sync::Arc;
use crate::core::error::FsError;
use crate::fs::vfs::VNode;
use crate::fs::ext4::volume::Ext4Volume;
use crate::fs::ext4::inode::Ext4Inode;
use crate::fs::ext4::dir_entry::Ext4DirEntryHeader;

pub struct Ext4Node {
    volume: Arc<Ext4Volume>,
    inode_num: u32,
    inode: Ext4Inode,
    is_directory: bool,
    size: usize,
}

impl Ext4Node {
    /// Construye el nodo raíz de Ext4 (Inodo 2) leyendo directamente del disco
    pub fn new_root(volume: Arc<Ext4Volume>) -> Self {
        let inode_num = 2; // El Inodo 2 siempre es la carpeta raíz en Linux
        
        let inode = volume.read_inode(inode_num).unwrap_or_else(|_| unsafe { core::mem::zeroed() });
        
        let is_dir = inode.is_directory();
        let size = inode.size() as usize;

        Self {
            volume,
            inode_num,
            inode,
            is_directory: is_dir,
            size,
        }
    }

    /// Getter para acceder al número de inodo respetando la encapsulación (Esto elimina el warning)
    pub fn inode_num(&self) -> u32 {
        self.inode_num
    }


    /// Añade una nueva entrada a este directorio encogiendo el padding
    /// de la última entrada.
    ///
    /// **Fase 3.3:** Migrado a `Result<(), FsError>`. Reglas de mapeo:
    /// - `!is_directory` → `FsError::CorruptedDirectory` (invariante
    ///   estructural del VNode).
    /// - `logical_to_physical_sector` falla → `FsError::CorruptedDirectory`
    ///   (inodo con extents corruptos o vacios para un directorio).
    /// - `read_block` / `write_block` → `FsError::BlockRead` /
    ///   `FsError::BlockWrite`.
    /// - Sin espacio de padding → `FsError::NoSpace`.
    /// - `rec_len == 0` o desbordamiento → `FsError::CorruptedDirectory`.
    pub fn add_entry(&self, name: &str, inode_num: u32) -> Result<(), FsError> {
        if !self.is_directory { return Err(FsError::CorruptedDirectory); }

        let block_size = self.volume.super_block.block_size() as usize;
        let mut buffer = alloc::vec![0u8; block_size];

        // 1. Ubicamos el sector fisico del bloque del directorio (Bloque logico 0 del Inodo 2)
        let physical_sector = self.volume.logical_to_physical_sector(&self.inode, 0)
            .ok_or(FsError::CorruptedDirectory)?;

        // 2. Leemos el bloque completo (4096 bytes = 8 sectores)
        for i in 0..(block_size / 512) {
            self.volume.device
                .read_block(physical_sector + i as u64, &mut buffer[i * 512 .. (i + 1) * 512])
                .map_err(|_| FsError::BlockRead)?;
        }

        // 3. Recorremos los registros hasta encontrar el último (el que tiene el padding sobrante)
        let mut offset = 0;
        loop {
            let entry_ptr = unsafe { buffer.as_ptr().add(offset) as *const Ext4DirEntryHeader };
            let rec_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*entry_ptr).rec_len)) } as usize;
            let name_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*entry_ptr).name_len)) } as usize;

            // Si este registro abarca hasta el final exacto del bloque de 4096 bytes... ¡Es el último!
            if offset + rec_len == block_size {
                
                // Calculamos cuánto espacio realmente ocupa
                let real_len = 8 + name_len;
                let aligned_len = (real_len + 3) & !3; // Alinear a múltiplos de 4 bytes

                let space_left = rec_len - aligned_len;
                let new_entry_real = 8 + name.len();
                let new_entry_aligned = (new_entry_real + 3) & !3;

                if space_left < new_entry_aligned {
                    return Err(FsError::NoSpace);
                }

                // A. Encogemos la entrada actual modificando su rec_len en la memoria RAM
                unsafe {
                    let entry_mut = &mut *(buffer.as_mut_ptr().add(offset) as *mut Ext4DirEntryHeader);
                    core::ptr::write_unaligned(core::ptr::addr_of_mut!(entry_mut.rec_len), aligned_len as u16);
                }

                let new_offset = offset + aligned_len;
                
                // B. Construimos nuestra nueva entrada byte a byte usando punteros para máxima seguridad
                unsafe {
                    let new_entry_ptr = buffer.as_mut_ptr().add(new_offset);
                    
                    // Inode (4 bytes)
                    core::ptr::write_unaligned(new_entry_ptr as *mut u32, inode_num);
                    // Rec_len (2 bytes, offset 4) - Se lleva todo el padding sobrante
                    core::ptr::write_unaligned(new_entry_ptr.add(4) as *mut u16, space_left as u16);
                    // Name_len (1 byte, offset 6)
                    core::ptr::write_unaligned(new_entry_ptr.add(6) as *mut u8, name.len() as u8);
                    // File_type (1 byte, offset 7) -> 1 = Archivo regular
                    core::ptr::write_unaligned(new_entry_ptr.add(7) as *mut u8, 1);
                    
                    // C. Copiamos el nombre de tu archivo (Empieza en el offset 8)
                    core::ptr::copy_nonoverlapping(name.as_ptr(), new_entry_ptr.add(8), name.len());
                }
                break;
            }

            offset += rec_len;
            if offset >= block_size || rec_len == 0 { return Err(FsError::CorruptedDirectory); }
        }

        // 4. Quemamos el directorio actualizado en el disco
        for i in 0..(block_size / 512) {
            self.volume.device
                .write_block(physical_sector + i as u64, &buffer[i * 512 .. (i + 1) * 512])
                .map_err(|_| FsError::BlockWrite)?;
        }

        Ok(())
    }

}

impl VNode for Ext4Node {
    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn get_size(&self) -> usize {
        self.size
    }

    /// Busca un archivo por nombre dentro de este directorio
    fn lookup(&self, name: &str) -> Option<Arc<dyn VNode>> {
        if !self.is_directory { return None; }

        // Sonda 1: Confirmamos que estamos en el inodo correcto y el tamaño
        crate::serial_println!("[EXT4-DEBUG] Iniciando búsqueda de '{}' en Inodo {}. Tamaño a leer: {} bytes", name, self.inode_num, self.size);

        let mut buffer = alloc::vec![0u8; self.size];
        let bytes_read = self.read(0, &mut buffer);
        
        // Sonda 2: Confirmamos si el motor DMA de Extents logró leer los datos físicos
        crate::serial_println!("[EXT4-DEBUG] El motor DMA devolvió {} bytes de datos.", bytes_read);

        if bytes_read == 0 {
            // Si devuelve 0, leemos el Header del Extent para saber por qué falló la traducción física
            let header_ptr = self.inode.i_block.as_ptr() as *const crate::fs::ext4::extents::Ext4ExtentHeader;
            let magic = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header_ptr).eh_magic)) };
            crate::serial_println!("[EXT4-DEBUG] ERROR FATAL: No se leyeron datos. Extent Magic: {:#06x} (Se esperaba 0xf30a).", magic);
            return None; 
        }

        let mut local_offset = 0;
        while local_offset < bytes_read {
            let entry_ptr = unsafe { 
                buffer.as_ptr().add(local_offset) as *const Ext4DirEntryHeader 
            };
            let entry = unsafe { &*entry_ptr };

            // Copiar campos a variables locales para evitar problemas de alineación
            let rec_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.rec_len)) } as usize;
            let inode = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.inode)) };
            let name_len = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(entry.name_len)) } as usize;

            if rec_len == 0 {
                crate::serial_println!("[EXT4-DEBUG] -> Bucle roto preventivamente: rec_len es 0 en offset {}", local_offset);
                break;
            }

            if inode != 0 {
                // Seguridad contra desbordamiento de memoria por corrupción de disco
                if local_offset + 8 + name_len > bytes_read {
                    crate::serial_println!("[EXT4-DEBUG] -> ADVERTENCIA: Entrada desbordada en offset {}", local_offset);
                    break;
                }

                let name_ptr = unsafe { 
                    buffer.as_ptr().add(local_offset + core::mem::size_of::<Ext4DirEntryHeader>()) 
                };
                let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };

                if let Ok(entry_name) = core::str::from_utf8(name_slice) {
                    // Sonda 3: Imprimimos CADA archivo que el kernel logre ver en el disco
                    crate::serial_println!("[EXT4-DEBUG] -> Visto en disco: '{}' (Inodo: {}, Offset: {})", entry_name, inode, local_offset);
                    
                    if entry_name == name {
                        crate::serial_println!("[EXT4-DEBUG] -> ¡COINCIDENCIA! Solicitando Inodo {} al driver AHCI...", inode);
                        if let Ok(child_inode) = self.volume.read_inode(inode) {
                            crate::serial_println!("[EXT4-DEBUG] -> Inodo {} cargado exitosamente en RAM.", inode);
                            return Some(Arc::new(Ext4Node {
                                volume: self.volume.clone(),
                                inode_num: inode,
                                inode: child_inode,
                                is_directory: child_inode.is_directory(),
                                size: child_inode.size() as usize,
                            }));
                        } else {
                            crate::serial_println!("[EXT4-DEBUG] -> ERROR: volume.read_inode({}) falló.", inode);
                        }
                    }
                }
            }

            local_offset += rec_len;
        }
        
        None
    }

    /// Lee bytes crudos del disco hacia la memoria usando el Árbol de Extents
    fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.size || buf.is_empty() { 
        return 0; 
    }

        let block_size = self.volume.super_block.block_size() as usize;
        let bytes_to_read = core::cmp::min(buf.len(), self.size - offset);
        
        let mut bytes_copied = 0;
        let mut sector_buffer = [0u8; 512]; 
        
        let mut current_offset = offset;

        while bytes_copied < bytes_to_read {
            let logical_block = (current_offset / block_size) as u32;
            let offset_in_block = current_offset % block_size;
            
            let sector_in_block = (offset_in_block / 512) as u64;
            let offset_in_sector = current_offset % 512;
            
            if let Some(base_lba) = self.volume.logical_to_physical_sector(&self.inode, logical_block) {
                let target_sector = base_lba + sector_in_block;

                if self.volume.device.read_block(target_sector, &mut sector_buffer).is_err() {
                    break; 
                }

                let remaining_in_sector = 512 - offset_in_sector;
                let remaining_to_read = bytes_to_read - bytes_copied;
                let chunk_size = core::cmp::min(remaining_in_sector, remaining_to_read);

                buf[bytes_copied .. bytes_copied + chunk_size]
                    .copy_from_slice(&sector_buffer[offset_in_sector .. offset_in_sector + chunk_size]);

                bytes_copied += chunk_size;
                current_offset += chunk_size;
            } else {
                break;
            }
        }

        bytes_copied
    }

    
} 