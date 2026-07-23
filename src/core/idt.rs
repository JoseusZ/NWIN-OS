//! Interrupt Descriptor Table (IDT) and 8259 PIC plumbing.
//!
//! Wires CPU exception handlers (page fault, GPF, divide error, invalid
//! opcode, double fault, breakpoint), the chained 8259 PIC, and the
//! hardware IRQ handlers for the timer tick, keyboard, and spurious
//! vector.

use pic8259::ChainedPics;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::instructions::port::Port;

/// Master PIC IRQ base (32-39).
pub const PIC_1_OFFSET: u8 = 32;
/// Slave PIC IRQ base (40-47).
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
/// First vector exposed by the master PIC: the timer tick.
pub const TIMER_INTERRUPT: u8 = PIC_1_OFFSET;

/// Chained 8259 PIC pair, initialised with the offsets above.
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// IDs of the hardware IRQ lines the kernel services.
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

/// 1 µs I/O delay via the POST port. Used after PIC initialisation
/// writes to give legacy ISA peripherals time to settle.
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

        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);

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

/// Loads the IDT and programs the chained 8259 PIC pair.
///
/// The reload sequence (`0x36`, low byte, high byte) sets the PIT
/// channel 0 in mode 3 (square wave generator) with a divisor of
/// `0x9B2E` (~100 Hz at 1193182 Hz). Mask `0xFC, 0xFF` enables only
/// the cascade line (IRQ 2) and the timer (IRQ 0).
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
// CPU exception handlers
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

    let is_present = error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);
    let is_write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_user = error_code.contains(PageFaultErrorCode::USER_MODE);

    // CoW interception: a write to a present-but-read-only page is
    // resolved silently by cloning the underlying frame. Other
    // presentation classes fall through to the structured error path.
    if is_present && is_write {
        if crate::mm::memory::resolve_cow_fault(fault_addr) {
            return;
        }
    }

    let pf_error = crate::core::error::KernelError::Memory(crate::core::error::MemoryError::PageFault {
        addr: fault_addr.as_u64() as usize,
        flags: error_code.bits(),
    });

    if is_user {
        // Ring 3 fault: kill the offending task, drop to the scheduler.
        crate::serial_println!("[WARNING] Task Terminated: Unhandled Page Fault");
        crate::serial_println!("{}", pf_error);

        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        // Ring 0 fault: panic with structured details.
        panic!("FATAL RING-0 EXCEPTION: Page Fault\nError Details: {}\nCPU State: {:#?}", pf_error, stack_frame);
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    // The two LSBs of the pushed CS reflect the CPL of the faulting
    // code (RPL == CPL for non-conforming segments).
    let is_user = (stack_frame.code_segment & 0b11) == 3;

    let gp_error = crate::core::error::KernelError::Privilege(crate::core::error::PrivilegeError::GeneralProtectionFault {
        error_code,
        is_user,
    });

    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: General Protection Fault (#GP)");
        crate::serial_println!("{}", gp_error);

        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: General Protection Fault\nError Details: {}\nCPU State: {:#?}", gp_error, stack_frame);
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let is_user = (stack_frame.code_segment & 0b11) == 3;

    let de_error = crate::core::error::KernelError::Privilege(
        crate::core::error::PrivilegeError::DivideError { is_user }
    );

    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: Divide-by-Zero Exception (#DE)");
        crate::println!("Signal SIGFPE: divide-by-zero detected. Killing task...");
        crate::serial_println!("{}", de_error);

        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: Divide-by-Zero\nError Details: {}\nCPU State: {:#?}", de_error, stack_frame);
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let is_user = (stack_frame.code_segment & 0b11) == 3;

    let ud_error = crate::core::error::KernelError::Privilege(
        crate::core::error::PrivilegeError::InvalidOpcode { is_user }
    );

    if is_user {
        crate::serial_println!("[WARNING] Task Terminated: Invalid Opcode Exception (#UD)");
        crate::println!("Signal SIGILL: illegal instruction detected. Killing task...");
        crate::serial_println!("{}", ud_error);

        crate::task::with_task_manager(|tm| {
            tm.exit_current_task();
        });

        loop {
            unsafe { crate::task::scheduler::schedule(); }
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    } else {
        panic!("FATAL RING-0 EXCEPTION: Invalid Opcode\nError Details: {}\nCPU State: {:#?}", ud_error, stack_frame);
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    // A double fault almost always means the kernel corrupted its
    // own stack. This is the CPU's last attempt to recover before
    // a Triple Fault resets the machine.
    let df_error = crate::core::error::KernelError::Privilege(
        crate::core::error::PrivilegeError::DoubleFault
    );
    panic!("FATAL EXCEPTION: DOUBLE FAULT\nError Details: {}\nCPU State: {:#?}", df_error, stack_frame);
}

// ========================================================
// Hardware IRQ handlers
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