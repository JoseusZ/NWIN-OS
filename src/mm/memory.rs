use spin::Mutex;
use limine::memmap::MEMMAP_USABLE;
use x86_64::structures::paging::{
    FrameAllocator as PagingFrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB, Translate
};
use x86_64::{PhysAddr, VirtAddr};
use x86_64::registers::control::{Cr3, Cr3Flags};

pub const FRAME_SIZE: u64 = 4096;

pub struct BitmapFrameAllocator {
    bitmap: &'static mut [u8],
    // --- NUEVO: Arreglo de Referencias para Copy-on-Write ---
    ref_counts: &'static mut [u16], 
    total_frames: usize,
    last_free_frame_hint: usize,
}

// 1. Ocultar ALLOCATOR detrás de una función para evitar accesos directos peligrosos.
static ALLOCATOR: Mutex<BitmapFrameAllocator> = Mutex::new(BitmapFrameAllocator::empty());

impl BitmapFrameAllocator {
    pub const fn empty() -> Self {
        Self {
            bitmap: &mut [],
            ref_counts: &mut [],
            total_frames: 0,
            last_free_frame_hint: 0,
        }
    }

    pub fn init(&mut self) {
        let mmap_response = crate::MEMMAP_REQUEST.response().expect("PANICO: Sin mapa de memoria");
        let entries = mmap_response.entries();

        // 1. CORRECCIÓN CRÍTICA: Calcular max_addr ignorando el MMIO
        let mut max_addr = 0;
        for entry in entries {
            // Solo rastreamos la memoria que nos pertenece o que podemos leer,
            // ignorando reservas de hardware con direcciones estratosféricas.
            if entry.type_ == limine::memmap::MEMMAP_USABLE 
                || entry.type_ == limine::memmap::MEMMAP_BOOTLOADER_RECLAIMABLE
                || entry.type_ == limine::memmap::MEMMAP_EXECUTABLE_AND_MODULES
                || entry.type_ == limine::memmap::MEMMAP_FRAMEBUFFER 
            {
                let top = entry.base + entry.length;
                if top > max_addr { max_addr = top; }
            }
        }

        let total_frames = ((max_addr + FRAME_SIZE - 1) / FRAME_SIZE) as usize;
        
        let raw_bitmap_size = (total_frames + 7) / 8;
        let bitmap_size_aligned = (raw_bitmap_size + 7) & !7; 
        
        let refcounts_size_in_bytes = total_frames * core::mem::size_of::<u16>();
        let metadata_total_size = bitmap_size_aligned as u64 + refcounts_size_in_bytes as u64;

        // 2. CORRECCIÓN DE SEGURIDAD: Evitar falsos positivos si la RAM arranca en 0x0
        let mut metadata_phys_addr: Option<u64> = None;
        for entry in entries {
            if entry.type_ == MEMMAP_USABLE && entry.length >= metadata_total_size {
                metadata_phys_addr = Some(entry.base);
                break;
            }
        }

        let metadata_phys_addr = metadata_phys_addr.expect("PANICO: No hay RAM contigua para la metadata del Allocator");

        // --- RESTAURADO: El resto de la inicialización que se había borrado ---
        let hhdm_offset = crate::HHDM_REQUEST.response()
            .expect("Fallo crítico: El Bootloader no proporcionó HHDM").offset;
            
        let metadata_virt_ptr = (hhdm_offset + metadata_phys_addr) as *mut u8;

        unsafe {
            let bitmap_slice = core::slice::from_raw_parts_mut(metadata_virt_ptr, raw_bitmap_size);
            bitmap_slice.fill(0xFF); // RAM ocupada por defecto
            self.bitmap = bitmap_slice;

            let refcounts_virt_ptr = metadata_virt_ptr.add(bitmap_size_aligned) as *mut u16;
            let refcounts_slice = core::slice::from_raw_parts_mut(refcounts_virt_ptr, total_frames);
            refcounts_slice.fill(1); // Cada bloque arranca con 1 referencia base
            self.ref_counts = refcounts_slice;
        }

        self.total_frames = total_frames;

        for entry in entries {
            if entry.type_ == MEMMAP_USABLE {
                let start_frame = ((entry.base + FRAME_SIZE - 1) / FRAME_SIZE) as usize;
                let end_frame = ((entry.base + entry.length) / FRAME_SIZE) as usize;
                
                for i in start_frame..end_frame {
                    self.force_free_bit(i);
                }
            }
        }

        let metadata_start_frame = (metadata_phys_addr / FRAME_SIZE) as usize;
        let metadata_end_frame = metadata_start_frame + ((metadata_total_size + FRAME_SIZE - 1) / FRAME_SIZE) as usize;
        
        for i in metadata_start_frame..metadata_end_frame {
            self.force_set_bit(i);
        }

        self.force_set_bit(0); // Proteger marco 0
    } // <--- LLAVE CERRADA CORRECTAMENTE

