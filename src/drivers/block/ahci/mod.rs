// src/drivers/block/ahci.rs

pub mod regs;
pub mod port;

use regs::{Volatile, HbaHostCont};
use port::{HbaPort, HbaCmdHeader};
use x86_64::{VirtAddr, PhysAddr};
use x86_64::structures::paging::{Page, PhysFrame, Mapper, PageTableFlags, Size4KiB, OffsetPageTable, PageTable};
use x86_64::registers::control::Cr3;
use alloc::sync::Arc;
use crate::drivers::block::AhciDisk;

#[repr(C)]
pub struct HbaMemory {
    pub host_capability: Volatile<u32>,
    pub global_host_control: Volatile<u32>,
    pub interrupt_status: Volatile<u32>,
    pub ports_implemented: Volatile<u32>,
    pub version: Volatile<u32>,
    pub ccc_control: Volatile<u32>,
    pub ccc_ports: Volatile<u32>,
    pub enclosure_management_location: Volatile<u32>,
    pub enclosure_management_control: Volatile<u32>,
    pub host_capabilities_extended: Volatile<u32>,
    pub bios_handoff_ctrl_sts: Volatile<u32>,
    pub _reserved: [u8; 0xA0 - 0x2C],
    pub vendor: [u8; 0x100 - 0xA0],
    pub ports: [HbaPort; 32], 
}

pub fn init(bar5_address: u32) {
    crate::serial_println!("[AHCI] Inicializando driver en memoria fisica: {:#010x}", bar5_address);
    
    let hhdm_offset = VirtAddr::new(crate::HHDM_REQUEST.response().expect("Fallo HHDM").offset);
    let virtual_bar5 = VirtAddr::new((bar5_address as u64) + hhdm_offset.as_u64());

    // =========================================================
    // MAPEO EXPLÍCITO DE MMIO EN LAS TABLAS DE PÁGINAS
    // =========================================================
    let (level_4_table_frame, _) = Cr3::read();
    let phys_to_virt = |frame: PhysFrame| -> *mut PageTable {
        let virt = hhdm_offset + frame.start_address().as_u64();
        virt.as_mut_ptr()
    };
    
    let level_4_table = unsafe { &mut *phys_to_virt(level_4_table_frame) };
    let mut mapper = unsafe { OffsetPageTable::new(level_4_table, hhdm_offset) };
    let mut frame_allocator = crate::mm::memory::SystemFrameAllocator;

    for i in 0..2 {
        let offset = i * 4096;
        let page = Page::<Size4KiB>::containing_address(virtual_bar5 + offset);
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new((bar5_address as u64) + offset));
        
        let flags = PageTableFlags::PRESENT 
                  | PageTableFlags::WRITABLE 
                  | PageTableFlags::NO_CACHE 
                  | PageTableFlags::WRITE_THROUGH;

        unsafe {
            match mapper.map_to(page, frame, flags, &mut frame_allocator) {
                Ok(mapping) => mapping.flush(),
                Err(e) => crate::serial_println!("[AHCI] Advertencia de mapeo: {:?}", e),
            }
        }
    }

    let hba_mem = unsafe { &mut *(virtual_bar5.as_mut_ptr::<HbaMemory>()) };

    let mut host_ctrl = HbaHostCont::from_bits_truncate(hba_mem.global_host_control.read());
    host_ctrl.insert(HbaHostCont::AE);
    hba_mem.global_host_control.write(host_ctrl.bits());

    let ports_impl = hba_mem.ports_implemented.read();
    crate::serial_println!("[AHCI] Mapa de puertos implementados: {:#034b}", ports_impl);

    for i in 0..32 {
        if (ports_impl & (1 << i)) != 0 {
            let port = &mut hba_mem.ports[i];
            let sata_status = port.ssts.read();
            
            let device_detection = sata_status & 0x0F;
            let power_management = (sata_status >> 8) & 0x0F;

            if device_detection == 3 && power_management == 1 {
                let sig = port.sig.read();

                if sig == 0xEB140101 {
                    crate::serial_println!("[AHCI] -> Lector de CD-ROM (ATAPI) detectado en el puerto {}. Ignorando.", i);
                    continue; 
                } else if sig == 0x00000101 {
                    crate::serial_println!("[AHCI] -> Disco duro (ATA) detectado en el puerto SATA {}. Motor encendido y DMA configurado.", i);
                    
                    // ==========================================
                    // CONFIGURACIÓN DMA DEL PUERTO ATA
                    // ==========================================
                    port.stop_cmd();

                    let dma_frames_phys = crate::mm::memory::get_allocator()
                        .allocate_contiguous_frames(3)
                        .expect("PANICO: Sin memoria contigua para AHCI DMA");
                    
                    let dma_frames_virt = hhdm_offset.as_u64() + dma_frames_phys;

                    unsafe { core::ptr::write_bytes(dma_frames_virt as *mut u8, 0, 4096 * 3); }

                    port.clb.write((dma_frames_phys & 0xFFFFFFFF) as u32);
                    port.clbu.write((dma_frames_phys >> 32) as u32);
                    
                    let fb_phys = dma_frames_phys + 1024;
                    port.fb.write((fb_phys & 0xFFFFFFFF) as u32);
                    port.fbu.write((fb_phys >> 32) as u32);

                    let headers_slice = unsafe { 
                        core::slice::from_raw_parts_mut(dma_frames_virt as *mut HbaCmdHeader, 32) 
                    };

                    for slot in 0..32 {
                        headers_slice[slot].prdtl.write(8); 
                        
                        let command_table_phys = dma_frames_phys + 4096 + (slot as u64 * 256);
                        headers_slice[slot].ctba.write((command_table_phys & 0xFFFFFFFF) as u32);
                        headers_slice[slot].ctbau.write((command_table_phys >> 32) as u32);
                    }

                    port.start_cmd();

                    let disk = Arc::new(AhciDisk {
                        port_index: i,
                        bar5_virt: virtual_bar5.as_u64(),
                    });

                    // ==========================================
                    // DELEGACIÓN LIMPIA (SRP)
                    // El driver solo entrega el hardware, el manager se encarga del resto.
                    // ==========================================
                    crate::fs::manager::process_disk(disk);
                }
            }
        }
    }
}