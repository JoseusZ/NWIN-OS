#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4Inode {
    pub i_mode: u16,        // Permisos y tipo (0x4000 = Directorio, 0x8000 = Archivo regular)
    pub i_uid: u16,
    pub i_size_lo: u32,     // Tamaño del archivo (parte baja)
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks_lo: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u8; 60],  // ¡Aquí vive el árbol de Extents! (Apunta a los datos reales)
    pub i_generation: u32,
    pub i_file_acl_lo: u32,
    pub i_size_hi: u32,     // Tamaño del archivo (parte alta, para archivos > 4GB)
    pub i_obso_faddr: u32,
    // (Omitimos el resto de osd2 y extra_isize por ahora para simplificar el MVP)
}

impl Ext4Inode {
    pub fn is_directory(&self) -> bool {
        let mode = { self.i_mode };
        (mode & 0xF000) == 0x4000
    }

    pub fn size(&self) -> u64 {
        let lo = { self.i_size_lo } as u64;
        let hi = { self.i_size_hi } as u64;
        (hi << 32) | lo
    }
}