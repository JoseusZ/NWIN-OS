// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/drivers/block/mod.rs

pub mod ahci;

use crate::drivers::block::ahci::HbaMemory;
use crate::fs::BlockDevice;

// ============================================================================
// `DriverError` — Taxonomia de fallos para la capa de drivers de bloque (Paso 3.6)
// ============================================================================
//
// Motivacion: NWIN OS apunta a HARDWARE REAL. En silicio real (vs QEMU),
// los fallos de controladores AHCI/NVMe/USB son eventos de primera clase:
// necesitamos tipado estricto para implementar telemetria, reintentos
// exponenciales, y propagacion tipada hacia el VFS.
//
// Variantes (alineadas con los modos de fallo fisicos mas comunes):
// - `IoFailure`            -- Operacion de E/S fallo a nivel DMA/PIO.
// - `DeviceNotFound`       -- LUN o dispositivo concreto ausente.
// - `ControllerMissing`    -- HBA entera no responde en PCI.
// - `Timeout`              -- La controladora no completo la op. en N ms.
// - `UnsupportedProtocol`  -- Revision de protocolo no soportada por el driver.
// - `BufferTooSmall`       -- El buffer del llamante no cabe un sector.
// - `BufferTooLarge`       -- El buffer del llamante excede una pagina fisica.
// - `NoDmaMemory`          -- Sin marcos fisicos para el bounce buffer.
//
// Politica de Display: prefijo "DRV:" para que un dmesg denso sea
// identificable al primer vistazo.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    IoFailure,
    DeviceNotFound,
    ControllerMissing,
    Timeout,
    UnsupportedProtocol,
    BufferTooSmall,
    BufferTooLarge,
    NoDmaMemory,
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DriverError::IoFailure           => write!(f, "DRV:IoFailure (DMA/PIO operation failed)"),
            DriverError::DeviceNotFound      => write!(f, "DRV:DeviceNotFound (LUN/id mismatch)"),
            DriverError::ControllerMissing   => write!(f, "DRV:ControllerMissing (HBA unresponsive on PCI)"),
            DriverError::Timeout             => write!(f, "DRV:Timeout (controller did not complete in time)"),
            DriverError::UnsupportedProtocol => write!(f, "DRV:UnsupportedProtocol (rev. not handled by driver)"),
            DriverError::BufferTooSmall      => write!(f, "DRV:BufferTooSmall (caller buffer cannot hold a sector)"),
            DriverError::BufferTooLarge      => write!(f, "DRV:BufferTooLarge (caller buffer exceeds a physical page)"),
            DriverError::NoDmaMemory         => write!(f, "DRV:NoDmaMemory (no free frames for bounce buffer)"),
        }
    }
}

impl core::error::Error for DriverError {
    // DriverError no envuelve errores externos (todas las variantes son unitarias).
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

pub struct AhciDisk {
    pub port_index: usize,
    pub bar5_virt: u64,
}

impl BlockDevice for AhciDisk {
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), DriverError> {
        // Paso 3.7b: la firma del trait exige `DriverError`. El mapeo
        // a `FsError` se hace via el puente `From<DriverError> for FsError`
        // declarado en `src/core/error.rs` (Paso 3.7a). Los call sites
        // FS siguen compilando sin cambios porque ese puente resuelve
        // automaticamente el `?`.
        if buffer.len() < 512 {
            return Err(DriverError::BufferTooSmall);
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo HHDM").offset;

        // 1. Pedimos RAM limpia al SystemFrameAllocator para el DMA
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or(DriverError::NoDmaMemory)?;
            
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
            Err(DriverError::IoFailure)
        }
    }

    fn write_block(&self, lba: u64, buffer: &[u8]) -> Result<(), DriverError> {
        // Paso 3.7b: misma justificacion que read_block.
        if buffer.len() > 4096 {
            return Err(DriverError::BufferTooLarge);
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo HHDM").offset;

        // 1. Pedimos RAM limpia (Bounce Buffer) para el DMA de escritura
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or(DriverError::NoDmaMemory)?;

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
            Err(DriverError::IoFailure)
        }
    }
}