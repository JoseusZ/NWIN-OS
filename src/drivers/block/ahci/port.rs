// src/drivers/block/ahci/port.rs

use super::regs::{Volatile, HbaPortCmd};

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

#[repr(C)]
pub struct HbaCmdHeader {
    pub flags: Volatile<u16>, 
    pub prdtl: Volatile<u16>, 
    pub prdbc: Volatile<u32>, 
    pub ctba: Volatile<u32>,  
    pub ctbau: Volatile<u32>, 
    pub _reserved: [u32; 4],  
}

#[repr(C)]
pub struct HbaPrdtEntry {
    pub dba: Volatile<u32>,   
    pub dbau: Volatile<u32>,  
    pub _reserved: u32,
    pub flags: Volatile<u32>, 
}

#[repr(C)]
pub struct HbaCmdTbl {
    pub cfis: [u8; 64],       
    pub acmd: [u8; 16],       
    pub _reserved: [u8; 48],
    pub prdt_entry: [HbaPrdtEntry; 1], 
}

impl HbaPort {
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
                crate::serial_println!("[AHCI] ERROR: El puerto está colgado (Hung).");
                return false;
            }
        }

        self.ci.write(1 << slot);

        loop {
            if (self.ci.read() & (1 << slot)) == 0 { break; }
            if (self.is.read() & (1 << 30)) != 0 {
                crate::serial_println!("[AHCI] ERROR DE LECTURA (Disk Error).");
                return false;
            }
            core::hint::spin_loop();
        }

        true
    }

    pub fn write_sector(&mut self, lba: u64, buffer_phys_addr: u64, hhdm_offset: u64) -> bool {
        // 1. Limpiamos interrupciones previas
        self.is.write(0xFFFFFFFF);

        let slot = 0;

        let clb_phys = (self.clbu.read() as u64) << 32 | (self.clb.read() as u64);
        let clb_virt = clb_phys + hhdm_offset;
        let headers = unsafe { core::slice::from_raw_parts_mut(clb_virt as *mut HbaCmdHeader, 32) };
        let header = &mut headers[slot];

        // DIFERENCIA CLAVE 1: Banderas de la cabecera
        // 5 DWORDS de longitud (FIS) | Bit 6 encendido (0x40) que significa "WRITE" (Host to Device)
        // 5 | 0x40 = 0x45 (69 en decimal)
        header.flags.write(0x45); 
        header.prdtl.write(1); 
        header.prdbc.write(0);

        let ctba_phys = (header.ctbau.read() as u64) << 32 | (header.ctba.read() as u64);
        let ctba_virt = ctba_phys + hhdm_offset;
        let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTbl) };

        unsafe { core::ptr::write_bytes(cmd_tbl as *mut HbaCmdTbl as *mut u8, 0, core::mem::size_of::<HbaCmdTbl>()); }

        // Apuntamos al buffer en memoria RAM que contiene los datos a escribir
        cmd_tbl.prdt_entry[0].dba.write((buffer_phys_addr & 0xFFFFFFFF) as u32);
        cmd_tbl.prdt_entry[0].dbau.write((buffer_phys_addr >> 32) as u32);
        cmd_tbl.prdt_entry[0].flags.write(511); 

        // ARMAMOS EL PAQUETE FIS (Command)
        cmd_tbl.cfis[0] = 0x27; // Registro Host to Device
        cmd_tbl.cfis[1] = 0x80; // Flag de Comando
        
        // DIFERENCIA CLAVE 2: Comando Write DMA Extended
        cmd_tbl.cfis[2] = 0x35; 

        // Dirección LBA (Sector que queremos sobrescribir)
        cmd_tbl.cfis[4] = (lba & 0xFF) as u8;
        cmd_tbl.cfis[5] = ((lba >> 8) & 0xFF) as u8;
        cmd_tbl.cfis[6] = ((lba >> 16) & 0xFF) as u8;
        cmd_tbl.cfis[7] = 0x40; // LBA mode
        
        cmd_tbl.cfis[8] = ((lba >> 24) & 0xFF) as u8;
        cmd_tbl.cfis[9] = ((lba >> 32) & 0xFF) as u8;
        cmd_tbl.cfis[10] = ((lba >> 40) & 0xFF) as u8;

        // Escribiremos 1 sector (512 bytes)
        cmd_tbl.cfis[12] = 1; 
        cmd_tbl.cfis[13] = 0; 

        // Esperar a que el puerto deje de estar ocupado
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

    pub fn start_cmd(&mut self) {
        while HbaPortCmd::from_bits_truncate(self.cmd.read()).contains(HbaPortCmd::CR) {
            core::hint::spin_loop();
        }

        let mut cmd = HbaPortCmd::from_bits_truncate(self.cmd.read());
        cmd.insert(HbaPortCmd::FRE | HbaPortCmd::ST);
        self.cmd.write(cmd.bits());
    }
}