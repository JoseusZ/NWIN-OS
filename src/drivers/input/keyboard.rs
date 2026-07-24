// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! PS/2 keyboard driver.
//!
//! A single-byte ring buffer of decoded [`char`]s, fed by the IRQ
//! handler via [`process_scancode`] and drained by the user shell
//! via [`pop_key`].

use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1, KeyCode};
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::{AtomicUsize, Ordering};

// ====================================================================
// 1. THE STRONG BOX (Lock-free Atomic Ring Buffer, 100% Bare-Metal)
// ====================================================================
const BUF_SIZE: usize = 256;
// Fixed-size static array. Zero dependencies, zero heap!
static mut BUFFER: [char; BUF_SIZE] = ['\0'; BUF_SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

/// Stores a key in the ring buffer in a lock-free manner.
pub fn push_key(c: char) {
    let tail = TAIL.load(Ordering::Relaxed);
    let next_tail = (tail + 1) % BUF_SIZE;

    // If the queue is not full, store the character.
    if next_tail != HEAD.load(Ordering::Acquire) {
        unsafe { BUFFER[tail] = c; }
        TAIL.store(next_tail, Ordering::Release);
    }
}

/// Removes the oldest key from the buffer. Returns `None` when empty.
pub fn pop_key() -> Option<char> {
    let head = HEAD.load(Ordering::Relaxed);

    // If head caught up with tail, there are no new keys.
    if head == TAIL.load(Ordering::Acquire) {
        None
    } else {
        let c = unsafe { BUFFER[head] };
        HEAD.store((head + 1) % BUF_SIZE, Ordering::Release);
        Some(c)
    }
}

// ====================================================================
// 2. THE STATE MACHINE
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
// 3. PRODUCER: hardware feeds keys here (Ring 0 -> Interrupt)
// ====================================================================
pub fn process_scancode(scancode: u8) {
    let mut keyboard = KEYBOARD.lock();
    
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    // \x03 is the ASCII code for End of Text (Ctrl+C).
                    if character == '\x03' {
                        crate::task::with_task_manager(|tm| {
                            if let Some(id) = tm.current_task {
                                if id.0 > 1 {
                                    // VITAL CORRECTION: do not yank the task violently.
                                    // Mark it as Dead and detach it from the CPU gently.
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
                    // --- ANSI MAPPING FOR ARROW KEYS ---
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