    // --- OPERACIONES INTERNAS DE INICIALIZACION ---

    fn force_set_bit(&mut self, frame: usize) {
        if frame >= self.total_frames { return; }
        let byte = frame / 8;
        let bit = frame % 8;
        self.bitmap[byte] |= 1 << bit;
        self.ref_counts[frame] = 1;
    }

    // RESTAURADO: Se separó force_free_bit de test_bit
    fn force_free_bit(&mut self, frame: usize) {
        if frame >= self.total_frames { return; }
        let byte = frame / 8;
        let bit = frame % 8;
        self.bitmap[byte] &= !(1 << bit);
        self.ref_counts[frame] = 0;
    }

    fn test_bit(&self, frame: usize) -> bool {
        if frame >= self.total_frames { return true; } // Asumir ocupado si está fuera de rango
        let byte = frame / 8;
        let bit = frame % 8;
        (self.bitmap[byte] & (1 << bit)) != 0
    }

    // --- INTERFAZ DEL ALLOCATOR CON SOPORTE CoW ---

    pub fn allocate_frame(&mut self) -> Option<u64> {
        for i in self.last_free_frame_hint..self.total_frames {
            if !self.test_bit(i) {
                self.force_set_bit(i);
                self.last_free_frame_hint = i + 1;
                return Some((i as u64) * FRAME_SIZE);
            }
        }
        
        for i in 0..self.last_free_frame_hint {
            if !self.test_bit(i) {
                self.force_set_bit(i);
                self.last_free_frame_hint = i + 1;
                return Some((i as u64) * FRAME_SIZE);
            }
        }
        None 
    }

    /// NUEVO: Implementación CoW - Solo libera el marco si las referencias llegan a 0.
    pub fn deallocate_frame(&mut self, phys_addr: u64) {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        assert!(frame < self.total_frames, "Fallo fatal: Intento de liberar marco inexistente");
        
        if self.ref_counts[frame] > 0 {
            self.ref_counts[frame] -= 1;
            
            // Si nadie más usa este marco, lo liberamos de verdad
            if self.ref_counts[frame] == 0 {
                let byte = frame / 8;
                let bit = frame % 8;
                self.bitmap[byte] &= !(1 << bit);
                
                if frame < self.last_free_frame_hint {
                    self.last_free_frame_hint = frame;
                }
            }
        } else {
            panic!("Doble liberacion de memoria detectada en Ring 0 (Marco {})", frame);
        }
    }

    /// NUEVO: Incrementa el contador de referencias. VITAL para tu futura syscall fork().
    pub fn reference_frame(&mut self, phys_addr: u64) {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        assert!(frame < self.total_frames, "Fallo fatal: Intento de referenciar marco inexistente");
        assert!(self.ref_counts[frame] > 0, "Intento de compartir un marco libre");
        
        self.ref_counts[frame] += 1;
    }

    /// NUEVO: Expone el contador para que el IDT decida si debe clonar la página.
    pub fn get_ref_count(&self, phys_addr: u64) -> u16 {
        let frame = (phys_addr / FRAME_SIZE) as usize;
        if frame >= self.total_frames { return 1; } // Bloques reservados son inmutables
        self.ref_counts[frame]
    }

