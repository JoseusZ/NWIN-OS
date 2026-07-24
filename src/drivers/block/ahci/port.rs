// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AHCI per-port structures, command headers, PRDT and the
//! read/write sector primitives that issue DMA commands to a
//! SATA device attached to an HBA port.

use super::regs::{Volatile, HbaPortCmd};

/// Per-port register file of an HBA, 128 bytes wide.
///
/// Field naming follows the AHCI 1.3.1 specification exactly:
/// `clb/clbu` is the Command List Base address, `fb/fbu` the FIS
/// Base, `cmd` the port command register, `tfd` the task file data
/// (last command status), `ci` the command issue bitmask and so on.
#[repr(C)]
pub struct HbaPort {
    pub clb: Volatile<u32>,
    pub clbu: Volatile<u32>,
    pub fb: Volatile<u32>,
    pub fbu: Volatile<u32>,
    pub is: Volatile<u32>,
    pub ie: Volatile<u32>,
    pub cmd: Volatile<u32>,
    pub _reserved: u32,
    pub tfd: Volatile<u32>,
    pub sig: Volatile<u32>,
    pub ssts: Volatile<u32>,
    pub sctl: Volatile<u32>,
    pub serr: Volatile<u32>,
    pub sact: Volatile<u32>,
    pub ci: Volatile<u32>,
    pub sntf: Volatile<u32>,
    pub fbs: Volatile<u32>,
    pub devslp: Volatile<u32>,
    pub _reserved_1: [u32; 10],
    pub vendor: [u32; 4],
}

/// One of the 32 command slots inside the HBA Command List.
#[repr(C)]
pub struct HbaCmdHeader {
    pub flags: Volatile<u16>,
    pub prdtl: Volatile<u16>,
    pub prdbc: Volatile<u32>,
    pub ctba: Volatile<u32>,
    pub ctbau: Volatile<u32>,
    pub _reserved: [u32; 4],
}

/// Physical Region Descriptor Table entry: describes one contiguous
/// data buffer in physical memory for a DMA command.
#[repr(C)]
pub struct HbaPrdtEntry {
    pub dba: Volatile<u32>,
    pub dbau: Volatile<u32>,
    pub _reserved: u32,
    pub flags: Volatile<u32>,
}

/// Command Table pointed at by a [`HbaCmdHeader`]: holds the Command
/// FIS, the ATAPI command (unused for plain SATA) and the PRDT.
#[repr(C)]
pub struct HbaCmdTbl {
    pub cfis: [u8; 64],
    pub acmd: [u8; 16],
    pub _reserved: [u8; 48],
    pub prdt_entry: [HbaPrdtEntry; 1],
}

impl HbaPort {
    /// Stops the port's command engine and waits for `CR`/`FR` in
    /// the command register to clear.
    pub fn stop_cmd(&mut self) {
        let mut cmd = HbaPortCmd::from_bits_truncate(self.cmd.read());
        cmd.remove(HbaPortCmd::ST | HbaPortCmd::FRE);
        self.cmd.write(cmd.bits());

        loop {
            let current_cmd = HbaPortCmd::from_bits_truncate(self.cmd.read());
            if !current_cmd.intersects(HbaPortCmd::CR | HbaPortCmd::FR) { break; }
            core::hint::spin_loop();
        }
    }

