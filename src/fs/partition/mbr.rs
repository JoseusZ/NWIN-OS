// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/fs/mbr.rs

use crate::fs::BlockDevice;
use alloc::sync::Arc;

// =====================================================================
// CONSTANTES DE TIPOS DE PARTICIÓN (PARTITION TYPES)
// =====================================================================
pub const PART_TYPE_EMPTY: u8 = 0x00;
pub const PART_TYPE_FAT16_CHS: u8 = 0x06;
pub const PART_TYPE_FAT32_CHS: u8 = 0x0B;
pub const PART_TYPE_FAT32_LBA: u8 = 0x0C;
pub const PART_TYPE_FAT16: u8 = 0x0E;
pub const PART_TYPE_LINUX: u8 = 0x83;
pub const PART_TYPE_GPT_PROTECTIVE: u8 = 0xEE; // Partición nativa de Linux (ext2/3/4)pub const PART_TYPE_GPT_PROTECTIVE: u8 = 0xEE; // Crucial para la futura compatibilidad GPT

// =====================================================================
// ESTRUCTURAS MBR
// =====================================================================

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct PartitionEntry {
    pub bootable: u8,
    pub start_chs: [u8; 3],
    pub partition_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,     // ¡EL DATO MÁS IMPORTANTE! Sector donde inicia el FS
    pub total_sectors: u32,
}

impl PartitionEntry {
    /// Indica si la entrada de la partición está vacía.
    pub fn is_empty(&self) -> bool {
        self.partition_type == PART_TYPE_EMPTY
    }

    /// Verifica si la partición está formateada en FAT32 o FAT16.
    pub fn is_fat(&self) -> bool {
        self.partition_type == PART_TYPE_FAT32_CHS 
        || self.partition_type == PART_TYPE_FAT32_LBA
        || self.partition_type == PART_TYPE_FAT16_CHS
        || self.partition_type == PART_TYPE_FAT16
    }

    
    pub fn is_linux(&self) -> bool {
        self.partition_type == PART_TYPE_LINUX
    }
}

#[derive(Debug)]
pub struct Mbr {
    pub partitions: [PartitionEntry; 4],
}

impl Mbr {
    /// Lee el Sector 0 de un disco, verifica firmas y decodifica la tabla de particiones.
    pub fn read_from(disk: Arc<dyn BlockDevice>) -> Result<Self, &'static str> {
        let mut buffer = [0u8; 512];
        
        disk.read_block(0, &mut buffer)?;
        
        // Verificamos la firma mágica de arranque (Boot Signature)
        if buffer[510] != 0x55 || buffer[511] != 0xAA {
            return Err("Firma MBR invalida (No es 0x55 0xAA)");
        }

        // La tabla de particiones empieza exactamente en el byte 446 del Sector 0
        let mut partitions: [PartitionEntry; 4] = unsafe { core::mem::zeroed() };
        
        for i in 0..4 {
            let offset = 446 + (i * 16);
            let entry_bytes = &buffer[offset..offset + 16];
            
            // Copiamos los bytes crudos a nuestra estructura de Rust
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry_bytes.as_ptr(),
                    &mut partitions[i] as *mut PartitionEntry as *mut u8,
                    16
                );
            }
        }

        Ok(Mbr { partitions })
    }

    /// Determina si este disco utiliza el esquema GPT moderno.
    /// Si es así, el kernel deberá ignorar las particiones MBR y leer el LBA 1.
    pub fn is_gpt_protective(&self) -> bool {
        // La especificación UEFI dicta que un disco GPT debe tener una única
        // partición de tipo 0xEE en el MBR ocupando todo el disco virtualmente.
        !self.partitions[0].is_empty() && self.partitions[0].partition_type == PART_TYPE_GPT_PROTECTIVE
    }
}