    // Algoritmo de contigüidad optimizado (Se mantiene para memoria no compartida/DMA)
    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        if count == 0 { return None; }
        
        let mut i = 0;
        while i <= self.total_frames.saturating_sub(count) {
            let mut free_run = 0;
            
            for j in 0..count {
                if self.test_bit(i + j) {
                    break;
                }
                free_run += 1;
            }

            if free_run == count {
                for j in i..(i + count) {
                    self.force_set_bit(j);
                }
                return Some((i as u64) * FRAME_SIZE);
            } else {
                i += free_run + 1;
            }
        }
        None
    }
}

// Proxy Seguro Anti-Deadlocks
pub struct SystemFrameAllocator;

// API Pública controlada para obtener memoria
pub fn get_allocator() -> spin::MutexGuard<'static, BitmapFrameAllocator> {
    ALLOCATOR.lock()
}

unsafe impl PagingFrameAllocator<Size4KiB> for SystemFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let frame_address = get_allocator().allocate_frame()?;
            
            let hhdm_offset = crate::HHDM_REQUEST.response().expect("Sin HHDM").offset;
            let virt_addr = frame_address + hhdm_offset;
            unsafe {
                core::ptr::write_bytes(virt_addr as *mut u8, 0, 4096);
            }
            
            let phys_addr = PhysAddr::new(frame_address);
            Some(PhysFrame::containing_address(phys_addr))
        })
    }
}

pub unsafe fn isolate_and_init_paging(
    physical_memory_offset: VirtAddr,
    allocator: &mut impl PagingFrameAllocator<Size4KiB>,
) -> OffsetPageTable<'static> {
    
    let (limine_pml4_frame, _) = Cr3::read();
    let limine_pml4_virt = physical_memory_offset + limine_pml4_frame.start_address().as_u64();
    let limine_pml4: &PageTable = &*(limine_pml4_virt.as_ptr());

    let new_pml4_frame = allocator.allocate_frame().expect("PANICO: Sin memoria fisica para la nueva PML4");
    let new_pml4_phys = new_pml4_frame.start_address();
    let new_pml4_virt = physical_memory_offset + new_pml4_phys.as_u64();
    
    let new_pml4: &mut PageTable = &mut *(new_pml4_virt.as_mut_ptr());
    new_pml4.zero(); 

    // Copiamos la mitad superior (Kernel Space)
    for i in 256..512 {
        new_pml4[i] = limine_pml4[i].clone();
    }
    
    // *** FIX CRÍTICO: Copiar también entradas de la mitad user ***
    // Limine pone el HHDM en la mitad user (índices < 256). Sin esto,
    // la nueva PML4 no puede traducir las direcciones del HHDM que usa
    // para acceder a memoria física, y todo #GP/#DF en cascada.
    //
    // Estrategia segura: copiar TODAS las entradas no-vacías de Limine.
    // Esto preserva HHDM + cualquier mapping que Limine haya hecho en
    // el rango user. Como después vamos a usar la mitad kernel para
    // todo, las entradas copiadas de la mitad user no se solapan.
    for i in 0..256 {
        let entry = limine_pml4[i].clone();
        if !entry.is_unused() {
            new_pml4[i] = entry;
        }
    }

    Cr3::write(new_pml4_frame, Cr3Flags::empty());
    OffsetPageTable::new(new_pml4, physical_memory_offset)
}
// --- API PUBLICA SEGURA PARA COPY-ON-WRITE (CoW) ---

/// Incrementa de forma segura el contador de referencias de un marco físico.
pub fn cow_reference_frame(phys_addr: u64) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().reference_frame(phys_addr);
    });
}

/// Disminuye el contador de referencias y libera el marco si llega a 0.
pub fn cow_deallocate_frame(phys_addr: u64) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().deallocate_frame(phys_addr);
    });
}

/// Consulta cuántas tareas están usando este marco físico actualmente.
pub fn cow_get_ref_count(phys_addr: u64) -> u16 {
    x86_64::instructions::interrupts::without_interrupts(|| {
        get_allocator().get_ref_count(phys_addr)
    })
}

