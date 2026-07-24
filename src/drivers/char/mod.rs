// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Character devices.
//!
//! Currently only the 16550A-compatible UART driver ([`serial`])
//! lives here. PS/2 keyboard and other input devices belong to the
//! [`crate::drivers::input`] module.

pub mod serial;
