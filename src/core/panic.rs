// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kernel panic handler.
//!
//! Disables interrupts, unlocks the serial port, prints a structured
//! diagnostic to COM1 (location, optional typed `KernelError` payload,
//! or the formatted message), then halts the CPU.

use core::panic::PanicInfo;
use crate::core::error::KernelError;
use crate::serial_println;

/// Global panic entry point registered with `#[panic_handler]`.
///
/// Sequence:
///
/// 1. Mask interrupts so no other task can re-enter the handler.
/// 2. Force-unlock the serial spin lock (the panic call site may have
///    been holding it).
/// 3. Print a banner, the panic `location`, and either the typed
///    `KernelError` payload (when present) or the formatted panic
///    message.
/// 4. Spin forever on `hlt` so the CPU idles at minimal power.
#[panic_handler]
#[allow(deprecated)]
fn panic(info: &PanicInfo) -> ! {
    x86_64::instructions::interrupts::disable();
    unsafe { crate::drivers::serial::SERIAL1.force_unlock() };

    serial_println!("\n==================================================");
    serial_println!("              !!! KERNEL PANIC !!!                ");
    serial_println!("==================================================");

    if let Some(location) = info.location() {
        serial_println!("Location: {}", location);
    } else {
        serial_println!("Location: <unknown>");
    }

    // `PanicInfo::payload()` is deprecated upstream on stable (it never
    // returns anything useful); the `else` branch keeps the handler
    // working with the formatted `Arguments` payload that current
    // toolchains produce. The `if let` arm is ready for the moment the
    // API ships with a usable payload and call sites can pass `&err`.
    if let Some(err) = info.payload().downcast_ref::<KernelError>() {
        serial_println!("KernelError Payload: {}", err);
    } else {
        serial_println!("Message: {}", info.message());
    }

    serial_println!("==================================================");
    serial_println!("System halted.");

    loop {
        x86_64::instructions::hlt();
    }
}