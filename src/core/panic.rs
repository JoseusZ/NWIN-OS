use core::panic::PanicInfo;
use crate::serial_println;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Apagamos interrupciones de inmediato para que ningún otro 
    // hilo o evento intente usar la CPU mientras morimos.
    x86_64::instructions::interrupts::disable();

    // force_unlock is unsafe: wrap in an unsafe block per its contract
    unsafe { crate::drivers::serial::SERIAL1.force_unlock() };

    serial_println!("\n==================================================");
    serial_println!("              !!! KERNEL PANIC !!!                ");
    serial_println!("==================================================");
    
    // Imprime el archivo, la línea y el mensaje de error exacto
    serial_println!("{:#?}", info);
    
    serial_println!("==================================================");
    serial_println!("Sistema detenido (HALT).");

    // Bucle infinito de bajo consumo
    loop {
        x86_64::instructions::hlt();
    }
}