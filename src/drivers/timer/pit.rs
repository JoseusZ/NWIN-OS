// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! 8253/8254 Programmable Interval Timer (PIT) driver.
//!
//! Configures channel 0 to fire IRQ0 at 100 Hz, which the kernel
//! uses as the system tick for the scheduler.

use x86_64::instructions::port::Port;

/// Initialises the PIT (system clock) at 100 Hz.
pub fn init() {
    let mut command_port: Port<u8> = Port::new(0x43);
    let mut data_port: Port<u8> = Port::new(0x40);

    unsafe {
        command_port.write(0x36);
        let divisor: u16 = 11931; // Base frequency (1.193182 MHz) / 100 Hz.
        data_port.write((divisor & 0xFF) as u8);
        data_port.write((divisor >> 8) as u8);
    }
    crate::println!("[OK] PIT clock started at 100Hz.");
}