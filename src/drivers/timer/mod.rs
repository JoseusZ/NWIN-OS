// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Timer devices.
//!
//! Currently only the 8253/8254 PIT driver ([`pit`]) lives here.
//! HPET, TSC and APIC timers will follow.

pub mod pit;
