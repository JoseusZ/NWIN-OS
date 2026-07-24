// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device drivers and hardware adapters.
//!
//! - [`block`]: block-device drivers (currently AHCI for SATA disks).
//! - [`bus`]: system buses (PCI enumeration).
//! - [`char`]: character devices (16550A-compatible serial UART).
//! - [`display`]: framebuffer and TTY output.
//! - [`input`]: input devices (PS/2 keyboard).
//! - [`timer`]: programmable interval timer (PIT).
//!
//! The most commonly consumed pieces are re-exported at the crate
//! root: [`pci`] (bus scan) and [`serial`] (kernel log sink).

pub mod block;
pub mod bus;
pub mod char;
pub mod display;
pub mod input;
pub mod timer;

pub use self::bus::pci;
pub use self::char::serial;
pub use self::input::keyboard;
pub use self::timer::pit;