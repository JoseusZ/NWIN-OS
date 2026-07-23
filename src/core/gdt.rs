// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Global Descriptor Table (GDT) and Task State Segment (TSS).
//!
//! Defines the kernel / user code and data segments, the TSS with two
//! hardened stacks (privilege-level-0 entry stack and IST stack for
//! double-fault recovery), and the selector set exported to the
//! `syscall` initialiser.

use lazy_static::lazy_static;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// IST slot used by the double-fault handler. The CPU switches to the
/// IST stack defined here before delivering `#DF`.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        // Privilege-level-0 stack. The CPU expects a static stack when
        // transitioning from Ring 3 to Ring 0 on interrupt delivery.
        tss.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut RSP0_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(core::ptr::addr_of!(RSP0_STACK));
            stack_start + STACK_SIZE
        };

        // IST entry reserved for the double-fault handler.
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            // x86_64 stacks grow downward: pass the high address.
            let stack_end =
                VirtAddr::from_ptr(core::ptr::addr_of!(STACK)) + STACK_SIZE;

            stack_end
        };
        tss
    };
}

lazy_static! {
    /// Global Descriptor Table paired with the segment selectors that
    /// downstream subsystems (notably `syscall::init`) read.
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();

        let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());

        // For `sysretq` the user data selector must precede the user
        // code selector in the GDT; the `syscall` MSR pair relies on this.
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());

        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        (gdt, Selectors {
            kernel_code_selector,
            kernel_data_selector,
            user_code_selector,
            user_data_selector,
            tss_selector,
        })
    };
}

/// Segment selectors produced by the GDT layout above. Re-exported so
/// the syscall initialiser can load the user-mode selectors into
/// `STAR`.
#[allow(dead_code)]
pub struct Selectors {
    pub kernel_code_selector: SegmentSelector,
    pub kernel_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

/// Loads the GDT into the CPU, switches all data-segment registers to
/// the kernel selector and installs the TSS. Must be called exactly
/// once during boot before enabling interrupts.
pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        ES::set_reg(GDT.1.kernel_data_selector);
        SS::set_reg(GDT.1.kernel_data_selector);

        // FS and GS are not used for base protection on x86_64; Linux
        // re-purposes them for TLS. Point them at kernel data until a
        // dedicated TLS scheme lands.
        FS::set_reg(GDT.1.kernel_data_selector);
        GS::set_reg(GDT.1.kernel_data_selector);

        load_tss(GDT.1.tss_selector);
    }
}

/// Updates the RSP0 entry of the TSS, which the CPU uses as the
/// kernel stack when crossing from Ring 3 into a syscall or interrupt.
pub unsafe fn set_tss_rsp0(rsp0: VirtAddr) {
    let tss_ptr = &*TSS as *const _ as *mut TaskStateSegment;
    (*tss_ptr).privilege_stack_table[0] = rsp0;
}