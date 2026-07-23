// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod vfs;
pub mod fd;
pub mod manager;    
pub mod partition;  // Tu nueva carpeta de particiones
pub mod fat;        // Tu nueva carpeta FAT
pub mod ext4;




pub fn init_filesystem() {
    if let Some(modules_response) = crate::MODULES_REQUEST.response() {
        if let Some(module) = modules_response.modules().first() {
            let module_ptr = *module as *const _ as *const u64;
            let tar_ptr = unsafe { *module_ptr.add(1) as *const u8 };
            let tar_size = unsafe { *module_ptr.add(2) as usize };
            
            crate::fs::vfs::init(tar_ptr, tar_size);
            crate::fs::vfs::build_vfs_tree_from_tar();
            
            crate::println!("[OK] VFS inicializado. Arbol de directorios montado ({} bytes).", tar_size);
        } else {
            crate::println!("[ERROR] El modulo TAR no se detecto en memoria.");
        }
    } else {
        crate::println!("[ERROR] Limine ignoro la peticion de modulos.");
    }
}


pub trait BlockDevice: Send + Sync {
    /// Lee un bloque (sector) lógico y lo guarda en el buffer proporcionado.
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), &'static str>;
    
    /// Escribe el contenido del buffer en un bloque (sector) lógico.
    fn write_block(&self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;
}

