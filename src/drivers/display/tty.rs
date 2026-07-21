use core::fmt;
use super::fb::FrameBuffer;

// --- CONSTANTES ---
const BG_DEFAULT: [u8; 3] = [0, 0, 0];
const FG_DEFAULT: [u8; 3] = [170, 170, 170];
const ANSI_COLORS: [[u8; 3]; 8] = [
    [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170],
    [170, 0, 0], [170, 0, 170], [170, 170, 0], [170, 170, 170],
];

#[derive(Clone, Copy, PartialEq)]
enum AnsiState { Normal, Escape, Csi }

/// Capa 2: Emulador Lógico de Terminal (TTY)
pub struct Writer {
    fb: FrameBuffer,
    x_pos: usize,
    y_pos: usize,
    fg_color: [u8; 3],
    bg_color: [u8; 3],
    ansi_state: AnsiState,
    ansi_params: [u32; 4],
    ansi_param_idx: usize,
    ansi_current_val: u32,
}

impl Writer {
    pub fn new(fb_ptr: *mut u8, width: usize, height: usize, pitch: usize, bpp: usize) -> Self {
        let fb = FrameBuffer::new(fb_ptr, width, height, pitch, bpp);
        let mut writer = Writer { 
            fb, x_pos: 0, y_pos: 0,
            fg_color: FG_DEFAULT, bg_color: BG_DEFAULT,
            ansi_state: AnsiState::Normal,
            ansi_params: [0; 4], ansi_param_idx: 0, ansi_current_val: 0,
        };
        writer.clear_screen(); 
        writer.draw_cursor();
        writer
    }

    pub fn reset_cursor(&mut self) { self.x_pos = 0; self.y_pos = 0; }
    pub fn clear_screen(&mut self) { self.fb.clear(self.bg_color); }
    
    // El Framebuffer hace que el cursor sea súper simple de dibujar
    pub fn draw_cursor(&mut self) { self.fb.fill_rect(self.x_pos, self.y_pos + 6, 8, 2, FG_DEFAULT); }
    pub fn erase_cursor(&mut self) { self.fb.fill_rect(self.x_pos, self.y_pos + 6, 8, 2, self.bg_color); }

    pub fn write_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                match byte {
                    0x1B => self.ansi_state = AnsiState::Escape,
                    b'\n' => self.new_line(),
                    b'\r' => self.x_pos = 0,
                    0x08 => self.backspace(),
                    _ => if byte >= 0x20 && byte <= 0x7E { self.print_char(byte); }
                }
            },
            AnsiState::Escape => {
                if byte == b'[' {
                    self.ansi_state = AnsiState::Csi;
                    self.ansi_params = [0; 4];
                    self.ansi_param_idx = 0;
                    self.ansi_current_val = 0;
                } else { self.ansi_state = AnsiState::Normal; }
            },
            AnsiState::Csi => {
                match byte {
                    b'0'..=b'9' => self.ansi_current_val = self.ansi_current_val.wrapping_mul(10).wrapping_add((byte - b'0') as u32),
                    b';' => {
                        if self.ansi_param_idx < 4 {
                            self.ansi_params[self.ansi_param_idx] = self.ansi_current_val;
                            self.ansi_param_idx += 1;
                        }
                        self.ansi_current_val = 0;
                    },
                    cmd_char => {
                        if self.ansi_param_idx < 4 {
                            self.ansi_params[self.ansi_param_idx] = self.ansi_current_val;
                            self.ansi_param_idx += 1;
                        }
                        self.execute_ansi_cmd(cmd_char);
                        self.ansi_state = AnsiState::Normal;
                    }
                }
            }
        }
    }

    fn execute_ansi_cmd(&mut self, cmd: u8) {
        match cmd {
            b'm' => {
                for i in 0..self.ansi_param_idx {
                    match self.ansi_params[i] {
                        0 => { self.fg_color = FG_DEFAULT; self.bg_color = BG_DEFAULT; },
                        code @ 30..=37 => self.fg_color = ANSI_COLORS[(code - 30) as usize],
                        code @ 40..=47 => self.bg_color = ANSI_COLORS[(code - 40) as usize],
                        _ => {}
                    }
                }
            },
            b'J' => if self.ansi_params[0] == 2 { self.clear_screen(); self.reset_cursor(); },
            b'H' => {
                let y = if self.ansi_params[0] > 0 { self.ansi_params[0] as usize - 1 } else { 0 };
                let x = if self.ansi_params[1] > 0 { self.ansi_params[1] as usize - 1 } else { 0 };
                self.x_pos = x * 8; self.y_pos = y * 8;
            },
            b'K' => { // Borrado limpio hacia la derecha
                let mode = if self.ansi_param_idx > 0 { self.ansi_params[0] } else { 0 };
                if mode == 0 {
                    let remaining_width = self.fb.width.saturating_sub(self.x_pos);
                    self.fb.fill_rect(self.x_pos, self.y_pos, remaining_width, 8, self.bg_color);
                }
            },
            _ => {}
        }
    }

    fn print_char(&mut self, byte: u8) {
        if self.x_pos + 8 >= self.fb.width { self.new_line(); }
        self.fb.draw_char(self.x_pos, self.y_pos, byte, self.fg_color, self.bg_color);
        self.x_pos += 8;
    }

    fn backspace(&mut self) {
        if self.x_pos >= 8 {
            self.x_pos -= 8;
        } else if self.y_pos >= 8 {
            self.y_pos -= 8;
            self.x_pos = self.fb.width - (self.fb.width % 8) - 8; 
        }
        // Limpiamos el rastro físicamente con el framebuffer
        self.fb.fill_rect(self.x_pos, self.y_pos, 8, 8, self.bg_color);
    }

    fn new_line(&mut self) {
        self.x_pos = 0;
        self.y_pos += 8;
        if self.y_pos + 8 >= self.fb.height {
            self.y_pos = 0;
            self.clear_screen(); 
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() { self.write_byte(byte); }
        Ok(())
    }
}