// --- TRADUCCIÓN DE DIRECCIONES ---

/// Traduce una dirección virtual a física leyendo las tablas de páginas actuales de la CPU.
/// Es seguro llamarla desde el IDT durante un Page Fault.
pub fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    // 1. Obtenemos el offset HHDM que el bootloader nos dio
    let hhdm_offset = crate::HHDM_REQUEST.response()
        .expect("Fallo crítico: El Bootloader no proporcionó HHDM").offset;
    let phys_mem_offset = VirtAddr::new(hhdm_offset);

    // 2. Leemos la dirección física de la PML4 activa desde el registro Cr3
    let (pml4_frame, _) = Cr3::read();
    
    // 3. Calculamos la dirección virtual donde podemos leer/modificar esta tabla
    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    
    // 4. Creamos una referencia mutable temporal a la tabla (requerida por OffsetPageTable)
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr()) };

    // 5. Instanciamos el mapper temporalmente y traducimos
    let mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };
    
    // Usamos el trait 'Translate' para hacer el trabajo sucio por nosotros
    mapper.translate_addr(addr)
}

// --- RESOLUCIÓN COPY-ON-WRITE ---

/// Resuelve un fallo de página causado por Copy-on-Write.
/// Retorna `true` si logró clonar la página y restaurar los permisos.
// --- RESOLUCIÓN COPY-ON-WRITE ---
pub fn resolve_cow_fault(fault_addr: VirtAddr) -> bool {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame};
    
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo crítico: Sin HHDM").offset;
    let phys_mem_offset = VirtAddr::new(hhdm_offset);

    let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    let page = Page::<Size4KiB>::containing_address(fault_addr);

    let phys_addr = match mapper.translate_addr(fault_addr) {
        Some(addr) => addr,
        None => return false,
    };

    let current_flags = match mapper.translate(fault_addr) {
        x86_64::structures::paging::mapper::TranslateResult::Mapped { flags, .. } => flags,
        _ => return false,
    };

    let ref_count = cow_get_ref_count(phys_addr.as_u64());
    if ref_count == 0 { return false; } 

    // --- ESTO FUE LO QUE SE BORRÓ ACCIDENTALMENTE ---
    if ref_count == 1 {
        unsafe {
            mapper.update_flags(page, current_flags | PageTableFlags::WRITABLE)
                .expect("Fallo al actualizar flags CoW")
                .flush(); 
        }
        return true;
    }
    // ------------------------------------------------

    let new_frame_addr = match get_allocator().allocate_frame() {
        Some(addr) => addr,
        None => return false, 
    };
    let new_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(new_frame_addr));

    unsafe {
        let src_ptr = (phys_mem_offset + phys_addr.as_u64()).as_ptr::<u8>();
        let dst_ptr = (phys_mem_offset + new_frame_addr).as_mut_ptr::<u8>();
        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 4096);

        let (_, flush) = mapper.unmap(page).expect("Fallo al desmapear página CoW");
        flush.flush();

        let mut allocator = SystemFrameAllocator;
        mapper.map_to(page, new_frame, current_flags | PageTableFlags::WRITABLE, &mut allocator)
            .expect("Fallo al mapear clon CoW")
            .flush();
    }

    cow_deallocate_frame(phys_addr.as_u64());
    true
}

