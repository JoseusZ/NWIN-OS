use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;

/// Estructura que representa la capa física del puerto serie 8250 UART
pub struct SerialPort {
    data: Port<u8>,
    int_en: Port<u8>,
    fifo_ctrl: Port<u8>,
    line_ctrl: Port<u8>,
    modem_ctrl: Port<u8>,
    line_sts: Port<u8>,
}

impl SerialPort {
    /// Inicializa las direcciones de los puertos basándose en el puerto base (ej. 0x3F8 para COM1)
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

    /// Configura el hardware con el protocolo estandar 8N1 a 38400 baudios
    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00);    // 1. Desactivar todas las interrupciones del chip
            self.line_ctrl.write(0x80); // 2. Activar bit DLAB (Para configurar velocidad)
            self.data.write(0x03);      // 3. Divisor bajo (38400 baudios)
            self.int_en.write(0x00);    //    Divisor alto
            self.line_ctrl.write(0x03); // 4. 8 bits de datos, sin paridad, 1 bit de parada (8N1)
            self.fifo_ctrl.write(0xC7); // 5. Habilitar FIFOs, limpiar buffers TX/RX, umbral 14 bytes
            self.modem_ctrl.write(0x0B); // 6. Marcar el hardware como listo (RTS/DSR)
        }
    }

    /// Espera en un spin loop hasta que el buffer del transmisor hardware esté vacío
    fn wait_for_tx_empty(&mut self) {
        unsafe {
            // El bit 5 del Line Status Register nos dice si podemos enviar datos
            while (self.line_sts.read() & 0x20) == 0 {
                core::hint::spin_loop(); // Le dice a la CPU que estamos esperando activamente
            }
        }
    }

    /// Envía un único byte al puerto
    pub fn send(&mut self, data: u8) {
        match data {
            // En hardware de terminal clásico, un salto de línea requiere 'Retorno de Carro' y 'Salto de Línea'
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

// Implementamos core::fmt::Write para poder usar la macro format_args! de Rust
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

// ==========================================
// CAPA DE SINCRONIZACIÓN Y MACROS
// ==========================================

lazy_static! {
    /// Bóveda global del puerto COM1. 
    /// Se inicializa automáticamente la primera vez que se llama a serial_print!
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = SerialPort::new(0x3F8); // 0x3F8 es el estándar inquebrantable para COM1
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // EL ESCUDO ANTI-DEADLOCK:
    // Deshabilitamos interrupciones de hardware locales justo antes de pedir el Mutex.
    // Si no hacemos esto y una interrupción del temporizador salta mientras tenemos el candado,
    // el manejador de interrupciones intentará imprimir, esperará el Mutex eternamente y la máquina colapsará.
    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).expect("Fallo crítico al imprimir en el puerto serie");
    });
}

/// Imprime en el puerto serie anfitrión
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::drivers::serial::_print(format_args!($($arg)*))
    };
}

/// Imprime en el puerto serie anfitrión con un salto de línea
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*
    ));
}