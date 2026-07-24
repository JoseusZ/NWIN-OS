// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Raw linear framebuffer abstraction.
//!
//! Layer 1 of the display stack: a minimal wrapper around the
//! memory-mapped framebuffer that Limine/UEFI hands us, exposing
//! only the operations the TTY layer above needs.

use font8x8::{BASIC_FONTS, UnicodeFonts};

/// Layer 1: hardware abstraction over a linear framebuffer.
pub struct FrameBuffer {
    pub ptr: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bytes_per_pixel: usize,
}

unsafe impl Send for FrameBuffer {}
unsafe impl Sync for FrameBuffer {}

impl FrameBuffer {
    /// Builds a framebuffer over the linear pixel buffer at `ptr`,
    /// with the given resolution, `pitch` (bytes per scanline) and
    /// `bpp` (bits per pixel; the constructor converts to bytes).
    pub fn new(ptr: *mut u8, width: usize, height: usize, pitch: usize, bpp: usize) -> Self {
        Self { ptr, width, height, pitch, bytes_per_pixel: bpp / 8 }
    }

    /// Plots a single RGB pixel at `(x, y)`, silently ignoring
    /// out-of-bounds coordinates.
    #[inline(always)]
    pub fn draw_pixel(&mut self, x: usize, y: usize, color: [u8; 3]) {
        if x < self.width && y < self.height {
            let offset = y * self.pitch + x * self.bytes_per_pixel;
            unsafe {
                *(self.ptr.add(offset))     = color[0];
                *(self.ptr.add(offset + 1)) = color[1];
                *(self.ptr.add(offset + 2)) = color[2];
            }
        }
    }

    /// Fills a solid `w x h` rectangle with `color`, useful for
    /// clearing the screen, erasing glyphs and drawing the cursor.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: [u8; 3]) {
        for row_i in 0..h {
            for col_i in 0..w {
                self.draw_pixel(x + col_i, y + row_i, color);
            }
        }
    }

    /// Ultra-fast whole-screen clear to `color`.
    pub fn clear(&mut self, color: [u8; 3]) {
        for y in 0..self.height {
            for x in 0..self.width {
                let offset = y * self.pitch + x * self.bytes_per_pixel;
                unsafe {
                    *(self.ptr.add(offset))     = color[0];
                    *(self.ptr.add(offset + 1)) = color[1];
                    *(self.ptr.add(offset + 2)) = color[2];
                }
            }
        }
    }

    /// Rasterises a single 8x8 glyph from the `font8x8` font table
    /// at `(x, y)`, using `fg` for set bits and `bg` for cleared bits.
    pub fn draw_char(&mut self, x: usize, y: usize, byte: u8, fg: [u8; 3], bg: [u8; 3]) {
        let glyph = BASIC_FONTS.get(byte as char).unwrap_or([0; 8]); 
        for (row_i, row) in glyph.iter().enumerate() {
            for bit_i in 0..8 {
                let is_active = *row & (1 << bit_i) != 0;
                let color = if is_active { fg } else { bg };
                self.draw_pixel(x + bit_i, y + row_i, color);
            }
        }
    }
}