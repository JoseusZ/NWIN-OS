// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use x86_64::registers::model_specific::{Efer, EferFlags, Star, LStar, SFMask};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

// ========================================================
// INFRAESTRUCTURA DE MEMORIA PARA SYSCALLS
// ========================================================

#[no_mangle]
pub static mut KERNEL_RSP: u64 = 0; 
#[no_mangle]
pub static mut SCRATCH_RSP: u64 = 0;

#[repr(C)]
#[derive(Debug)]
pub struct SyscallRegisters {
    pub rax: u64, 
    pub rdi: u64, 
    pub rsi: u64, 
    pub rdx: u64, 
    pub r10: u64, 
    pub r8:  u64, 
    pub r9:  u64, 
}

pub fn init() {
    let selectors = &crate::core::gdt::GDT.1;

    unsafe {
        Efer::update(|flags| flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
        
        Star::write(
            selectors.user_code_selector,   
            selectors.user_data_selector,   
            selectors.kernel_code_selector, 
            selectors.kernel_data_selector  
        ).expect("Fallo crítico: No se pudo escribir en el MSR STAR");

        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
        SFMask::write(RFlags::INTERRUPT_FLAG);
    }
}

// ========================================================
// EL ESCUDO ZERO-TRUST (Validación de Cuarentena)
// ========================================================
/// Verifica matemáticamente que un rango de memoria pertenece estrictamente
/// a la mitad inferior del espacio virtual (Ring 3) y no cruza al Kernel.
fn is_valid_user_memory(ptr: u64, len: usize) -> bool {
    if ptr == 0 { return false; }
    if len > (isize::MAX as usize) { return false; }
    if len == 0 { return true; }

    let end = ptr.saturating_add(len as u64);
    if end > 0x00007FFFFFFFFFFF { return false; }

    let start_page = ptr & !0xFFF;
    let end_page = (end - 1) & !0xFFF;

    let mut current_page = start_page;
    while current_page <= end_page {
        if !crate::mm::memory::is_user_page_mapped(x86_64::VirtAddr::new(current_page)) {
            return false;
        }
        current_page += 4096;
    }

    true
}

// ========================================================
// EL GUARDIÁN (TRAMPOLÍN EN ENSAMBLADOR)
// ========================================================
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "mov qword ptr [rip + {scratch}], rsp",
        "mov rsp, qword ptr [rip + {kernel_rsp}]",
        "push qword ptr [rip + {scratch}]",
        "push rcx", "push r11", "push r9", "push r8",
        "push r10", "push rdx", "push rsi", "push rdi", "push rax",
        
        "mov rdi, rsp", 
        
        "push r12",
        "mov r12, rsp",
        "and rsp, -16",
        "call {handle}",
        "mov rsp, r12",
        "pop r12",

        "pop rax", "pop rdi", "pop rsi", "pop rdx", "pop r10",
        "pop r8", "pop r9", "pop r11", "pop rcx", "pop rsp",
        "sysretq",
        
        scratch = sym SCRATCH_RSP,
        kernel_rsp = sym KERNEL_RSP,
        handle = sym handle_syscall_rust,
    );
}