/// Mapea una página virtual específica asignándole permisos de Ring 3 (Usuario).
/// VITAL para poder ejecutar procesos fuera del Kernel.
/// Mapea una página virtual específica asignándole permisos de Ring 3 (Usuario).
/// VITAL para poder ejecutar procesos fuera del Kernel.
/// Mapea una página virtual específica asignándole permisos de Ring 3 (Usuario).
/// AHORA RECIBE LA PML4 DESTINO PARA NO DEPENDER DEL CR3.
/// Mapea una página virtual específica en una PML4 REMOTA.
pub fn allocate_and_map_user_page(
    target_pml4: x86_64::structures::paging::PhysFrame, 
    virtual_address: x86_64::VirtAddr
) -> Result<(), &'static str> {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, OffsetPageTable, PageTable, Size4KiB};
    
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo crítico: Sin HHDM").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    // EL SECRETO ESTÁ AQUÍ: Ignoramos CR3 y reconstruimos la tabla usando target_pml4
    let pml4_virt = phys_mem_offset + target_pml4.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr() as *mut PageTable) };
    let mut mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    let page = Page::<Size4KiB>::containing_address(virtual_address);
    
    // Protección contra solapamiento de segmentos ELF
    if mapper.translate_page(page).is_ok() {
        return Ok(()); 
    }
    
    let frame_addr = crate::mm::memory::get_allocator().allocate_frame().ok_or("Out of memory")?;
    let frame = PhysFrame::containing_address(x86_64::PhysAddr::new(frame_addr));

    let flags = PageTableFlags::PRESENT 
              | PageTableFlags::WRITABLE 
              | PageTableFlags::USER_ACCESSIBLE;

    let mut allocator = crate::mm::memory::SystemFrameAllocator;
    
    unsafe {
        mapper.map_to(page, frame, flags, &mut allocator)
            .map_err(|_| "Fallo al mapear la página remota")?
            .flush(); // En tablas remotas el flush es inofensivo
            
        let hhdm_ptr = (phys_mem_offset + frame_addr).as_mut_ptr::<u8>();
        core::ptr::write_bytes(hhdm_ptr, 0, 4096);
    }
    
    Ok(())
}

/// Traduce una dirección virtual buscando en una PML4 REMOTA específica.
pub fn translate_in_pml4(
    target_pml4: x86_64::structures::paging::PhysFrame, 
    virtual_address: x86_64::VirtAddr
) -> Option<x86_64::PhysAddr> {
    use x86_64::structures::paging::{OffsetPageTable, PageTable};
    
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo crítico: Sin HHDM").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    // Reconstruimos el traductor apuntando a la mente del proceso hijo
    let pml4_virt = phys_mem_offset + target_pml4.start_address().as_u64();
    let pml4 = unsafe { &mut *(pml4_virt.as_mut_ptr() as *mut PageTable) };
    let mapper = unsafe { OffsetPageTable::new(pml4, phys_mem_offset) };

    mapper.translate_addr(virtual_address)
}
// --- RECOLECCIÓN DE BASURA AVANZADA (GRIM REAPER) ---

