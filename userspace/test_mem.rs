#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

// ==========================================
// SYSTEM CALL ABI ABSTRACTIONS
// ==========================================

#[inline(always)]
unsafe fn sys_write(fd: usize, buf: *const u8, count: usize) -> usize {
    let mut ret: usize;
    asm!(
        "syscall",
        inout("rax") 1usize => ret,
        in("rdi") fd, in("rsi") buf, in("rdx") count,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn sys_mmap(addr: usize, length: usize, prot: usize, flags: usize, fd: usize, offset: usize) -> usize {
    let mut ret: usize;
    asm!(
        "syscall",
        inout("rax") 9usize => ret,
        in("rdi") addr, in("rsi") length, in("rdx") prot,
        in("r10") flags, in("r8") fd, in("r9") offset,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn sys_brk(addr: usize) -> usize {
    let mut ret: usize;
    asm!(
        "syscall",
        inout("rax") 12usize => ret,
        in("rdi") addr,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
unsafe fn sys_exit(code: usize) -> ! {
    asm!("syscall", in("rax") 60usize, in("rdi") code, options(noreturn));
    loop {}
}

// ==========================================
// UTILITY FUNCTIONS
// ==========================================

fn print(s: &str) {
    unsafe { sys_write(1, s.as_ptr(), s.len()) };
}

fn print_hex(val: usize) {
    let mut buf = [b'0'; 16];
    let hex_chars = b"0123456789ABCDEF";
    let mut temp = val;
    for i in (0..16).rev() {
        buf[i] = hex_chars[(temp & 0xF) as usize];
        temp >>= 4;
    }
    unsafe { sys_write(1, buf.as_ptr(), 16) };
}

// ==========================================
// ENTRY POINT (RING 3)
// ==========================================

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print("\x1b[1;36m[TEST 3.1]\x1b[0m Starting dynamic memory allocation suite...\n");

    // --- TEST 1: SYS_BRK ---
    print("\x1b[1;33m[INFO]\x1b[0m Testing sys_brk (Syscall 12)...\n");
    unsafe {
        let initial_brk = sys_brk(0);
        if initial_brk == 0 {
            print("\x1b[1;31m[FAIL]\x1b[0m sys_brk returned 0.\n");
            sys_exit(1);
        }

        let requested_brk = initial_brk + 4096;
        let new_brk = sys_brk(requested_brk);

        if new_brk != requested_brk {
            print("\x1b[1;31m[FAIL]\x1b[0m sys_brk refused memory expansion.\n");
            sys_exit(1);
        }

        let ptr_brk = initial_brk as *mut u64;
        *ptr_brk = 0xDEADBEEFCAFEBABE; // Write to virgin memory

        if *ptr_brk == 0xDEADBEEFCAFEBABE {
            print("\x1b[1;32m[PASS]\x1b[0m sys_brk successfully mapped physical pages.\n");
        } else {
            print("\x1b[1;31m[FAIL]\x1b[0m sys_brk memory corruption detected.\n");
            sys_exit(1);
        }
    }

    // --- TEST 2: SYS_MMAP ---
    print("\x1b[1;33m[INFO]\x1b[0m Testing sys_mmap (Syscall 9)...\n");
    unsafe {
        // MAP_PRIVATE (0x02) | MAP_ANONYMOUS (0x20) = 0x22
        let mmap_addr = sys_mmap(0, 4096, 0x3, 0x22, usize::MAX, 0);
        
        if mmap_addr as isize <= 0 {
            print("\x1b[1;31m[FAIL]\x1b[0m sys_mmap rejected the mapping.\n");
            sys_exit(1);
        }

        print("       Address granted by kernel: 0x");
        print_hex(mmap_addr);
        print("\n");

        let ptr_mmap = mmap_addr as *mut u64;
        *ptr_mmap = 0x1234567890ABCDEF; // Write to isolated memory

        if *ptr_mmap == 0x1234567890ABCDEF {
            print("\x1b[1;32m[PASS]\x1b[0m sys_mmap successfully mapped anonymous pages.\n");
        } else {
            print("\x1b[1;31m[FAIL]\x1b[0m sys_mmap memory corruption detected.\n");
            sys_exit(1);
        }
    }

    print("\x1b[1;32m[SUCCESS]\x1b[0m Phase 3.1 memory tests completed without faults.\n");
    unsafe { sys_exit(0) };
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("\x1b[1;31m[PANIC]\x1b[0m Test suite crashed unexpectedly.\n");
    unsafe { sys_exit(1) }
}