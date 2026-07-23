// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/fs/ext4/volume.rs

use alloc::sync::Arc;
use crate::fs::BlockDevice;
use super::super_block::Ext4SuperBlock;
use crate::fs::ext4::inode::Ext4Inode;
use crate::fs::ext4::extents::{Ext4ExtentHeader, Ext4Extent};

#[allow(dead_code)]
pub struct Ext4Volume {
    pub device: Arc<dyn BlockDevice>,
    pub start_lba: u64,
    pub super_block: Ext4SuperBlock,
}

impl Ext4Volume {
    /// Lee el Superbloque y valida que sea un sistema Ext4
    pub fn mount(device: Arc<dyn BlockDevice>, start_lba: u64) -> Result<Self, &'static str> {
        let mut buffer = [0u8; 1024]; // Leemos un poco más para asegurar el superbloque
        
        
        // El superbloque de ext4 comienza en el offset 1024 (Byte 1024) respecto al inicio de la partición.
        // Si el LBA inicia en `start_lba`, el superbloque está en `start_lba + 2` (asumiendo sectores de 512 bytes).
        let super_block_sector = start_lba + 2;
        
        device.read_block(super_block_sector, &mut buffer[0..512])?;
        // A veces el superbloque puede cruzar fronteras, leemos un segundo sector por seguridad
        device.read_block(super_block_sector + 1, &mut buffer[512..1024])?;

