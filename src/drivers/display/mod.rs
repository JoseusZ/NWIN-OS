pub mod fb;
pub mod tty;

use spin::Mutex;

// Exportamos Writer para que syscall.rs y el resto del kernel lo vean igual que antes
pub use tty::Writer;

// ==========================================
// EL MUTEX GLOBAL
// ==========================================
pub static WRITER: Mutex<Option<Writer>> = Mutex::new(None);

// ==========================================
// MACROS PÚBLICAS
// ==========================================
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::drivers::display::_print(format_args!($($arg)*)));
}

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
            // El ciclo limpio: borrar cursor, escribir, dibujar cursor
            writer.erase_cursor();
            writer.write_fmt(args).unwrap();
            writer.draw_cursor();
        }
    });
}

// ==========================================
// INICIALIZACIÓN
// ==========================================
pub fn init() {
    if let Some(framebuffer_response) = crate::FRAMEBUFFER_REQUEST.response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().first() {
            let width = framebuffer.width as usize;
            let height = framebuffer.height as usize;
            let pitch = framebuffer.pitch as usize;
            let bpp = framebuffer.bpp as usize;
            let fb_ptr = framebuffer.address() as *mut u8;

            // Limpia el framebuffer a color negro inicialmente
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