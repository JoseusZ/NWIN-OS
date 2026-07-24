// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Block-device driver layer: AHCI driver bring-up, the
//! [`AhciDisk`] adapter that implements the VFS [`BlockDevice`]
//! trait, and the [`DriverError`] taxonomy used by every block
//! driver.

pub mod ahci;

use crate::drivers::block::ahci::HbaMemory;
use crate::fs::BlockDevice;

// ============================================================================
// `DriverError` — Failure taxonomy for the block-driver layer (Phase 3.6)
// ============================================================================
//
// Motivation: NWIN OS targets REAL HARDWARE. On real silicon (vs QEMU),
// AHCI / NVMe / USB controller failures are first-class events:
// we need strict typing to enable telemetry, exponential backoff
// retries, and typed propagation into the VFS.
//
// Variants (aligned with the most common physical failure modes):
// - `IoFailure`            -- I/O operation failed at the DMA/PIO level.
// - `DeviceNotFound`       -- specific LUN or device is missing.
// - `ControllerMissing`    -- entire HBA is unresponsive on PCI.
// - `Timeout`              -- the controller did not complete the op. in N ms.
// - `UnsupportedProtocol`  -- protocol revision not handled by the driver.
// - `BufferTooSmall`       -- the caller buffer cannot hold a sector.
// - `BufferTooLarge`       -- the caller buffer exceeds one physical page.
// - `NoDmaMemory`          -- no free physical frames for the bounce buffer.
//
// Display policy: "DRV:" prefix so a dense dmesg is identifiable at
// first glance.

/// Failure taxonomy for the block-driver layer.
///
/// Aligned with the most common physical failure modes of AHCI /
/// NVMe / USB controllers. Translates into [`crate::core::error::FsError`]
/// at the VFS boundary via the `From<DriverError> for FsError` impl
/// in `core/error.rs`.
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
    // DriverError does not wrap external errors (all variants are unit).
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

/// Adapter that exposes a single AHCI port to the VFS as a
/// [`BlockDevice`].
///
/// Each detected SATA disk is wrapped in one of these and handed to
/// the FS manager during AHCI bring-up.
pub struct AhciDisk {
    pub port_index: usize,
    pub bar5_virt: u64,
}

impl BlockDevice for AhciDisk {
    /// Reads one logical block of the disk into `buffer` using a
    /// single-sector AHCI DMA command.
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), DriverError> {
        // Phase 3.7b: the trait signature requires `DriverError`.
        // The mapping to `FsError` is performed by the
        // `From<DriverError> for FsError` bridge declared in
        // `src/core/error.rs` (Phase 3.7a). FS call sites keep
        // compiling unchanged because the bridge resolves the `?`
        // automatically.
        if buffer.len() < 512 {
            return Err(DriverError::BufferTooSmall);
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("HHDM failed").offset;

        // 1. Ask the SystemFrameAllocator for clean RAM for the DMA bounce buffer.
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or(DriverError::NoDmaMemory)?;

        let frame_virt = hhdm_offset + frame_phys;

        // 2. Recover the access to the SATA port.
        let hba_mem = unsafe { &mut *(self.bar5_virt as *mut HbaMemory) };
        let port = &mut hba_mem.ports[self.port_index];

        // 3. Ask the physical laser to read the disk.
        let success = port.read_sector(lba, frame_phys, hhdm_offset);

        if success {
            // 4. Copy the hardware bytes into the VFS buffer.
            let hardware_data = unsafe {
                core::slice::from_raw_parts(frame_virt as *const u8, buffer.len())
            };
            buffer.copy_from_slice(hardware_data);

            // 5. IMPORTANT: return the physical frame so we do not exhaust RAM.
            crate::mm::memory::get_allocator().deallocate_frame(frame_phys);

            Ok(())
        } else {
            crate::mm::memory::get_allocator().deallocate_frame(frame_phys);
            Err(DriverError::IoFailure)
        }
    }

    /// Writes one logical block of the disk from `buffer` using a
    /// single-sector AHCI DMA command.
    fn write_block(&self, lba: u64, buffer: &[u8]) -> Result<(), DriverError> {
        // Phase 3.7b: same justification as read_block.
        if buffer.len() > 4096 {
            return Err(DriverError::BufferTooLarge);
        }

        let hhdm_offset = crate::HHDM_REQUEST.response().expect("HHDM failed").offset;

        // 1. Ask for clean RAM (bounce buffer) for the write DMA.
        let frame_phys = crate::mm::memory::get_allocator()
            .allocate_contiguous_frames(1)
            .ok_or(DriverError::NoDmaMemory)?;

        let frame_virt = hhdm_offset + frame_phys;

        // 2. Copy the VFS data into the transit physical memory BEFORE notifying the disk.
        let hardware_buffer = unsafe { 
            core::slice::from_raw_parts_mut(frame_virt as *mut u8, buffer.len()) 
        };
        hardware_buffer.copy_from_slice(buffer);

        // 3. Recover the access to the SATA port.
        let hba_mem = unsafe { &mut *(self.bar5_virt as *mut HbaMemory) };
        let port = &mut hba_mem.ports[self.port_index];

        // 4. Fire the physical write at the magnetic platter!
        let success = port.write_sector(lba, frame_phys, hhdm_offset);

        // 5. Free the transit RAM to avoid memory leaks.
        crate::mm::memory::get_allocator().deallocate_frame(frame_phys);

        if success {
            Ok(())
        } else {
            Err(DriverError::IoFailure)
        }
    }
}