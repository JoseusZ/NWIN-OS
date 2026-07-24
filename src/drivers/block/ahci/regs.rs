// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AHCI register layout and ATA command opcodes.
//!
//! Pure data definitions: a `Volatile<T>` wrapper around MMIO
//! registers, the HBA control/status bitflags and the subset of ATA
//! commands the driver issues. Runtime logic lives in [`super::port`].

use core::ptr::{read_volatile, write_volatile};

/// Transparent wrapper that forces every access to go through
/// `read_volatile`/`write_volatile`, preventing the compiler from
/// reordering or coalescing MMIO operations.
#[repr(transparent)]
pub struct Volatile<T>(T);

impl<T> Volatile<T> {
    /// Reads the wrapped value through a volatile load.
    pub fn read(&self) -> T {
        unsafe { read_volatile(&self.0) }
    }

    /// Writes `value` through a volatile store.
    pub fn write(&mut self, value: T) {
        unsafe { write_volatile(&mut self.0, value) }
    }
}

// Bits of the HBA host control register (`HBA_MEM::ghc`).
bitflags::bitflags! {
    pub struct HbaHostCont: u32 {
        const HR =   1 << 0;  // HBA Reset
        const IE =   1 << 1;  // Interrupt Enable
        const MRSM = 1 << 2;  // MSI Revert to Single Message
        const AE =   1 << 31; // AHCI Enable
    }
}

// Bits of the per-port command register (`HBA_PORT::cmd`).
bitflags::bitflags! {
    pub struct HbaPortCmd: u32 {
        const ST =  1 << 0;  // Start
        const SUD = 1 << 1;  // Spin-Up Device
        const POD = 1 << 2;  // Power On Device
        const CLO = 1 << 3;  // Command List Override
        const FRE = 1 << 4;  // FIS Receive Enable
        const FR =  1 << 14; // FIS Receive Running
        const CR =  1 << 15; // Command List Running
    }
}

/// Subset of ATA command opcodes issued by the AHCI driver.
#[derive(Debug, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum AtaCommand {
    ReadDma = 0xC8,
    ReadDmaExt = 0x25,
    WriteDma = 0xCA,
    WriteDmaExt = 0x35,
    IdentifyDevice = 0xEC,
}