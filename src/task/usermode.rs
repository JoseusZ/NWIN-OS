use x86_64::VirtAddr;
use core::arch::asm;

/// Realiza el salto sin retorno al Nivel de Privilegio 3 (User Space).
/// Esta función falsifica un "Interrupt Stack Frame" y ejecuta iretq.
pub unsafe fn jump_to_user_mode(entry_point: VirtAddr, user_stack: VirtAddr) -> ! {
    let user_data = crate::core::gdt::GDT.1.user_data_selector.0 | 3;
    let user_code = crate::core::gdt::GDT.1.user_code_selector.0 | 3;

    // RFLAGS con interrupciones habilitadas
    let rflags: u64 = 0x202;

    asm!(
        "mov ds, cx",
        "mov es, cx",
        "mov fs, cx",
        "mov gs, cx",
        "push rcx",      // SS
        "push rsi",      // RSP
        "push rdx",      // RFLAGS
        "push r8",       // CS  <-- Usamos r8 en lugar del reservado rbx
        "push r9",       // RIP <-- Usamos r9
        "iretq",
        in("rcx") user_data as u64,
        in("rsi") user_stack.as_u64(),
        in("rdx") rflags,
        in("r8") user_code as u64,
        in("r9") entry_point.as_u64(),
        options(noreturn)
    );
}