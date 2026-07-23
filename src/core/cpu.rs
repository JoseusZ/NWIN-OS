// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CPU feature enablement: SSE/AVX coprocessor and Local APIC
//! configuration.

use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;

/// Enables SIMD/SSE in CR0/CR4 and forces the legacy 8259 PIC as the
/// active interrupt source by disabling the Local APIC through MSR
/// `0x1B` bit 11.
///
/// Must be called before `gdt::init` so segment state is consistent.
pub fn init() {
    unsafe {
        // Enable the x87/MMX/SSE coprocessor by clearing CR0.EM and
        // setting CR0.MP; let CR4.OSFXSR + OSXMMEXCPT_ENABLE allow
        // SSE instructions to retire without #UD.
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);

        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
    crate::println!("[OK] SIMD/SSE coprocessor enabled.");

    // Bit 11 of MSR 0x1B (IA32_APIC_BASE) clears the "APIC global enable"
    // flag, disabling the Local APIC so the 8259 PIC routes interrupts.
    let mut apic_base = Msr::new(0x1B);
    unsafe {
        let mut val = apic_base.read();
        val &= !(1 << 11);
        apic_base.write(val);
    }
    crate::println!("[OK] Local APIC disabled; legacy 8259 PIC routing active.");
}