#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4DirEntryHeader {
    pub inode: u32,       // Número de inodo al que apunta este archivo
    pub rec_len: u16,     // Longitud total de este registro (para saltar al siguiente)
    pub name_len: u8,     // Longitud real del nombre de texto
    pub file_type: u8,    // 1 = Archivo Regular, 2 = Directorio
}

// Nota: El texto del nombre sigue inmediatamente después de estos 8 bytes en la memoria.