// ========================================================
// MANEJADOR LÓGICO EN RUST
// ========================================================
extern "C" fn handle_syscall_rust(regs: &mut SyscallRegisters) {
    match regs.rax {
        0 => { // ABI de Linux: sys_read
            let fd = regs.rdi as usize; 
            let buffer_ptr = regs.rsi as *mut u8; 
            let length = regs.rdx as usize; 

            if !is_valid_user_memory(buffer_ptr as u64, length) {
                regs.rax = (-14i64) as u64; // -EFAULT
                return;
            }

            let mut is_stdin = false;
            let mut bytes_read = 0;
            let mut error_code: i64 = 0; // AHORA ES UN ENTERO CON SIGNO

            crate::task::with_task_manager(|tm| {
                if let Some(task_id) = tm.current_task {
                    if let Some(task) = tm.task_registry.get_mut(&task_id) {
                        if let Some(descriptor) = task.fd_table.get_mut(fd) {
                            match descriptor {
                                crate::fs::fd::FileDescriptor::Stdin => is_stdin = true,
                                crate::fs::fd::FileDescriptor::Stdout | crate::fs::fd::FileDescriptor::Stderr => {
                                    error_code = -9; // -EBADF
                                },
                                crate::fs::fd::FileDescriptor::RegularFile { ref vnode, ref mut offset } => {
                                    let user_slice = unsafe { core::slice::from_raw_parts_mut(buffer_ptr, length) };
                                    let read_bytes = vnode.read(*offset, user_slice);

                                    if read_bytes > 0 {
                                        *offset += read_bytes;
                                        bytes_read = read_bytes as u64;
                                    } else {
                                        bytes_read = 0; 
                                    }
                                }
                            }
                        } else { error_code = -9; }
                    }
                }
            });

            if error_code != 0 {
                regs.rax = error_code as u64; // Retorna el número negativo
                return;
            }

            if !is_stdin {
                regs.rax = bytes_read;
                return;
            }

            if length > 0 {
                loop {
                    if let Some(c) = crate::drivers::keyboard::pop_key() {
                        let user_slice = unsafe { core::slice::from_raw_parts_mut(buffer_ptr, length) };
                        let mut temp_buf = [0; 4];
                        let encoded = c.encode_utf8(&mut temp_buf);
                        let bytes_to_copy = core::cmp::min(encoded.len(), length);
                        
                        user_slice[..bytes_to_copy].copy_from_slice(&encoded.as_bytes()[..bytes_to_copy]);
                        regs.rax = bytes_to_copy as u64; 
                        break;
                    } else {
                        { x86_64::instructions::interrupts::enable_and_hlt(); }
                        x86_64::instructions::interrupts::disable(); 
                    }
                }
            } else { regs.rax = 0; }
        },

        1 => { // ABI de Linux: sys_write
            let fd = regs.rdi as usize; 
            let buffer_ptr = regs.rsi as *const u8; 
            let length = regs.rdx as usize; 

            if !is_valid_user_memory(buffer_ptr as u64, length) {
                regs.rax = (-14i64) as u64; // -EFAULT
                return;
            }

            let mut bytes_written = 0;
            let mut error_code: i64 = 0;

            crate::task::with_task_manager(|tm| {
                if let Some(task_id) = tm.current_task {
                    if let Some(task) = tm.task_registry.get_mut(&task_id) {
                        if let Some(descriptor) = task.fd_table.get_mut(fd) {
                            match descriptor {
                                crate::fs::fd::FileDescriptor::Stdout => {
                                    let slice = unsafe { core::slice::from_raw_parts(buffer_ptr, length) };
                                    if let Ok(text) = core::str::from_utf8(slice) {
                                        crate::print!("{}", text);
                                        bytes_written = length as u64;
                                    }
                                },
                                crate::fs::fd::FileDescriptor::Stderr => {
                                    let slice = unsafe { core::slice::from_raw_parts(buffer_ptr, length) };
                                    if let Ok(text) = core::str::from_utf8(slice) {
                                        crate::print!("\x1b[31m{}\x1b[0m", text);
                                        bytes_written = length as u64;
                                    }
                                },
                                crate::fs::fd::FileDescriptor::Stdin | crate::fs::fd::FileDescriptor::RegularFile { .. } => {
                                    error_code = -9; // -EBADF
                                }
                            }
                        } else { error_code = -9; }
                    } else { error_code = -9; }
                }
            });

            if error_code != 0 { regs.rax = error_code as u64; } 
            else { regs.rax = bytes_written; }
        },

        2 => { // ABI de Linux: sys_open
            let filename_ptr = regs.rdi as *const u8; 
            
            if !is_valid_user_memory(filename_ptr as u64, 256) {
                regs.rax = (-14i64) as u64; // -EFAULT
                return;
            }

            let mut filename_buf = [0u8; 256];
            let mut len = 0;
            unsafe {
                while len < 256 {
                    let byte = *filename_ptr.add(len);
                    if byte == 0 { break; }
                    filename_buf[len] = byte;
                    len += 1;
                }
            }
            let filename = core::str::from_utf8(&filename_buf[..len]).unwrap_or("");

            if let Some(file_vnode) = crate::fs::vfs::open_vnode(filename) {
                let mut new_fd = 0;
                
                crate::task::with_task_manager(|tm| {
                    if let Some(task_id) = tm.current_task {
                        if let Some(task) = tm.task_registry.get_mut(&task_id) {
                            new_fd = task.fd_table.insert(crate::fs::fd::FileDescriptor::RegularFile {
                                vnode: file_vnode,
                                offset: 0, 
                            });
                        }
                    }
                });

                if new_fd > 0 { regs.rax = new_fd as u64; } 
                else { regs.rax = (-24i64) as u64; } // -EMFILE
            } else {
                regs.rax = (-2i64) as u64; // -ENOENT
            }
        },

        3 => { // ABI de Linux: sys_close
            let fd = regs.rdi as usize;
            let mut error_code: i64 = 0;

            crate::task::with_task_manager(|tm| {
                if let Some(task_id) = tm.current_task {
                    if let Some(task) = tm.task_registry.get_mut(&task_id) {
                        if !task.fd_table.close(fd) {
                            error_code = -9; // -EBADF
                        }
                    } else { error_code = -9; }
                } else { error_code = -9; }
            });

            regs.rax = if error_code == 0 { 0 } else { error_code as u64 };
        },

        9 => { // ABI de Linux: sys_mmap
            let addr = regs.rdi;
            let length = regs.rsi;
            let flags = regs.r10;

            // Para la Fase 3.1, los asignadores de memoria de Ring 3 (como malloc) 
            // exigen memoria anónima y privada (MAP_PRIVATE | MAP_ANONYMOUS = 0x22).
            if (flags & 0x20) == 0 {
                regs.rax = (-22i64) as u64; // -EINVAL (Aún no mapeamos archivos al VFS)
                return;
            }

            let mut ret_val = (-12i64) as u64; // Default: -ENOMEM

            crate::task::with_task_manager(|tm| {
                if let Some(task_id) = tm.current_task {
                    if let Some(task) = tm.task_registry.get_mut(&task_id) {
                        
                        // Alinear la longitud solicitada al siguiente múltiplo de 4KB
                        let alloc_size = (length + 0xFFF) & !0xFFF; 
                        
                        // Si el usuario pide addr 0, el kernel decide dónde ponerlo
                        let start_addr = if addr != 0 { addr } else { task.mmap_base };
                        
                        // Validar límites de seguridad de Ring 3
                        if start_addr >= 0x1000 && start_addr.saturating_add(alloc_size) <= 0x00007FFFFFFFFFFF {
                            let pages = alloc_size / 4096;
                            let mut success = true;
                            let target_pml4 = task.pml4_frame;

                            for i in 0..pages {
                                let virt_addr = x86_64::VirtAddr::new(start_addr + (i * 4096));
                                if crate::mm::memory::allocate_and_map_user_page(target_pml4, virt_addr).is_err() {
                                    success = false;
                                    break;
                                }
                            }

                            if success {
                                if addr == 0 {
                                    task.mmap_base += alloc_size; // Mover el puntero para la siguiente llamada
                                }
                                ret_val = start_addr;
                            }
                        }
                    }
                }
            });

            regs.rax = ret_val;
        },

        12 => { // ABI de Linux: sys_brk
            // sys_brk no suele retornar códigos de error negativos estándar en caso de fallo,
            // simplemente devuelve el program_break actual o inmodificado.
            let requested_brk = regs.rdi; 
            let current_task_id = crate::task::TASK_MANAGER.lock().current_task;
            
            if let Some(task_id) = current_task_id {
                let mut tm = crate::task::TASK_MANAGER.lock();
                if let Some(task) = tm.task_registry.get_mut(&task_id) {
                    
                    if requested_brk == 0 {
                        regs.rax = task.program_break;
                        return;
                    }

                    if requested_brk < task.heap_start || requested_brk >= 0x700000000000 {
                        regs.rax = task.program_break; 
                        return;
                    }

                    let current_page_end = (task.program_break + 0xFFF) & !0xFFF;
                    let new_page_end = (requested_brk + 0xFFF) & !0xFFF;

                    if new_page_end > current_page_end {
                        let pages_to_allocate = (new_page_end - current_page_end) / 0x1000;
                        let mut success = true;
                        let target_pml4 = task.pml4_frame;
                        
                        for i in 0..pages_to_allocate {
                            let virt_addr = x86_64::VirtAddr::new(current_page_end + (i * 0x1000));
                            if crate::mm::memory::allocate_and_map_user_page(target_pml4, virt_addr).is_err() {
                                success = false;
                                break;
                            }
                        }

                        if success {
                            task.program_break = requested_brk;
                            regs.rax = requested_brk;
                        } else {
                            regs.rax = task.program_break;
                        }
                    } else {
                        task.program_break = requested_brk;
                        regs.rax = requested_brk;
                    }
                } else { regs.rax = 0; }
            } else { regs.rax = 0; }
        },

        59 => { // ABI de Linux: sys_execve
            let filename_ptr = regs.rdi as *const u8;
            let args_ptr = regs.rsi as *const u8;   
            let args_len = regs.rdx as usize;       
            
            if !is_valid_user_memory(filename_ptr as u64, 64) {
                regs.rax = (-14i64) as u64; // -EFAULT
                return;
            }
            if args_len > 0 && !is_valid_user_memory(args_ptr as u64, args_len) {
                regs.rax = (-14i64) as u64; // -EFAULT
                return;
            }
                
            let mut filename_buf = [0u8; 64];
            let mut len = 0;
            unsafe {
                while len < 64 {
                    let byte = *filename_ptr.add(len);
                    if byte == 0 { break; }
                    filename_buf[len] = byte;
                    len += 1;
                }
            }
            let filename = core::str::from_utf8(&filename_buf[..len]).unwrap_or("");
        
            let mut args_buf = alloc::vec::Vec::new();
            if args_len > 0 && args_len < 4096 {
                args_buf.resize(args_len, 0);
                unsafe { core::ptr::copy_nonoverlapping(args_ptr, args_buf.as_mut_ptr(), args_len); }
            } else {
                args_buf.extend_from_slice(filename.as_bytes());
                args_buf.push(0); 
            }

            if let Some(elf_slice) = crate::fs::vfs::find_file(filename) {
                if let Some(target_pml4) = crate::mm::memory::create_isolated_pml4() {
                    let load_result = crate::task::elf::load_elf(elf_slice, target_pml4);

                    let user_stack_base = 0x7FFFF0000000;
                    let mut user_stack_top = user_stack_base + 0x1000;
                    let _ = crate::mm::memory::allocate_and_map_user_page(
                        target_pml4,
                        x86_64::VirtAddr::new(user_stack_base)
                    );
                    
                    unsafe {
                        let (old_pml4, cr3_flags) = x86_64::registers::control::Cr3::read();
                        x86_64::registers::control::Cr3::write(target_pml4, cr3_flags);

                        user_stack_top -= args_buf.len() as u64;
                        let strings_base = user_stack_top;
                        core::ptr::copy_nonoverlapping(args_buf.as_ptr(), user_stack_top as *mut u8, args_buf.len());

                        let mut argv_pointers = alloc::vec::Vec::new();
                        let mut current_ptr = strings_base;
                        let mut is_new_arg = true;
                        
                        let args_slice = args_buf.as_slice();
                        for i in 0..args_slice.len() {
                            if args_slice[i] == 0 {
                                is_new_arg = true;
                            } else if is_new_arg {
                                argv_pointers.push(current_ptr);
                                is_new_arg = false;
                            }
                            current_ptr += 1;
                        }
                        
                        let argc = argv_pointers.len() as u64;
                        user_stack_top &= !0xF;

                        user_stack_top -= 8; *(user_stack_top as *mut u64) = 0;
                        user_stack_top -= 8; *(user_stack_top as *mut u64) = 0;

                        for &ptr in argv_pointers.iter().rev() {
                            user_stack_top -= 8;
                            *(user_stack_top as *mut u64) = ptr;
                        }

                        user_stack_top -= 8;
                        *(user_stack_top as *mut u64) = argc;
                        x86_64::registers::control::Cr3::write(old_pml4, cr3_flags);
                    }

                    match load_result {
                        Ok(entry_point) => {
                            let entry_fn: fn() -> ! = unsafe { ::core::mem::transmute(entry_point as usize) };
                            crate::task::with_task_manager(|tm| {
                                match tm.spawn_dynamic(entry_fn, target_pml4, user_stack_top) {
                                    Ok(id) => { regs.rax = id.0; },
                                    Err(e) => {
                                        crate::serial_println!("[ERROR] Spawn failed: {:?}", e);
                                        regs.rax = (-11i64) as u64; // -EAGAIN
                                    }
                                }
                            });
                        },
                        Err(e) => { 
                            crate::serial_println!("[ERROR] ELF load failed: {:?}", e);
                            regs.rax = (-8i64) as u64; // -ENOEXEC
                        }
                    }
                } else { regs.rax = (-12i64) as u64; /* -ENOMEM */ }
            } else { regs.rax = (-2i64) as u64; /* -ENOENT */ }
        },

        61 => { // ABI de Linux: sys_wait4
            let pid = regs.rdi; 
            let mut will_block = false;

            crate::task::with_task_manager(|tm| {
                will_block = tm.wait_for_child(crate::task::TaskId(pid));
            });

            if will_block {
                unsafe { crate::task::scheduler::schedule(); }
                regs.rax = pid; 
            } else {
                regs.rax = (-10i64) as u64; // -ECHILD 
            }
        },
        
        60 => { // ABI de Linux: sys_exit
            crate::task::with_task_manager(|tm| {
                tm.exit_current_task();
            });
            loop { { x86_64::instructions::interrupts::enable_and_hlt(); } }
        },

        88 => { // ACPI Poweroff / QEMU Exit
            unsafe {
                let mut port_debug = x86_64::instructions::port::Port::<u32>::new(0xf4);
                port_debug.write(0x10); 
                
                let mut port_a = x86_64::instructions::port::Port::<u16>::new(0x604);
                port_a.write(0x2000); 
            }
            loop { x86_64::instructions::interrupts::enable_and_hlt(); }
        },

        500 => { // ABI de NWIN OS: Syscall 500
            x86_64::instructions::interrupts::without_interrupts(|| {
                if let Some(writer) = crate::drivers::display::WRITER.lock().as_mut() {
                    writer.clear_screen();
                    writer.reset_cursor();
                }
            });
            regs.rax = 0; 
        },

        _ => { regs.rax = (-38i64) as u64; /* -ENOSYS */ }
    }
}