        let mut super_block: Ext4SuperBlock = unsafe { core::mem::zeroed() };
        unsafe {
            // El superbloque empieza en el byte 0 del sector 2 (offset 1024 del inicio de partición)
            core::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                &mut super_block as *mut Ext4SuperBlock as *mut u8,
                core::mem::size_of::<Ext4SuperBlock>()
            );
        }

        // Validación de la firma mágica
        if super_block.s_magic != 0xEF53 {
            return Err("Firma Ext4 invalida (No se encontro 0xEF53)");
        }

        Ok(Ext4Volume {
            device,
            start_lba,
            super_block,
        })
    }

    pub fn bgd_block(&self) -> u64 {
        // En Ext4, el Superbloque ocupa 1024 bytes.
        // Si el tamaño de bloque es 1024, el Superbloque es el Bloque 1, y la BGD es el Bloque 2.
        // Si el tamaño de bloque es > 1024 (ej. 4096), el Superbloque está en el offset 1024 del Bloque 0, y la BGD es el Bloque 1.
        if self.super_block.block_size() == 1024 {
            2
        } else {
            1
        }
    }

    pub fn allocate_inode(&self) -> Result<u32, &'static str> {
        let block_size = self.super_block.block_size();

        // Para nuestro MVP, buscaremos espacio libre en el Grupo de Bloques 0
        let group_index = 0;

        // 1. Leer el Block Group Descriptor (BGD) para saber dónde está guardado el Mapa de Bits
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device.read_block(bgd_sector_start, &mut bgd_buffer)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };
        
        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 2. Leer físicamente el Mapa de Bits de Inodos (Inode Bitmap) a memoria
        let bitmap_block = { bgd.bg_inode_bitmap_lo } as u64;
        let bitmap_sector = self.start_lba + (bitmap_block * (block_size / 512));

        // Leemos el primer sector del bitmap (512 bytes = controlan 4096 inodos, suficiente para probar)
        let mut bitmap_buffer = [0u8; 512];
        self.device.read_block(bitmap_sector, &mut bitmap_buffer)?;

        // 3. Escanear los bytes buscando un bit que sea '0'
        for (byte_index, byte) in bitmap_buffer.iter_mut().enumerate() {
            if *byte != 0xFF { // Si el byte no es 11111111 (255), significa que tiene al menos un '0' libre
                for bit_index in 0..8 {
                    if (*byte & (1 << bit_index)) == 0 {
                        // ¡ENCONTRAMOS UN INODO LIBRE!
                        
                        // 4. Lo marcamos como Ocupado (Cambiamos el 0 a 1 usando un OR bit a bit)
                        *byte |= 1 << bit_index;

                        // 5. ¡DISPARAMOS DMA DE ESCRITURA! Guardamos el bitmap actualizado en el disco
                        self.device.write_block(bitmap_sector, &bitmap_buffer)?;

                        // 6. Calculamos el número de Inodo real (Base 1 en Ext4)
                        let inode_num = (byte_index * 8 + bit_index + 1) as u32;

                        crate::serial_println!("[EXT4-ALLOC] Inodo reservado exitosamente en el disco: {}", inode_num);
                        return Ok(inode_num);
                    }
                }
            }
        }

        Err("No hay inodos libres en el grupo de bloques actual")
    }

    /// Busca un bloque de datos físico libre, lo marca como ocupado y guarda el cambio en el disco.
    pub fn allocate_block(&self) -> Result<u32, &'static str> {
        let block_size = self.super_block.block_size();
        let group_index = 0; // Para el MVP seguimos operando en el Grupo 0

        // 1. Leer el Block Group Descriptor (BGD) para encontrar el Mapa de Bloques
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));

        let mut bgd_buffer = [0u8; 512];
        self.device.read_block(bgd_sector_start, &mut bgd_buffer)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };
        
        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 2. Localizar físicamente el Mapa de Bits de Bloques (Block Bitmap)
        let bitmap_block = { bgd.bg_block_bitmap_lo } as u64;
        let bitmap_sector = self.start_lba + (bitmap_block * (block_size / 512));

        // Leemos el sector DMA
        let mut bitmap_buffer = [0u8; 512];
        self.device.read_block(bitmap_sector, &mut bitmap_buffer)?;

        // 3. Escanear buscando un bloque de 4096 bytes libre (bit == 0)
        for (byte_index, byte) in bitmap_buffer.iter_mut().enumerate() {
            if *byte != 0xFF { // Si no está lleno de '1's
                for bit_index in 0..8 {
                    if (*byte & (1 << bit_index)) == 0 {
                        // ¡ENCONTRAMOS UN BLOQUE LIBRE!
                        
                        // 4. Cambiamos el 0 por 1 (Reservado)
                        *byte |= 1 << bit_index;

                        // 5. ¡DISPARAMOS DMA! Quemamos el mapa actualizado en el disco magnético
                        self.device.write_block(bitmap_sector, &bitmap_buffer)?;

                        // 6. El índice del bit representa el número lógico de bloque dentro del grupo
                        // En Ext4 moderno (con bloques de 4K), el bit 0 corresponde al bloque lógico 0.
                        let block_num = (byte_index * 8 + bit_index) as u32;

                        crate::serial_println!("[EXT4-ALLOC] Bloque de datos físico reservado en: {}", block_num);
                        return Ok(block_num);
                    }
                }
            }
        }

        Err("No hay bloques libres en el grupo actual")
    }

    /// Lee un Inodo específico directamente desde el disco usando AHCI (DMA)
    /// Lee un Inodo específico directamente desde el disco usando AHCI (DMA)
    pub fn read_inode(&self, inode_num: u32) -> Result<Ext4Inode, &'static str> {
        if inode_num < 1 || inode_num > self.super_block.s_inodes_count {
            return Err("Número de Inodo fuera de los límites del disco.");
        }

        let block_size = self.super_block.block_size();
        let inodes_per_group = self.super_block.s_inodes_per_group;
        let inode_size = 256; // Tamaño estándar moderno

        // 1. Matemáticas de Ext4: ¿A qué Block Group pertenece este inodo?
        let group_index = (inode_num - 1) / inodes_per_group;
        let inode_index_in_group = (inode_num - 1) % inodes_per_group;

        // 2. Leer la Tabla de Descriptores de Grupo (BGD)
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));
        
        let mut bgd_buffer = [0u8; 512];
        self.device.read_block(bgd_sector_start, &mut bgd_buffer)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };
        
        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 3. Localizar la Tabla de Inodos físicamente en el disco (Matemática Absoluta Corregida)
        let inode_table_block = { bgd.bg_inode_table_lo } as u64;
        let byte_offset_in_table = (inode_index_in_group as u64) * inode_size;
        
        let absolute_byte_offset = (inode_table_block * block_size) + byte_offset_in_table;
        
        // ¡Aquí es donde usamos las variables del warning!
        let target_sector = self.start_lba + (absolute_byte_offset / 512);
        let offset_in_sector = (absolute_byte_offset % 512) as usize;

        // 4. Disparar DMA y trasladar a memoria (Aquí se consume target_sector)
        let mut inode_buffer = [0u8; 512];
        self.device.read_block(target_sector, &mut inode_buffer)?;

        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        unsafe {
            // Y aquí consumimos el offset_in_sector para atrapar el inodo exacto dentro de los 512 bytes
            core::ptr::copy_nonoverlapping(
                inode_buffer.as_ptr().add(offset_in_sector),
                &mut inode as *mut Ext4Inode as *mut u8,
                core::mem::size_of::<Ext4Inode>()
            );
        }

        Ok(inode)
        
    }

    pub fn write_data_to_block(&self, block_num: u32, data: &[u8]) -> Result<(), &'static str> {
        let block_size = self.super_block.block_size() as usize;

        if data.len() > block_size {
            return Err("El buffer de datos excede el tamaño de un bloque Ext4");
        }

        // 1. Calculamos el sector físico inicial (Base LBA + offset del bloque)
        let start_sector = self.start_lba + (block_num as u64 * (block_size as u64 / 512));

        // 2. Preparamos un "molde" de 4096 bytes limpio (lleno de ceros)
        let mut block_buffer = alloc::vec![0u8; block_size];
        
        // 3. Copiamos nuestros datos reales al inicio del molde
        block_buffer[..data.len()].copy_from_slice(data);

        // 4. Escribimos el bloque físico sector por sector (8 sectores de 512 bytes)
        for i in 0..(block_size / 512) {
            let sector_lba = start_sector + i as u64;
            let offset = i * 512;
            
            // Extraemos la rebanada de 512 bytes correspondiente
            let sector_slice = &block_buffer[offset .. offset + 512];
            
            // ¡Disparamos el DMA al láser del disco!
            self.device.write_block(sector_lba, sector_slice)?;
        }

        crate::serial_println!("[EXT4] Datos quemados con éxito en el Bloque Físico {}", block_num);
        Ok(())
    }

    /// Convierte un bloque lógico de un inodo en un sector físico real (LBA)
    pub fn logical_to_physical_sector(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        let block_size = self.super_block.block_size();
        
        // 1. Interpretar los primeros 12 bytes del array i_block como el header del árbol
        let header = unsafe { &*(inode.i_block.as_ptr() as *const Ext4ExtentHeader) };
        
        // 0xF30A es la firma mágica universal de un Extent Header en Linux
        if header.eh_magic != 0xF30A {
            return None; 
        }

        // 2. Para esta fase, procesaremos hojas directas (Profundidad 0).
        // Archivos masivos o muy fragmentados usan profundidad > 0, pero esto cubre la gran mayoría.
        if header.eh_depth == 0 {
            // Saltamos los 12 bytes del header para leer el array de extents
            let entries_ptr = unsafe { 
                inode.i_block.as_ptr().add(core::mem::size_of::<Ext4ExtentHeader>()) as *const Ext4Extent 
            };
            let entries = unsafe { 
                core::slice::from_raw_parts(entries_ptr, header.eh_entries as usize) 
            };

            // 3. Buscamos qué extent contiene el bloque lógico que el VFS nos está pidiendo
            for extent in entries {
                let start_logic = { extent.ee_block };
                let len = { extent.ee_len } as u32;
                
                if logical_block >= start_logic && logical_block < start_logic + len {
                    // Calculamos el offset dentro de este extent continuo
                    let offset_in_extent = logical_block - start_logic;
                    
                    // Unimos la mitad alta y baja de la dirección física
                    let physical_block = (({ extent.ee_start_hi } as u64) << 32) | ({ extent.ee_start_lo } as u64);
                    let target_physical_block = physical_block + offset_in_extent as u64;
                    
                    // Convertimos el bloque físico de Linux (ej. 4096 bytes) a Sectores AHCI (512 bytes)
                    return Some(self.start_lba + (target_physical_block * (block_size / 512)));
                }
            }
        }
        None
    }

    pub fn debug_info(&self) {
        crate::serial_println!("=== INFO VOLUMEN EXT4 ===");
        crate::serial_println!("-> INODOS TOTALES: {}", { self.super_block.s_inodes_count });
        crate::serial_println!("-> TAMAÑO DE BLOQUE: {} bytes", self.super_block.block_size());
        crate::serial_println!("=========================");

        // 4. Probar la asignación y escritura
        crate::serial_println!("[VFS] -> Solicitando espacio al disco para un nuevo archivo...");
        
        if let Ok(_nuevo_inodo) = self.allocate_inode() {
            if let Ok(nuevo_bloque) = self.allocate_block() {
                
                // ¡EL MOMENTO DE LA VERDAD!
                let mi_texto = "Este archivo fue creado y escrito 100% desde mi propio kernel Rust. ¡NWIN OS vive!";
                
                crate::serial_println!("[VFS] -> Enviando datos por DMA al bloque {}...", nuevo_bloque);
                match self.write_data_to_block(nuevo_bloque, mi_texto.as_bytes()) {
                    Ok(_) => crate::serial_println!("[VFS] -> ¡ÉXITO! Los datos ahora existen en el disco duro magnético."),
                    Err(e) => crate::serial_println!("[VFS] -> ERROR DE ESCRITURA: {}", e),
                }
            }
        }
    }

    /// Crea un Inodo en blanco configurado como archivo regular (Archivo de texto/binario)
    pub fn build_file_inode(&self, file_size: u32, physical_block: u32) -> Ext4Inode {
        let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };
        
        inode.i_mode = 0x81A4; // 0x8000 (Archivo Regular) + 0x01A4 (Permisos 644 rw-r--r--)
        inode.i_uid = 0;       // Root
        inode.i_size_lo = file_size;
        inode.i_links_count = 1; 
        inode.i_blocks_lo = 8; // 8 sectores de 512 bytes = 1 bloque Ext4 (4096 bytes)
        inode.i_flags = 0x80000; // EXTENTS_FL: Fundamental. Le dice a Ext4 que usamos el árbol moderno.

        // FABRICANDO EL ÁRBOL DE EXTENTS (Dentro de los 60 bytes de i_block)
        let header = crate::fs::ext4::extents::Ext4ExtentHeader {
            eh_magic: 0xF30A,
            eh_entries: 1, // Tenemos 1 bloque de datos
            eh_max: 4,     // El inodo tiene espacio para 4 ramas
            eh_depth: 0,   // Profundidad 0 (apunta directamente a la data física)
            eh_generation: 0,
        };

        let extent = crate::fs::ext4::extents::Ext4Extent {
            ee_block: 0, // Bloque lógico 0 (el inicio del archivo)
            ee_len: 1,   // Ocupa 1 bloque consecutivo
            ee_start_hi: 0,
            ee_start_lo: physical_block, // ¡AQUÍ CONECTAMOS EL INODO CON TU BLOQUE RESERVADO!
        };

        unsafe {
            let i_block_ptr = inode.i_block.as_mut_ptr() as *mut u8;
            
            // Inyectamos el Header
            core::ptr::copy_nonoverlapping(
                &header as *const _ as *const u8,
                i_block_ptr,
                core::mem::size_of::<crate::fs::ext4::extents::Ext4ExtentHeader>()
            );
            // Inyectamos el Extent justo después del Header
            core::ptr::copy_nonoverlapping(
                &extent as *const _ as *const u8,
                i_block_ptr.add(core::mem::size_of::<crate::fs::ext4::extents::Ext4ExtentHeader>()),
                core::mem::size_of::<crate::fs::ext4::extents::Ext4Extent>()
            );
        }

        inode
    }


    pub fn write_inode(&self, inode_num: u32, inode: &Ext4Inode) -> Result<(), &'static str> {
        if inode_num < 1 || inode_num > self.super_block.s_inodes_count {
            return Err("Número de Inodo fuera de los límites del disco.");
        }

        let block_size = self.super_block.block_size();
        let inodes_per_group = self.super_block.s_inodes_per_group;
        let inode_size = 256; 

        // 1. Matemáticas para ubicar el inodo
        let group_index = (inode_num - 1) / inodes_per_group;
        let inode_index_in_group = (inode_num - 1) % inodes_per_group;

        // 2. Leer la Tabla de Descriptores de Grupo (BGD)
        let bgd_block_logic = self.bgd_block();
        let bgd_sector_start = self.start_lba + (bgd_block_logic * (block_size / 512));
        
        let mut bgd_buffer = [0u8; 512];
        self.device.read_block(bgd_sector_start, &mut bgd_buffer)?;

        let bgd_offset = (group_index as usize * core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()) % 512;
        let mut bgd: crate::fs::ext4::block_group::Ext4BlockGroupDescriptor = unsafe { core::mem::zeroed() };
        unsafe {
            core::ptr::copy_nonoverlapping(
                bgd_buffer.as_ptr().add(bgd_offset),
                &mut bgd as *mut crate::fs::ext4::block_group::Ext4BlockGroupDescriptor as *mut u8,
                core::mem::size_of::<crate::fs::ext4::block_group::Ext4BlockGroupDescriptor>()
            );
        }

        // 3. Localizar el sector exacto del Inodo
        let inode_table_block = { bgd.bg_inode_table_lo } as u64;
        let byte_offset_in_table = (inode_index_in_group as u64) * inode_size;
        let absolute_byte_offset = (inode_table_block * block_size) + byte_offset_in_table;
        
        let target_sector = self.start_lba + (absolute_byte_offset / 512);
        let offset_in_sector = (absolute_byte_offset % 512) as usize;

        // 4. READ-MODIFY-WRITE (Traer sector, modificar memoria, quemar sector)
        let mut sector_buffer = [0u8; 512];
        self.device.read_block(target_sector, &mut sector_buffer)?;

        unsafe {
            core::ptr::copy_nonoverlapping(
                inode as *const Ext4Inode as *const u8,
                sector_buffer.as_mut_ptr().add(offset_in_sector),
                core::mem::size_of::<Ext4Inode>()
            );
        }

        self.device.write_block(target_sector, &sector_buffer)?;

        crate::serial_println!("[EXT4] Metadatos actualizados en disco para Inodo {}", inode_num);
        Ok(())
    }

}