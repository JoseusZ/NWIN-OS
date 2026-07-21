use core::str;
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::collections::BTreeMap;
use alloc::string::String;

// ====================================================================
// ESTRUCTURAS DE SEGURIDAD PARA CONCURRENCIA
// ====================================================================

// --- Envoltorio para hacer el puntero crudo seguro entre hilos ---
// Como los punteros crudos (*const u8) no implementan Send ni Sync por defecto,
// el compilador nos prohíbe usarlos en variables globales (lazy_static).
// Al crear esta estructura y prometerle a Rust que nosotros controlaremos
// el acceso mediante un Mutex, evitamos el error E0277.

pub trait VNode: Send + Sync {
    // --- FILE OPERATIONS ---
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
    fn get_size(&self) -> usize { 0 }

    // --- DIRECTORY OPERATIONS ---
    fn is_dir(&self) -> bool { false }
    
    /// Searches for a child node by name within this directory.
    fn lookup(&self, _name: &str) -> Option<alloc::sync::Arc<dyn VNode>> { None }
}


#[derive(Clone, Copy)]
pub struct TarContext {
    pub ptr: *const u8,
    pub size: usize,
}

unsafe impl Send for TarContext {}
unsafe impl Sync for TarContext {}

// ====================================================================
// BÓVEDA GLOBAL DEL SISTEMA DE ARCHIVOS VIRTUAL (VFS)
// ====================================================================

lazy_static! {
    pub static ref INITRAMFS: Mutex<Option<TarContext>> = Mutex::new(None);
    pub static ref MOUNT_TABLE: MountTable = MountTable::new();
    pub static ref VFS_ROOT: alloc::sync::Arc<DirVNode> = alloc::sync::Arc::new(DirVNode::new());
}

/// Guarda la dirección del Initramfs durante el arranque del sistema.
/// Debe llamarse una sola vez desde `main.rs`.
pub fn init(tar_address: *const u8, tar_size: usize) {
    *INITRAMFS.lock() = Some(TarContext {
        ptr: tar_address,
        size: tar_size,
    });
}

// ====================================================================
// LÓGICA DE BÚSQUEDA Y LECTURA
// ====================================================================

/// Busca un archivo por su nombre dentro del archivo TAR global cargado en memoria.
/// Retorna un *slice* (porción de memoria) con el contenido exacto del archivo.
pub fn find_file(filename: &str) -> Option<&'static [u8]> {
    let initramfs = INITRAMFS.lock();
    
    // Si el disco RAM ya fue inicializado, procedemos a buscar
    if let Some(tar_ctx) = *initramfs {
        let mut offset = 0;

        while offset + 512 <= tar_ctx.size {
            // La cabecera TAR es de 512 bytes
            let header_ptr = unsafe { tar_ctx.ptr.add(offset) };
            let header = unsafe { core::slice::from_raw_parts(header_ptr, 512) };

            // Si el primer byte es 0, llegamos al final del archivo TAR
            if header[0] == 0 {
                break;
            }

            // 1. Extraer el nombre del archivo (Los primeros 100 bytes)
            // Buscamos dónde termina el texto (el primer byte nulo '\0')
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let current_filename = core::str::from_utf8(&header[0..name_len]).unwrap_or("");

            // 2. Extraer el tamaño del archivo (Bytes 124 a 135, en formato Octal ASCII)
            let size_bytes = &header[124..135]; // Leemos 11 bytes (el 12º es el nulo)
            let size_str = core::str::from_utf8(size_bytes).unwrap_or("0").trim();
            let file_size = usize::from_str_radix(size_str, 8).unwrap_or(0);

            // 3. Calcular dónde empiezan los datos reales
            let data_start = offset + 512;

            // ¿Es este el archivo que buscamos?
            if current_filename == filename {
                let data_ptr = unsafe { tar_ctx.ptr.add(data_start) };
                let data_slice = unsafe { core::slice::from_raw_parts(data_ptr, file_size) };
                return Some(data_slice);
            }

            // 4. Saltar al siguiente archivo
            // Los datos siempre ocupan bloques completos de 512 bytes, aunque sobren bytes.
            let data_blocks = (file_size + 511) / 512; 
            offset += 512 + (data_blocks * 512);
        }
    }

    None // Archivo no encontrado o VFS no inicializado
}

pub struct TarVNode {
    data: &'static [u8],
}

impl TarVNode {
    pub fn new(data: &'static [u8]) -> Self {
        Self { data }
    }
}

impl VNode for TarVNode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        // Prevent reading out of bounds
        if offset >= self.data.len() {
            return 0;
        }
        
        let available = self.data.len() - offset;
        let bytes_to_copy = core::cmp::min(buf.len(), available);
        
        buf[..bytes_to_copy].copy_from_slice(&self.data[offset..offset + bytes_to_copy]);
        bytes_to_copy
    }

    fn get_size(&self) -> usize {
        self.data.len()
    }
}

