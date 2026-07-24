// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display stack: the raw [`FrameBuffer`] ([`fb`]) and the
//! TTY/ANSI writer ([`tty`]) that sits on top of it, plus the
//! global `WRITER` singleton, the `print!` / `println!` macros
//! the rest of the kernel uses, and the [`init`] bring-up entry
//! point that consumes the Limine framebuffer request.

pub mod fb;
pub mod tty;

use spin::Mutex;

// Re-export the TTY `Writer` so syscall.rs and the rest of the kernel can refer to it directly.
pub use tty::Writer;

// ==========================================
// GLOBAL MUTEX
// ==========================================
pub static WRITER: Mutex<Option<Writer>> = Mutex::new(None);

// ==========================================
// PUBLIC MACROS
// ==========================================

/// Prints formatted text to the active TTY.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::drivers::display::_print(format_args!($($arg)*)));
}

/// Prints formatted text to the active TTY followed by a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(writer) = WRITER.lock().as_mut() {
            // Clean cycle: erase cursor, write, redraw cursor.
            writer.erase_cursor();
            writer.write_fmt(args).unwrap();
            writer.draw_cursor();
        }
    });
}

// ==========================================
// INITIALISATION
// ==========================================

/// Brings up the display stack using the framebuffer that Limine
/// reported in its `FramebufferRequest`.
pub fn init() {
    if let Some(framebuffer_response) = crate::FRAMEBUFFER_REQUEST.response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().first() {
            let width = framebuffer.width as usize;
            let height = framebuffer.height as usize;
            let pitch = framebuffer.pitch as usize;
            let bpp = framebuffer.bpp as usize;
            let fb_ptr = framebuffer.address() as *mut u8;

            // Clear the framebuffer to black before installing the Writer.
            for y in 0..height {
                for x in 0..width {
                    let offset = y * pitch + x * (bpp / 8);
                    unsafe {
                        *(fb_ptr.add(offset)) = 0;
                        *(fb_ptr.add(offset + 1)) = 0;
                        *(fb_ptr.add(offset + 2)) = 0;
                    }
                }
            }

            let writer = Writer::new(fb_ptr, width, height, pitch, bpp);
            *WRITER.lock() = Some(writer);
        }
    }
}