// src/drivers/block/ahci/regs.rs

use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
pub struct Volatile<T>(T);

impl<T> Volatile<T> {
    pub fn read(&self) -> T {
        unsafe { read_volatile(&self.0) }
    }
    pub fn write(&mut self, value: T) {
        unsafe { write_volatile(&mut self.0, value) }
    }
}

bitflags::bitflags! {
    pub struct HbaHostCont: u32 {
        const HR =   1 << 0;  // HBA Reset
        const IE =   1 << 1;  // Interrupt Enable
        const MRSM = 1 << 2;  // MSI Revert to Single Message
        const AE =   1 << 31; // AHCI Enable
    }
}

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

#[derive(Debug, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum AtaCommand {
    ReadDma = 0xC8,
    ReadDmaExt = 0x25,
    WriteDma = 0xCA,
    WriteDmaExt = 0x35,
    IdentifyDevice = 0xEC,
}