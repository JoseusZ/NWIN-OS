// src/fs/fat/volume.rs

use alloc::sync::Arc;
use alloc::string::String;
use crate::fs::BlockDevice;
use super::bpb::{Fat16BootSector, Fat32BootSector, DirectoryEntry, FatType};

#[allow(dead_code)]
pub struct FatVolume {
    pub device: Arc<dyn BlockDevice>, // Ahora es publico para que FatNode pueda usar el DMA
    pub start_lba: u64,
    pub fat_type: FatType,            // Ahora es publico para el VNode
}

impl FatVolume {
    pub fn mount(device: Arc<dyn BlockDevice>, start_lba: u64) -> Result<Self, &'static str> {
        let mut buffer = [0u8; 512];
        device.read_block(start_lba, &mut buffer)?;
        
        if buffer[510] != 0x55 || buffer[511] != 0xAA {
            return Err("Sector de arranque FAT invalido (Falta firma 0x55 0xAA)");
        }

        let table_size_16 = u16::from_le_bytes([buffer[22], buffer[23]]);
        
        let fat_type = if table_size_16 == 0 {
            let mut boot_sector: Fat32BootSector = unsafe { core::mem::zeroed() };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr(),
                    &mut boot_sector as *mut Fat32BootSector as *mut u8,
                    core::mem::size_of::<Fat32BootSector>()
                );
            }
            FatType::Fat32(boot_sector)
        } else {
            let mut boot_sector: Fat16BootSector = unsafe { core::mem::zeroed() };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr(),
                    &mut boot_sector as *mut Fat16BootSector as *mut u8,
                    core::mem::size_of::<Fat16BootSector>()
                );
            }
            FatType::Fat16(boot_sector)
        };

        Ok(FatVolume {
            device,
            start_lba,
            fat_type,
        })
    }

    pub fn root_dir_sector(&self) -> u64 {
        match &self.fat_type {
            FatType::Fat16(bs) => {
                let reserved = { bs.reserved_sector_count } as u64;
                let table_count = { bs.table_count } as u64;
                let table_size = { bs.table_size_16 } as u64;
                self.start_lba + reserved + (table_count * table_size)
            }
            FatType::Fat32(bs) => {
                let root_cluster = { bs.root_cluster };
                self.cluster_to_sector(root_cluster)
            }
        }
    }

    pub fn first_data_sector(&self) -> u64 {
        match &self.fat_type {
            FatType::Fat16(bs) => {
                let bytes_per_sector = { bs.bytes_per_sector } as u64;
                let root_entries = { bs.root_entry_count } as u64;
                let root_dir_sectors = ((root_entries * 32) + (bytes_per_sector - 1)) / bytes_per_sector;
                self.root_dir_sector() + root_dir_sectors
            }
            FatType::Fat32(_) => {
                self.root_dir_sector()
            }
        }
    }

    pub fn cluster_to_sector(&self, cluster: u32) -> u64 {
        let sectors_per_cluster = match &self.fat_type {
            FatType::Fat16(bs) => ({ bs.sectors_per_cluster }) as u64,
            FatType::Fat32(bs) => ({ bs.sectors_per_cluster }) as u64,
        };
        let data_start = self.first_data_sector();
        let cluster_offset = (cluster as u64 - 2) * sectors_per_cluster;
        data_start + cluster_offset
    }

    pub fn next_cluster(&self, current_cluster: u32) -> Option<u32> {
        let mut buffer = [0u8; 512];
        let (fat_start_sector, bytes_per_sector, is_fat32) = match &self.fat_type {
            FatType::Fat16(bs) => {
                let reserved = { bs.reserved_sector_count } as u64;
                (self.start_lba + reserved, { bs.bytes_per_sector } as u64, false)
            }
            FatType::Fat32(bs) => {
                let reserved = { bs.reserved_sector_count } as u64;
                (self.start_lba + reserved, { bs.bytes_per_sector } as u64, true)
            }
        };

        let byte_offset = if is_fat32 { (current_cluster as u64) * 4 } else { (current_cluster as u64) * 2 };
        let sector_offset = byte_offset / bytes_per_sector;
        let byte_in_sector = (byte_offset % bytes_per_sector) as usize;
        let target_sector = fat_start_sector + sector_offset;

        if self.device.read_block(target_sector, &mut buffer).is_err() {
            return None;
        }

        if is_fat32 {
            let next = u32::from_le_bytes([
                buffer[byte_in_sector], buffer[byte_in_sector + 1],
                buffer[byte_in_sector + 2], buffer[byte_in_sector + 3],
            ]);
            let next = next & 0x0FFFFFFF; 
            if next >= 0x0FFFFFF8 { None } else { Some(next) }
        } else {
            let next = u16::from_le_bytes([
                buffer[byte_in_sector], buffer[byte_in_sector + 1],
            ]) as u32;
            if next >= 0xFFF8 { None } else { Some(next) }
        }
    }

    pub fn debug_info(&self) {
        crate::serial_println!("=== INFO VOLUMEN FAT ===");
        match &self.fat_type {
            FatType::Fat16(bs) => {
                crate::serial_println!("-> TIPO DETECTADO: FAT16");
                crate::serial_println!("Bytes por sector: {}", { bs.bytes_per_sector });
                crate::serial_println!("Sectores por cluster: {}", { bs.sectors_per_cluster });
                crate::serial_println!("Entradas maximas en raiz: {}", { bs.root_entry_count });
            }
            FatType::Fat32(bs) => {
                crate::serial_println!("-> TIPO DETECTADO: FAT32");
                crate::serial_println!("Bytes por sector: {}", { bs.bytes_per_sector });
                crate::serial_println!("Sectores por cluster: {}", { bs.sectors_per_cluster });
                crate::serial_println!("Cluster de la carpeta raiz: {}", { bs.root_cluster });
            }
        }
        crate::serial_println!("-> SECTOR FÍSICO RAÍZ: {}", self.root_dir_sector());
        crate::serial_println!("-> INICIO ZONA DE DATOS: {}", self.first_data_sector());
        crate::serial_println!("========================");
    }

    pub fn list_root_dir(&self) {
        let mut buffer = [0u8; 512];
        let root_sector = self.root_dir_sector();
        
        if self.device.read_block(root_sector, &mut buffer).is_err() {
            crate::serial_println!("[VFS] Error leyendo el sector raiz del disco.");
            return;
        }

        let entries = unsafe {
            core::slice::from_raw_parts(buffer.as_ptr() as *const DirectoryEntry, 16)
        };

        crate::serial_println!("=== CONTENIDO DE LA CARPETA RAIZ (/) ===");
        
        for entry in entries {
            let first_byte = entry.name[0];
            if first_byte == 0x00 { break; }
            if first_byte == 0xE5 || entry.attributes == 0x0F || (entry.attributes & 0x08) != 0 { continue; }

            let mut name_str = String::new();
            for i in 0..8 { if entry.name[i] != b' ' { name_str.push(entry.name[i] as char); } }
            if entry.name[8] != b' ' {
                name_str.push('.');
                for i in 8..11 { if entry.name[i] != b' ' { name_str.push(entry.name[i] as char); } }
            }

            let is_dir = (entry.attributes & 0x10) != 0;
            let file_size = { entry.file_size };
            let cluster_low = { entry.first_cluster_low };
            let cluster_high = { entry.first_cluster_high };
            let start_cluster = ((cluster_high as u32) << 16) | (cluster_low as u32);

            if is_dir {
                crate::serial_println!("[DIR]  {} (Cluster Base: {})", name_str, start_cluster);
            } else {
                let mut current = start_cluster;
                let mut fragmentos = 1;
                while let Some(next) = self.next_cluster(current) {
                    fragmentos += 1;
                    current = next;
                    if fragmentos > 20000 { break; } 
                }
                crate::serial_println!(
                    "[FILE] {} - {} bytes | Inicia en Clúster: {} | Ocupa {} clústeres", 
                    name_str, file_size, start_cluster, fragmentos
                );
            }
        }
        crate::serial_println!("========================================");
    }
}