// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1, KeyCode};
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::{AtomicUsize, Ordering};

// ====================================================================
// 1. LA CAJA FUERTE (Ring Buffer Atómico 100% Bare-Metal)
// ====================================================================
const BUF_SIZE: usize = 256;
// Arreglo estático de tamaño fijo. ¡Cero dependencias, cero Heap!
static mut BUFFER: [char; BUF_SIZE] = ['\0'; BUF_SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

/// Guarda una tecla de forma segura sin usar bloqueos.
pub fn push_key(c: char) {
    let tail = TAIL.load(Ordering::Relaxed);
    let next_tail = (tail + 1) % BUF_SIZE;
    
    // Si la cola no está llena, guardamos la letra
    if next_tail != HEAD.load(Ordering::Acquire) {
        unsafe { BUFFER[tail] = c; }
        TAIL.store(next_tail, Ordering::Release);
    }
}

/// Extrae la tecla más antigua. Devuelve None si está vacío.
pub fn pop_key() -> Option<char> {
    let head = HEAD.load(Ordering::Relaxed);
    
    // Si head alcanzó a tail, no hay teclas nuevas
    if head == TAIL.load(Ordering::Acquire) {
        None
    } else {
        let c = unsafe { BUFFER[head] };
        HEAD.store((head + 1) % BUF_SIZE, Ordering::Release);
        Some(c)
    }
}

// ====================================================================
// 2. LA MÁQUINA DE ESTADOS
// ====================================================================
lazy_static! {
    pub static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::MapLettersToUnicode, 
        ));
}

// ====================================================================
// 3. PRODUCTOR: El hardware mete teclas aquí (Ring 0 -> Interrupción)
// ====================================================================
pub fn process_scancode(scancode: u8) {
    let mut keyboard = KEYBOARD.lock();
    
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    // \x03 es el código ASCII para End of Text (Ctrl+C)
                    if character == '\x03' {
                        crate::task::with_task_manager(|tm| {
                            if let Some(id) = tm.current_task {
                                if id.0 > 1 {
                                    // CORRECCIÓN VITAL: No borrar la tarea violentamente.
                                    // Marcarla como Dead y desconectarla de la CPU suavemente.
                                    tm.exit_current_task();
                                    crate::println!("^C");
                                }
                            }
                        });
                    } else {
                        push_key(character);
                    }
                },
                DecodedKey::RawKey(key) => match key {
                    KeyCode::Return => push_key('\n'),
                    KeyCode::Backspace => push_key('\x08'),
                    // --- MAPEO ANSI PARA FLECHAS ---
                    KeyCode::ArrowUp => { push_key('\x1b'); push_key('['); push_key('A'); },
                    KeyCode::ArrowDown => { push_key('\x1b'); push_key('['); push_key('B'); },
                    KeyCode::ArrowRight => { push_key('\x1b'); push_key('['); push_key('C'); },
                    KeyCode::ArrowLeft => { push_key('\x1b'); push_key('['); push_key('D'); },
                    _ => {}
                },
            }
        }
    }
}