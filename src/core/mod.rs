// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core kernel primitives: CPU state, descriptor tables, IRQ/exception
//! dispatch, the `syscall` ABI, the panic handler, the typed error
//! tree (`KernelError` and friends) and the kernel mutex wrappers.

pub mod cpu;
pub mod error;
pub mod gdt;
pub mod idt;
pub mod panic;
pub mod sync;
pub mod syscall;