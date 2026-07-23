// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

// src/fs/ext4/extents.rs

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,      // Debe ser 0xF30A
    pub eh_entries: u16,    // Cuántos nodos hay en este nivel
    pub eh_max: u16,        // Capacidad máxima de nodos
    pub eh_depth: u16,      // Profundidad del árbol (0 = apunta directo a datos)
    pub eh_generation: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4Extent {
    pub ee_block: u32,      // Bloque lógico inicial
    pub ee_len: u16,        // Cuántos bloques contiguos abarca este extent
    pub ee_start_hi: u16,   // Dirección física (mitad alta)
    pub ee_start_lo: u32,   // Dirección física (mitad baja)
}