/// Opens a file and returns it as a standardized Virtual Node.
/// This abstracts the RAM implementation away from the Syscall handler.
/// Abre un archivo utilizando el motor de resolución de rutas POSIX.
pub fn open_vnode(path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
    resolve_path(path)
}
pub struct DirVNode {
    children: Mutex<BTreeMap<String, alloc::sync::Arc<dyn VNode>>>,
}

impl DirVNode {
    pub fn new() -> Self {
        Self {
            children: Mutex::new(BTreeMap::new()),
        }
    }

    /// Mounts or adds a new child node to this directory.
    pub fn add(&self, name: &str, node: alloc::sync::Arc<dyn VNode>) {
        self.children.lock().insert(String::from(name), node);
    }
}

impl VNode for DirVNode {
    fn is_dir(&self) -> bool { true }
    
    fn lookup(&self, name: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
        self.children.lock().get(name).cloned()
    }
}


// ====================================================================
// PATH RESOLUTION ENGINE (NIVEL POSIX)
// ====================================================================

pub fn resolve_path(path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
    // 1. Inicia desde la raíz. 
    let mut current: alloc::sync::Arc<dyn VNode> = MOUNT_TABLE.get_mount("/")
        .unwrap_or_else(|| VFS_ROOT.clone());

    let parts = path.split('/').filter(|p| !p.is_empty());
    let mut current_path = alloc::string::String::from("");

    for part in parts {
        if !current.is_dir() {
            return None; // Intentamos entrar a un archivo como si fuera carpeta
        }
        
        // Reconstruimos la ruta paso a paso para revisar puntos de montaje
        current_path.push('/');
        current_path.push_str(part);

        // 2. INTERCEPCIÓN POSIX: ¿Hay un disco anclado en esta carpeta exacta?
        if let Some(mounted_node) = MOUNT_TABLE.get_mount(&current_path) {
            current = mounted_node; // Cruzamos la frontera al nuevo disco
            continue;
        }

        // 3. Si no hay disco anclado, buscamos el archivo/carpeta de forma normal
        match current.lookup(part) {
            Some(next_node) => current = next_node,
            None => return None, // El archivo/carpeta no existe
        }
    }

    Some(current)
}

// ====================================================================
// TABLA DE PUNTOS DE MONTAJE (MOUNT TABLE)
// ====================================================================

pub struct MountTable {
    mounts: spin::Mutex<alloc::collections::BTreeMap<alloc::string::String, alloc::sync::Arc<dyn VNode>>>,
}

impl MountTable {
    pub fn new() -> Self {
        Self {
            mounts: spin::Mutex::new(alloc::collections::BTreeMap::new()),
        }
    }

    /// Ancla un VNode (Sistema de archivos entero) a una ruta específica.
    pub fn mount(&self, path: &str, node: alloc::sync::Arc<dyn VNode>) {
        self.mounts.lock().insert(alloc::string::String::from(path), node);
    }

    /// Busca si existe un sistema de archivos montado en esta ruta exacta.
    pub fn get_mount(&self, path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
        self.mounts.lock().get(path).cloned()
    }

    /// Desmonta un VNode, liberando el punto de anclaje. VITAL para pivot_root.
    pub fn unmount(&self, path: &str) -> bool {
        self.mounts.lock().remove(path).is_some()
    }
}

/// Escanea el archivo TAR en RAM y construye el árbol de directorios inicial en VFS_ROOT.
pub fn build_vfs_tree_from_tar() {
    let initramfs = INITRAMFS.lock();
    
    if let Some(tar_ctx) = *initramfs {
        let mut offset = 0;
        
        while offset + 512 <= tar_ctx.size {
            let header_ptr = unsafe { tar_ctx.ptr.add(offset) };
            let header = unsafe { core::slice::from_raw_parts(header_ptr, 512) };
            
            // Fin del archivo TAR
            if header[0] == 0 { break; }

            // Extraer nombre
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let current_filename = core::str::from_utf8(&header[0..name_len]).unwrap_or("");

            // Extraer tamaño
            let size_bytes = &header[124..135];
            let size_str = core::str::from_utf8(size_bytes).unwrap_or("0").trim();
            let file_size = usize::from_str_radix(size_str, 8).unwrap_or(0);

            let data_start = offset + 512;
            
            // Si es un archivo válido, lo encapsulamos en un VNode y lo anclamos a la raíz
            if file_size > 0 && !current_filename.is_empty() {
                let data_ptr = unsafe { tar_ctx.ptr.add(data_start) };
                let data_slice = unsafe { core::slice::from_raw_parts(data_ptr, file_size) };
                
                let vnode = alloc::sync::Arc::new(TarVNode::new(data_slice));
                
                // Limpiamos barras iniciales por si el TAR las incluye (ej. "/shell.elf" -> "shell.elf")
                let clean_name = current_filename.trim_start_matches('/');
                VFS_ROOT.add(clean_name, vnode);
            }

            // Saltar al siguiente bloque
            let data_blocks = (file_size + 511) / 512; 
            offset += 512 + (data_blocks * 512);
        }
    }
}