    /// Issues a single-sector DMA READ on the port at the given LBA
    /// into the physical buffer described by `buffer_phys_addr`.
    ///
    /// `hhdm_offset` is the Higher-Half Direct Mapping offset the
    /// kernel uses to convert physical addresses into kernel virtual
    /// addresses. Returns `false` if the port hangs or reports a
    /// disk error.
    pub fn read_sector(&mut self, lba: u64, buffer_phys_addr: u64, hhdm_offset: u64) -> bool {
        self.is.write(0xFFFFFFFF);
        let slot = 0;
        
        let clb_phys = (self.clbu.read() as u64) << 32 | (self.clb.read() as u64);
        let clb_virt = clb_phys + hhdm_offset;
        let headers = unsafe { core::slice::from_raw_parts_mut(clb_virt as *mut HbaCmdHeader, 32) };
        let header = &mut headers[slot];

        header.flags.write(5); 
        header.prdtl.write(1); 
        header.prdbc.write(0);

        let ctba_phys = (header.ctbau.read() as u64) << 32 | (header.ctba.read() as u64);
        let ctba_virt = ctba_phys + hhdm_offset;
        let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTbl) };

        unsafe { core::ptr::write_bytes(cmd_tbl as *mut HbaCmdTbl as *mut u8, 0, core::mem::size_of::<HbaCmdTbl>()); }

        cmd_tbl.prdt_entry[0].dba.write((buffer_phys_addr & 0xFFFFFFFF) as u32);
        cmd_tbl.prdt_entry[0].dbau.write((buffer_phys_addr >> 32) as u32);
        cmd_tbl.prdt_entry[0].flags.write(511); 

        cmd_tbl.cfis[0] = 0x27; 
        cmd_tbl.cfis[1] = 0x80; 
        cmd_tbl.cfis[2] = 0x25; 

        cmd_tbl.cfis[4] = (lba & 0xFF) as u8;
        cmd_tbl.cfis[5] = ((lba >> 8) & 0xFF) as u8;
        cmd_tbl.cfis[6] = ((lba >> 16) & 0xFF) as u8;
        cmd_tbl.cfis[7] = 0x40; 
        
        cmd_tbl.cfis[8] = ((lba >> 24) & 0xFF) as u8;
        cmd_tbl.cfis[9] = ((lba >> 32) & 0xFF) as u8;
        cmd_tbl.cfis[10] = ((lba >> 40) & 0xFF) as u8;

        cmd_tbl.cfis[12] = 1; 
        cmd_tbl.cfis[13] = 0; 

        let mut spin = 0;
        while (self.tfd.read() & (0x80 | 0x08)) != 0 {
            core::hint::spin_loop();
            spin += 1;
            if spin > 1_000_000 {
                crate::serial_println!("[AHCI] ERROR: Port is hung.");
                return false;
            }
        }

        self.ci.write(1 << slot);

        loop {
            if (self.ci.read() & (1 << slot)) == 0 { break; }
            if (self.is.read() & (1 << 30)) != 0 {
                crate::serial_println!("[AHCI] WRITE ERROR (Disk Error).");
                return false;
            }
            core::hint::spin_loop();
        }

        true
    }

    /// Issues a single-sector DMA WRITE on the port at the given LBA
    /// from the physical buffer described by `buffer_phys_addr`.
    ///
    /// Mirrors [`read_sector`]; the only differences are the `0x45`
    /// command-flag bitmask (bit 6 = write) and the `0x35` opcode
    /// (`WRITE DMA EXT`) inside the Command FIS.
    pub fn write_sector(&mut self, lba: u64, buffer_phys_addr: u64, hhdm_offset: u64) -> bool {
        // 1. Clear any pending interrupts.
        self.is.write(0xFFFFFFFF);

        let slot = 0;

        let clb_phys = (self.clbu.read() as u64) << 32 | (self.clb.read() as u64);
        let clb_virt = clb_phys + hhdm_offset;
        let headers = unsafe { core::slice::from_raw_parts_mut(clb_virt as *mut HbaCmdHeader, 32) };
        let header = &mut headers[slot];

        // KEY DIFFERENCE 1: command-list header flags.
        // 5 DWORDs of FIS length | bit 6 (0x40) set = "WRITE" (Host to Device).
        // 5 | 0x40 = 0x45 (69 in decimal).
        header.flags.write(0x45);
        header.prdtl.write(1);
        header.prdbc.write(0);

        let ctba_phys = (header.ctbau.read() as u64) << 32 | (header.ctba.read() as u64);
        let ctba_virt = ctba_phys + hhdm_offset;
        let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTbl) };

        unsafe { core::ptr::write_bytes(cmd_tbl as *mut HbaCmdTbl as *mut u8, 0, core::mem::size_of::<HbaCmdTbl>()); }

        // Point at the RAM buffer that holds the data to write.
        cmd_tbl.prdt_entry[0].dba.write((buffer_phys_addr & 0xFFFFFFFF) as u32);
        cmd_tbl.prdt_entry[0].dbau.write((buffer_phys_addr >> 32) as u32);
        cmd_tbl.prdt_entry[0].flags.write(511);

        // Build the Command FIS.
        cmd_tbl.cfis[0] = 0x27; // Host to Device register.
        cmd_tbl.cfis[1] = 0x80; // Command flag.

        // KEY DIFFERENCE 2: Write DMA Extended opcode.
        cmd_tbl.cfis[2] = 0x35;

        // LBA address (sector we want to overwrite).
        cmd_tbl.cfis[4] = (lba & 0xFF) as u8;
        cmd_tbl.cfis[5] = ((lba >> 8) & 0xFF) as u8;
        cmd_tbl.cfis[6] = ((lba >> 16) & 0xFF) as u8;
        cmd_tbl.cfis[7] = 0x40; // LBA mode

        cmd_tbl.cfis[8] = ((lba >> 24) & 0xFF) as u8;
        cmd_tbl.cfis[9] = ((lba >> 32) & 0xFF) as u8;
        cmd_tbl.cfis[10] = ((lba >> 40) & 0xFF) as u8;

        // We will write 1 sector (512 bytes).
        cmd_tbl.cfis[12] = 1;
        cmd_tbl.cfis[13] = 0;

        // Wait until the port is no longer busy.
        let mut spin = 0;
        while (self.tfd.read() & (0x80 | 0x08)) != 0 {
            core::hint::spin_loop();
            spin += 1;
            if spin > 1_000_000 {
                crate::serial_println!("[AHCI] ERROR: El puerto está colgado en escritura (Hung).");
                return false;
            }
        }

        // Disparamos la orden
        self.ci.write(1 << slot);

        // Esperamos confirmación
        loop {
            if (self.ci.read() & (1 << slot)) == 0 {
                break;
            }
            if (self.is.read() & (1 << 30)) != 0 {
                crate::serial_println!("[AHCI] ERROR DE ESCRITURA (Disk Error).");
                return false;
            }
            core::hint::spin_loop();
        }

        true
    }

    /// Starts the port's command engine by setting `FRE|ST` in the
    /// command register, after waiting for any prior command to
    /// drain.
    pub fn start_cmd(&mut self) {
        while HbaPortCmd::from_bits_truncate(self.cmd.read()).contains(HbaPortCmd::CR) {
            core::hint::spin_loop();
        }

        let mut cmd = HbaPortCmd::from_bits_truncate(self.cmd.read());
        cmd.insert(HbaPortCmd::FRE | HbaPortCmd::ST);
        self.cmd.write(cmd.bits());
    }
}