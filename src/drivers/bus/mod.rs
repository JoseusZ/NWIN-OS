// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! System buses.
//!
//! Currently only PCI enumeration lives here ([`pci`]); other buses
//! (USB, I2C, SPI) will follow.

pub mod pci;