/// Recorre el árbol de paginación de una tarea muerta y libera
/// estrictamente las páginas físicas que fueron asignadas al Espacio de Usuario.
pub unsafe fn destroy_user_address_space(pml4_frame: x86_64::structures::paging::PhysFrame) {
    use x86_64::structures::paging::{PageTable, PageTableFlags};
    
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo crítico").offset;
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm_offset);

    // 1. Acceder a la PML4 (Nivel 4)
    let pml4_virt = phys_mem_offset + pml4_frame.start_address().as_u64();
    let pml4 = &mut *(pml4_virt.as_mut_ptr() as *mut PageTable);

    // 2. Escaneamos SOLO la mitad inferior (User Space: entradas 0 a 255)
    // La mitad superior (256 a 511) es el kernel y jamás debe tocarse.
    for p4_idx in 0..256 {
        let p4_entry = &pml4[p4_idx];
        if !p4_entry.is_unused() && p4_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
            
            let pdpt_virt = phys_mem_offset + p4_entry.addr().as_u64();
            let pdpt = &mut *(pdpt_virt.as_mut_ptr() as *mut PageTable);
            
            for p3_idx in 0..512 {
                let p3_entry = &pdpt[p3_idx];
                if !p3_entry.is_unused() && p3_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
                    
                    // SEGURIDAD CRÍTICA: ¿Es una página gigante de 1GB?
                    if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                        cow_deallocate_frame(p3_entry.addr().as_u64());
                        pdpt[p3_idx].set_unused();
                        continue; // No intentamos bajar al siguiente nivel
                    }

                    let pd_virt = phys_mem_offset + p3_entry.addr().as_u64();
                    let pd = &mut *(pd_virt.as_mut_ptr() as *mut PageTable);
                    
                    for p2_idx in 0..512 {
                        let p2_entry = &pd[p2_idx];
                        if !p2_entry.is_unused() && p2_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
                            
                            // SEGURIDAD CRÍTICA: ¿Es una página gigante de 2MB?
                            if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                                cow_deallocate_frame(p2_entry.addr().as_u64());
                                pd[p2_idx].set_unused();
                                continue; // No intentamos bajar al siguiente nivel
                            }

                            // Llegamos a la Tabla de Páginas Final (Nivel 1 - 4KB)
                            let pt_virt = phys_mem_offset + p2_entry.addr().as_u64();
                            let pt = &mut *(pt_virt.as_mut_ptr() as *mut PageTable);
                            
                            for p1_idx in 0..512 {
                                let p1_entry = &pt[p1_idx];
                                if !p1_entry.is_unused() && p1_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
                                    
                                    // Liberamos la página de datos finales (Código, Stack, Heap de usuario)
                                    let phys_frame = p1_entry.addr().as_u64();
                                    cow_deallocate_frame(phys_frame);
                                    
                                    // Limpiamos la entrada para evitar referencias nulas/fantasmas
                                    pt[p1_idx].set_unused();
                                }
                            }
                            // Terminamos con esta tabla nivel 1, liberamos la tabla en sí misma
                            cow_deallocate_frame(p2_entry.addr().as_u64());
                            pd[p2_idx].set_unused();
                        }
                    }
                    // Liberamos la tabla nivel 2
                    cow_deallocate_frame(p3_entry.addr().as_u64());
                    pdpt[p3_idx].set_unused();
                }
            }
            // Liberamos la tabla nivel 3
            cow_deallocate_frame(p4_entry.addr().as_u64());
            pml4[p4_idx].set_unused();
        }
    }

    // 3. Finalmente, destruimos el marco físico maestro (la propia PML4)
    cow_deallocate_frame(pml4_frame.start_address().as_u64());
}

/// Crea una nueva PML4 clonando el espacio de Kernel y HHDM, aislando el Ring 3.
/// Crea una nueva PML4 clonando el espacio de Kernel y HHDM, aislando el Ring 3.
pub fn create_isolated_pml4() -> Option<x86_64::structures::paging::PhysFrame> {
    use x86_64::structures::paging::{PageTable, PageTableFlags, FrameAllocator};
    use x86_64::registers::control::Cr3;

    let mut allocator = SystemFrameAllocator;
    let new_frame = allocator.allocate_frame()?;

    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Sin HHDM").offset;
    let phys_offset = x86_64::VirtAddr::new(hhdm_offset);
    let new_pml4_virt = phys_offset + new_frame.start_address().as_u64();
    let new_pml4 = unsafe { &mut *(new_pml4_virt.as_mut_ptr() as *mut PageTable) };
    
    // 1. Limpiamos toda la tabla. Terreno virgen.
    new_pml4.zero();

    let (current_pml4_frame, _) = Cr3::read();
    let current_pml4_virt = phys_offset + current_pml4_frame.start_address().as_u64();
    let current_pml4 = unsafe { &*(current_pml4_virt.as_ptr() as *const PageTable) };

    // 2. Copiamos la mitad superior íntegra (Kernel Space estricto: 256 a 511)
    for i in 256..512 {
        new_pml4[i] = current_pml4[i].clone();
    }

    // 3. EL FILTRO QUIRÚRGICO (Mitad inferior: 0 a 255)
    // Conservamos el HHDM y el Framebuffer de Limine, pero excluimos TODA
    // la memoria que le pertenezca a un proceso de usuario (Ring 3).
    for i in 0..256 {
        let entry = current_pml4[i].clone();
        
        // Si la entrada está en uso y NO tiene el flag de usuario, es del kernel/Limine. La copiamos.
        if !entry.is_unused() && !entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
            new_pml4[i] = entry;
        }
    }

    Some(new_frame)
}

