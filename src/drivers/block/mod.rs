// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/drivers/block/mod.rs

pub mod ahci;

use crate::drivers::block::ahci::HbaMemory; 
use crate::fs::BlockDevice;

pub struct AhciDisk {
    pub port_index: usize,
    pub bar5_virt: u64,
}

impl BlockDevice for AhciDisk {
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
        if buffer.len() < 512 {
            return Err("El buffer del VFS es muy pequeño para un sector");
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo HHDM").offset;
        
        // 1. Pedimos RAM limpia al SystemFrameAllocator para el DMA
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or("Sin memoria fisica para DMA")?;
            
        let frame_virt = hhdm_offset + frame_phys;

        // 2. Recuperamos el acceso al puerto SATA
        let hba_mem = unsafe { &mut *(self.bar5_virt as *mut HbaMemory) };
        let port = &mut hba_mem.ports[self.port_index];

        // 3. Le pedimos al láser físico que lea el disco
        let success = port.read_sector(lba, frame_phys, hhdm_offset);

        if success {
            // 4. Copiamos los datos mágicos del hardware al buffer del VFS
            let hardware_data = unsafe { 
                core::slice::from_raw_parts(frame_virt as *const u8, buffer.len()) 
            };
            buffer.copy_from_slice(hardware_data);
            
            // 5. IMPORTANTE: Devolvemos el marco físico para no agotar la RAM
            crate::mm::memory::get_allocator().deallocate_frame(frame_phys);
            
            Ok(())
        } else {
            crate::mm::memory::get_allocator().deallocate_frame(frame_phys);
            Err("Fallo de hardware al leer el disco")
        }
    }

    fn write_block(&self, lba: u64, buffer: &[u8]) -> Result<(), &'static str> {
        if buffer.len() > 4096 {
            return Err("El buffer excede el tamaño de la pagina física (4096 bytes)");
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo HHDM").offset;
        
        // 1. Pedimos RAM limpia (Bounce Buffer) para el DMA de escritura
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or("Sin memoria fisica para DMA de escritura")?;
            
        let frame_virt = hhdm_offset + frame_phys;

        // 2. Copiamos los datos del VFS a nuestra memoria física de tránsito ANTES de avisarle al disco
        let hardware_buffer = unsafe { 
            core::slice::from_raw_parts_mut(frame_virt as *mut u8, buffer.len()) 
        };
        hardware_buffer.copy_from_slice(buffer);

        // 3. Recuperamos el acceso al puerto SATA
        let hba_mem = unsafe { &mut *(self.bar5_virt as *mut HbaMemory) };
        let port = &mut hba_mem.ports[self.port_index];

        // 4. ¡Disparamos la escritura física al plato magnético!
        let success = port.write_sector(lba, frame_phys, hhdm_offset);

        // 5. Limpiamos la RAM de tránsito para evitar fugas de memoria
        crate::mm::memory::get_allocator().deallocate_frame(frame_phys);

        if success {
            Ok(())
        } else {
            Err("Fallo de hardware al escribir en el disco")
        }
    }
}