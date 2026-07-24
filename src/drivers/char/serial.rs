// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! 16550A-compatible UART serial port driver.
//!
//! Provides a tiny abstraction over the legacy 8250/16550 register
//! file, the [`SERIAL1`] singleton for COM1 (`0x3F8`), and the
//! `serial_print!` / `serial_println!` macros that the rest of the
//! kernel uses as its log sink.

use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;

/// Physical layer of the 16550 UART: one I/O port per register.
pub struct SerialPort {
    data: Port<u8>,
    int_en: Port<u8>,
    fifo_ctrl: Port<u8>,
    line_ctrl: Port<u8>,
    modem_ctrl: Port<u8>,
    line_sts: Port<u8>,
}

impl SerialPort {
    /// Builds a `SerialPort` rooted at `port_base` (e.g. `0x3F8` for COM1).
    pub const fn new(port_base: u16) -> Self {
        Self {
            data: Port::new(port_base),
            int_en: Port::new(port_base + 1),
            fifo_ctrl: Port::new(port_base + 2),
            line_ctrl: Port::new(port_base + 3),
            modem_ctrl: Port::new(port_base + 4),
            line_sts: Port::new(port_base + 5),
        }
    }

    /// Configures the hardware with the standard 8N1 protocol at 38400 baud.
    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00);    // 1. Disable all chip interrupts.
            self.line_ctrl.write(0x80); // 2. Set the DLAB bit to unlock the baud-rate divisor.
            self.data.write(0x03);      // 3. Low divisor byte (38400 baud).
            self.int_en.write(0x00);    //    High divisor byte.
            self.line_ctrl.write(0x03); // 4. 8 data bits, no parity, 1 stop bit (8N1).
            self.fifo_ctrl.write(0xC7); // 5. Enable FIFOs, clear TX/RX buffers, 14-byte threshold.
            self.modem_ctrl.write(0x0B); // 6. Mark hardware as ready (RTS/DSR).
        }
    }

    /// Spins until the hardware transmit buffer is empty.
    fn wait_for_tx_empty(&mut self) {
        unsafe {
            // Bit 5 of the Line Status Register tells us when the transmitter is ready.
            while (self.line_sts.read() & 0x20) == 0 {
                core::hint::spin_loop(); // Tell the CPU we are actively waiting.
            }
        }
    }

    /// Sends a single byte to the port.
    pub fn send(&mut self, data: u8) {
        match data {
            // Classic terminal hardware requires both a Carriage Return
            // and a Line Feed for a newline.
            b'\n' => {
                self.wait_for_tx_empty();
                unsafe { self.data.write(b'\r'); }
                self.wait_for_tx_empty();
                unsafe { self.data.write(b'\n'); }
            }
            _ => {
                self.wait_for_tx_empty();
                unsafe { self.data.write(data); }
            }
        }
    }
}

// Implement core::fmt::Write so the `format_args!` macro can drive the port.
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

// ==========================================
// SYNCHRONISATION LAYER AND MACROS
// ==========================================

lazy_static! {
    /// Global vault for the COM1 port.
    /// Initialised on first use of `serial_print!`.
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = SerialPort::new(0x3F8); // 0x3F8 is the canonical, unbreakable standard for COM1.
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // THE ANTI-DEADLOCK SHIELD:
    // Disable local hardware interrupts right before taking the Mutex.
    // Without this, a timer interrupt could fire while we hold the
    // lock and the IRQ handler would try to print, blocking forever
    // on the Mutex and hanging the machine.
    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).expect("Critical failure while printing to the serial port");
    });
}

/// Prints to the host serial port.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::drivers::serial::_print(format_args!($($arg)*))
    };
}

/// Prints to the host serial port followed by a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*
    ));
}