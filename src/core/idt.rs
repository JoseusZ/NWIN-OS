use pic8259::ChainedPics;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::instructions::port::Port;

// Mapeamos los chips PIC a partir del número 32.
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const TIMER_INTERRUPT: u8 = PIC_1_OFFSET;

// Instanciamos los controladores maestros
pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
    Spurious = PIC_1_OFFSET + 7,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { usize::from(self.as_u8()) }
}

#[inline]
unsafe fn io_wait() {
    Port::<u8>::new(0x80).write(0);
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        
        // --- NUESTRAS NUEVAS TRAMPAS DE HARDWARE ---
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        // -------------------------------------------

        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(crate::core::gdt::DOUBLE_FAULT_IST_INDEX);
        }

        idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Spurious.as_usize()].set_handler_fn(spurious_interrupt_handler);

        idt
    };
}

pub fn init() {
    IDT.load();
    unsafe {
        Port::<u8>::new(0x43).write(0x36);
        io_wait();
        Port::<u8>::new(0x40).write(0x9B); 
        io_wait();
        Port::<u8>::new(0x40).write(0x2E); 
        io_wait();

        PICS.lock().initialize();
        io_wait();

        PICS.lock().write_masks(0xFC, 0xFF);
        io_wait();
    }
}

// ========================================================
// MANEJADORES DE EXCEPCIONES (EL FORENSE)
// ========================================================

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("[DEBUG] Breakpoint Exception:\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();

    // 1. Extracción de Banderas de Diagnóstico
    // - `is_present` y `is_write` se usan en la rama de Copy-on-Write.
    // - `is_user` se usa en el veredicto Ring 3 vs Ring 0.
    // - `is_instruction_fetch` ya NO se guarda: la nueva API
    //   `MemoryError::PageFault { addr, flags }` almacena `error_code.bits()`
    //   en crudo y los consumidores futuros pueden extraer estas senales
    //   con `bitflags` desde `flags`.
    let is_present = error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);
    let is_write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_user = error_code.contains(PageFaultErrorCode::USER_MODE);

    // 2. Intercepción del Copy-on-Write (CoW)
    if is_present && is_write {
        if crate::mm::memory::resolve_cow_fault(fault_addr) {
            return; // Resuelto silenciosamente, la tarea continúa
        }
    }

    // 3. Construcción del Error Estructurado
    let pf_error = crate::core::error::KernelError::Memory(crate::core::error::MemoryError::PageFault {
        addr: fault_addr.as_u64() as usize,
        flags: error_code.bits(),
    });

    // 4. El Veredicto (Ring 3 vs Ring 0)
    if is_user {
        // Aislamiento activo: Matamos al proceso de usuario, el kernel sobrevive.
        crate::serial_println!("[WARNING] Task Terminated: Unhandled Page Fault");
        crate::serial_println!("{:#?}", pf_error);

        // CORRECCIÓN: Delegamos la destrucción al Segador
        crate::task::with_task_manager(|tm| {
            tm.exit_current_task(); 
        });

        // Bucle terminal: Obligamos al Scheduler a saltar a nsh y evitamos el iretq
        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        // Colapso de Ring 0: Activamos al Enterrador
        panic!("FATAL RING-0 EXCEPTION: Page Fault\nError Details: {:#?}\nCPU State: {:#?}", pf_error, stack_frame);
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    // Truco x86_64: Extraer el nivel de privilegio (CPL) desde los 2 bits más bajos del segmento de código (RPL).
    // Si es 3, el código se estaba ejecutando en Ring 3 (Usuario).
    let is_user = (stack_frame.code_segment & 0b11) == 3;

    let gp_error = crate::core::error::KernelError::Privilege(crate::core::error::PrivilegeError::GeneralProtectionFault {
        error_code,
        is_user,
    });

    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: General Protection Fault (#GP)");
        crate::serial_println!("{:#?}", gp_error);

        // CORRECCIÓN: Delegamos la destrucción al Segador
        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: General Protection Fault\nError Details: {:#?}\nCPU State: {:#?}", gp_error, stack_frame);
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let is_user = (stack_frame.code_segment & 0b11) == 3;
    
    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: Divide-by-Zero Exception (#DE)");
        crate::println!("Señal SIGFPE: Division por cero detectada. Matando tarea...");

        // CORRECCIÓN: Delegamos la destrucción al Segador
        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: Divide-by-Zero\nCPU State: {:#?}", stack_frame);
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let is_user = (stack_frame.code_segment & 0b11) == 3;

    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: Invalid Opcode Exception (#UD)");
        crate::println!("Señal SIGILL: Instruccion ilegal detectada. Matando tarea...");

        // CORRECCIÓN: Delegamos la destrucción al Segador
        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: Invalid Opcode\nCPU State: {:#?}", stack_frame);
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    // Un doble fallo casi siempre significa que el kernel corrompió su propia pila.
    // Es el último grito de ayuda de la CPU antes del Triple Fault (reinicio).
    panic!("FATAL EXCEPTION: DOUBLE FAULT\nCPU State: {:#?}", stack_frame);
}

// ========================================================
// MANEJADORES DE HARDWARE ASÍNCRONO
// ========================================================

extern "x86-interrupt" fn spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
    
    unsafe {
        crate::task::scheduler::schedule();
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    crate::drivers::keyboard::process_scancode(scancode);
    
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

pub fn reload() {
    IDT.load();
}