pub fn debug_page_tables(pml4_frame: x86_64::structures::paging::PhysFrame, vaddr: u64) {
    use x86_64::structures::paging::PageTable;
    
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Sin HHDM").offset;
    
    // Calculamos los índices para cada nivel
    let p4_idx = (vaddr >> 39) & 0x1FF;
    let p3_idx = (vaddr >> 30) & 0x1FF;
    let p2_idx = (vaddr >> 21) & 0x1FF;
    let p1_idx = (vaddr >> 12) & 0x1FF;

    let pml4_virt = pml4_frame.start_address().as_u64() + hhdm_offset;
    let pml4 = unsafe { &*(pml4_virt as *const PageTable) };
    
    crate::println!("--- DEBUG MMU PARA 0x{:X} ---", vaddr);
    
    let p4_entry = &pml4[p4_idx as usize];
    crate::println!("PML4[{}] -> Present: {}, Flags: {:?}", p4_idx, !p4_entry.is_unused(), p4_entry.flags());
    if p4_entry.is_unused() { return; }
    
    let pdpt_virt = p4_entry.addr().as_u64() + hhdm_offset;
    let pdpt = unsafe { &*(pdpt_virt as *const PageTable) };
    let p3_entry = &pdpt[p3_idx as usize];
    crate::println!("PDPT[{}] -> Present: {}, Flags: {:?}", p3_idx, !p3_entry.is_unused(), p3_entry.flags());
    if p3_entry.is_unused() { return; }
    
    let pd_virt = p3_entry.addr().as_u64() + hhdm_offset;
    let pd = unsafe { &*(pd_virt as *const PageTable) };
    let p2_entry = &pd[p2_idx as usize];
    crate::println!("PD[{}]   -> Present: {}, Flags: {:?}", p2_idx, !p2_entry.is_unused(), p2_entry.flags());
    if p2_entry.is_unused() { return; }
    
    let pt_virt = p2_entry.addr().as_u64() + hhdm_offset;
    let pt = unsafe { &*(pt_virt as *const PageTable) };
    let p1_entry = &pt[p1_idx as usize];
    crate::println!("PT[{}]   -> Present: {}, Flags: {:?}", p1_idx, !p1_entry.is_unused(), p1_entry.flags());
    crate::println!("-----------------------------");
}

pub fn is_user_page_mapped(addr: x86_64::VirtAddr) -> bool {
    let hhdm_offset = crate::HHDM_REQUEST.response().expect("Fallo crítico: Sin HHDM").offset;
    let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
    
    let p4_phys = pml4_frame.start_address().as_u64();
    
    // Función anónima para leer 8 bytes físicos saltándose al compilador
    let read_entry = |phys_addr: u64, index: usize| -> u64 {
        let virt = phys_addr + hhdm_offset + (index as u64 * 8);
        unsafe { core::ptr::read_volatile(virt as *const u64) }
    };

    const PRESENT: u64 = 1 << 0;
    const USER: u64 = 1 << 2;
    const HUGE: u64 = 1 << 7;
    const PHYS_MASK: u64 = 0x000FFFFF_FFFFF000;

    // Nivel 4 (PML4)
    let p4_entry = read_entry(p4_phys, usize::from(addr.p4_index()));
    if p4_entry & PRESENT == 0 { return false; }
    
    // Nivel 3 (PDPT)
    let p3_phys = p4_entry & PHYS_MASK;
    let p3_entry = read_entry(p3_phys, usize::from(addr.p3_index()));
    if p3_entry & PRESENT == 0 { return false; }
    if p3_entry & HUGE != 0 { return (p3_entry & USER) != 0; }

    // Nivel 2 (PD)
    let p2_phys = p3_entry & PHYS_MASK;
    let p2_entry = read_entry(p2_phys, usize::from(addr.p2_index()));
    if p2_entry & PRESENT == 0 { return false; }
    if p2_entry & HUGE != 0 { return (p2_entry & USER) != 0; }

    // Nivel 1 (PT)
    let p1_phys = p2_entry & PHYS_MASK;
    let p1_entry = read_entry(p1_phys, usize::from(addr.p1_index()));
    if p1_entry & PRESENT == 0 { return false; }
    
    (p1_entry & USER) != 0
}