// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;

pub fn init() {
    unsafe {
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);

        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR); 
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
    crate::println!("[OK] Coprocesador SIMD/SSE habilitado.");

    let mut apic_base = Msr::new(0x1B);
    unsafe {
        let mut val = apic_base.read();
        val &= !(1 << 11); 
        apic_base.write(val);
    }
    crate::println!("[OK] Local APIC apagado. Enrutamiento legado activo.");
}