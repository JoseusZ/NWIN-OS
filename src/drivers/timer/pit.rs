// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/drivers/pit.rs
use x86_64::instructions::port::Port;

/// Inicializa el Programmable Interval Timer (Reloj del Sistema) a 100Hz
pub fn init() {
    let mut command_port: Port<u8> = Port::new(0x43);
    let mut data_port: Port<u8> = Port::new(0x40);

    unsafe {
        command_port.write(0x36);
        let divisor: u16 = 11931; // Frecuencia base (1.193182 MHz) / 100 Hz
        data_port.write((divisor & 0xFF) as u8);
        data_port.write((divisor >> 8) as u8);
    }
    crate::println!("[OK] Reloj PIT arrancado a 100Hz.");
}