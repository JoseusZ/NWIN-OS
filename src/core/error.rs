// src/error.rs

/// # KernelError — sistema unificado de errores del kernel (Fase 1)
/// Contrato: `KernelError` es el **unico enum de error raiz** del
/// kernel NWIN OS. A partir de las Fase 1.x todas las funciones
/// falibles devolveran `Result<T, KernelError>` o `Result<T,
/// SubError>` cuando un `From<SubError> for KernelError` exista.
///
/// ## Capas cubiertas (ordenadas por dominio):
/// - `Memory(MemoryError)`            — fallos en paginacion / heap / CoW
/// - `Privilege(PrivilegeError)`       — #PF, #GP, #UD, #DE, #DF
/// - `Hardware(HardwareError)`         — fallos de E/S y controladoras
/// - `Syscall(...)`                    — fase 2.5; pendiente
/// - `Fs(FsError)`                     — VFS / FAT / ext4 / MBR
/// - `Task(TaskError)`                 — TaskManager / spawn / context_switch
/// - `Mm(...)`                         — fase 2.5; pendiente
///
/// Regla arquitectonica: NWIN OS no es Linux. Este `KernelError`
/// representa la **semantica interna en Rust**. Solo en la frontera
/// Ring 3 (syscalls) se traduce a `errno` POSIX mediante
/// `KernelError::to_errno()` (Fase 2.5).
///
/// El error maestro del kernel.
/// A partir de ahora, las funciones falibles devolverán Result<T, KernelError>
#[derive(Debug, Clone)]
pub enum KernelError {
    Memory(MemoryError),
    Privilege(PrivilegeError),
    Hardware(HardwareError),
    /// Variante anadida en Fase 1.8 para que el sistema de archivos
    /// pueda propagar errores tipados (reemplazando los `&'static str`
    /// dispersos). Mantiene paridad con `SystemError` durante la
    /// transicion.
    Fs(FsError),
    /// Variante anadida en Fase 1.8. Convive con `SystemError::TaskCreationFailure`
    /// (que sigue vivo en este archivo) hasta que se migre
    /// `src/task/task.rs:69` en fases posteriores.
    Task(TaskError),
    /// Mantiene las dos variantes historicas (`ElfParseFailed`,
    /// `TaskCreationFailure`) hasta que se complete la migracion.
    System(SystemError),
}

/// # MemoryError — Fallos del subsistema de memoria (Fase 1.2)
///
/// Variantes:
/// - `PageFault { addr, flags }` — Codigo crudo del #PF (direccion + PageFaultErrorCode).
///   `flags` refleja `PageFaultErrorCode` bit a bit, conservando el original
///   para que el handler IDT decida si fue ring 0/ring 3, present/write/user,
///   y para resolver CoW sin reinterpretar.
/// - `OutOfFrames` — El asignador fisico (Bitmap) se quedo sin RAM.
/// - `InvalidMapping` — Intento de mapear memoria que ya estaba ocupada o choca con HHDM.
/// - `CoWResolutionFailed` — El handler de #PF intento resolver Copy-on-Write y fallo
///   (p. ej. la pagina no estaba mapeada o ya es writable).
/// - `HhdmMissing` — Se intento usar el Higher-Half Direct Map antes de que Limine
///   lo entregara, o `HHDM_REQUEST.response()` devolvio `None`.
/// - `HeapOOM` — El heap del kernel agoto su espacio (`linked_list_allocator` devolvio None).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    PageFault { addr: usize, flags: u64 },
    OutOfFrames,
    InvalidMapping,
    CoWResolutionFailed,
    HhdmMissing,
    HeapOOM,
}

/// # PrivilegeError — Fallos de CPU por violacion de privilegios (Fase 1.3)
///
/// Variantes:
/// - `GeneralProtectionFault { error_code, is_user }` — `#GP`. El CPU
///   empuja un codigo de error + el CPL actual. `is_user` ya viene
///   derivado porque el IDT handler lo calcula desde `code_segment`.
/// - `InvalidOpcode { is_user }` — `#UD`. La tarea intento ejecutar una
///   instruccion ilegal (p. ej. `hlt` en Ring 3).
/// - `DivideError { is_user }` — `#DE`. Division por cero o similar
///   en coma entera.
/// - `DoubleFault` — `#DF`. Doble falta durante una excepcion previa;
///   en este kernel va siempre a `panic!` inmediato.
/// - `PermissionDenied` — Reservada para fallos futuros de proteccion
///   que no encajan en ninguna de las anteriores (p. ej. acceso a
///   puerto E/S denegado en Ring 3 si activamos IOPL en el futuro).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeError {
    GeneralProtectionFault { error_code: u64, is_user: bool },
    InvalidOpcode { is_user: bool },
    DivideError { is_user: bool },
    DoubleFault,
    PermissionDenied,
}

/// # HardwareError — Fallos de dispositivos fisicos (Fase 1.4)
///
/// Variantes:
/// - `IoFailure` — Una operacion de E/S fallo (lectura/escritura en bus,
///   DMA, PIO). Sin parametros: el codigo de error especifico se loguea
///   por el caller antes de construir este error.
/// - `ControllerMissing` — El dispositivo requerido (SATA, NIC, GPU,
///   etc.) no aparece en el bus PCI o no responde. Distinto de
///   `DeviceNotFound`: aqui el **controlador entero** esta ausente.
/// - `UnsupportedProtocol` — El dispositivo esta presente pero habla
///   una revision del protocolo no soportada por el driver (ej. NVMe
///   con version 2.0 cuando el driver solo maneja 1.4).
/// - `DeviceNotFound` — El bus responde, pero el dispositivo concreto
///   (por clase/ID) no se encontro.
///
/// Regla: las variantes `#DE`, `#UD` y `#MC` migraron en Fase 1.3 a
/// `PrivilegeError`. Esto deja `HardwareError` estrictamente para
/// fallos de controladoras de E/S reales, alineado con la categoria
/// raiz `KernelError::Hardware`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    IoFailure,
    ControllerMissing,
    UnsupportedProtocol,
    DeviceNotFound,
}

// ============================================================================
// FsError — Errores del subsistema de archivos (Fase 1.5)
// ============================================================================
//
// `ElfError` se mantiene declarado en `crate::task::elf::ElfError` (su
// ubicación histórica y correcta para Fase 3: el ELF Loader es una
// pieza del subsistema de tareas). Aqui solo se referencia por nombre
// para envolverlo dentro de `FsError::Elf`, lo que preserva la causa
// raíz sin duplicar la definición del tipo.
use crate::task::elf::ElfError;

/// # FsError — Fallos del subsistema de archivos (Fase 1.5)
///
/// Diseñado para reemplazar todos los `Result<T, &'static str>`
/// dispersos por el modulo `src/fs/*`. Cada variante lleva los datos
/// necesarios para reconstruir el contexto del fallo sin perder
/// informacion.
///
/// ## Variantes de bloques:
/// - `BlockRead` — `BlockDevice::read_block` fallo (controladora,
///   DMA, timeout, etc).
/// - `BlockWrite` — Idem para escritura.
///
/// ## Variantes de geometria:
/// - `BadMagic { expected, found }` — La firma magica del boot sector
///   no coincide. Lleva ambos valores para diagnostico.
/// - `MbrInvalid` — El sector 0 falla la verificacion 0x55 0xAA.
/// - `GptOnly` — El disco tiene un MBR protectivo GPT; el caller debe
///   parsear la cabecera GPT (futuro).
/// - `UnknownPartition(u8)` — La entrada MBR tiene un tipo no
///   reconocido. Lleva el codigo en bruto.
///
/// ## Variantes VFS:
/// - `VfsEntryMissing` — `open_vnode`/`lookup`/`resolve_path` no
///   encontraron el inodo objetivo.
///
/// ## Variantes ext4 / FAT / formato:
/// - `CorruptedDirectory` — Registro de directorio invalido
///   (rec_len=0, desbordamiento, etc).
/// - `NoSpace` — Directorio lleno / sin padding para encoger la entrada.
/// - `OutOfInodes` — Mapa de bits de inodos agotado.
/// - `OutOfBlocks` — Mapa de bits de bloques agotado.
///
/// ## Variantes ELF (envoltura):
/// - `Elf(ElfError)` — El ELF loader fallo; la causa exacta vive en
///   `ElfError` (TooSmall, InvalidMagicNumber, Not64Bit,
///   MemoryMappingFailed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    BlockRead,
    BlockWrite,
    BadMagic { expected: u16, found: u16 },
    MbrInvalid,
    GptOnly,
    UnknownPartition(u8),
    VfsEntryMissing,
    Elf(ElfError),
    OutOfInodes,
    OutOfBlocks,
    CorruptedDirectory,
    NoSpace,
}

// ============================================================================
// TaskError — Errores del subsistema de tareas / scheduler (Fase 1.6)
// ============================================================================
//
// NOTA IMPORTANTE DE CONVIVENCIA:
// En este archivo SIGUE EXISTIENDO `pub enum SystemError` con una
// variante homonima `TaskCreationFailure`. Es el tipo que consume
// `src/task/task.rs:69` hoy. NO se elimina todavia para preservar
// la compilacion (la tarea 1.6 es solo declarativa). En fases
// posteriores se migrara `task.rs` para que en lugar de
// `KernelError::System(SystemError::TaskCreationFailure)`
// devuelva `KernelError::Task(TaskError::TaskCreationFailure)`.
//
// Las dos variantes son homonimas pero viven en enums distintos, por
// lo que rustc las trata como simbolos separados y no hay colision.

/// # TaskError — Fallos del subsistema de tareas / scheduler (Fase 1.6)
///
/// Variantes:
/// - `StackAllocation` — `try_reserve_exact(STACK_SIZE)` fallo: la heap
///   no tiene 64 KiB contiguos para la pila de la tarea. Equivale
///   semánticamente a `-ENOMEM` en la frontera POSIX.
/// - `EntryMissing` — El entry point de la tarea (fn()) es null o no
///   resoluble. Deberia disparar un `panic!` antes de hacer spawn.
/// - `PrivilegeMismatch` — Se intento crear una tarea UserMode con
///   un entry point Ring 0 (o viceversa). La deteccion se hace
///   inspeccionando los selectores de segmento.
/// - `TaskCreationFailure` — Variante agregada que **convivira**
///   durante la fase de migracion con la homonima en `SystemError`.
///   Sera la fuente canonica del fallo cuando `task.rs` migre a
///   devolver `KernelError::Task(...)` en fases posteriores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    StackAllocation,
    EntryMissing,
    PrivilegeMismatch,
    TaskCreationFailure,
}

#[derive(Debug, Clone)]
pub enum SystemError {
    /// Falla al parsear un binario. 
    /// Reutilizamos tu enum existente en elf.rs.
    ElfParseFailed(crate::task::elf::ElfError),
    
    /// Falla al intentar crear una tarea nueva (ej. sin memoria para su pila)
    TaskCreationFailure,
}

// ============================================================================
// `core::fmt::Display` — Mensajes estructurados para usuarios humanos (Fase 1.7)
// ============================================================================
//
// Politica de formato: cada variante produce UNA linea, sin '\n' final.
// Para multilinea usar `Debug`. El prefijo de subsistema (MEMORY:,
// PRIV: HW:, FS:, TASK:, KERNEL:) permite a un lector encontrar el
// origen a primera vista en un serial.log denso.
//
// Importante: no usamos `format!()` (no_std sin heap). Solo `write!`
// con un `core::fmt::Formatter`, que vive en la pila y no asigna.

impl core::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MemoryError::PageFault { addr, flags } => {
                write!(f, "MEMORY:#PF addr=0x{:x} flags=0x{:x}", addr, flags)
            }
            MemoryError::OutOfFrames => write!(f, "MEMORY:OutOfFrames (bitmap exhausted)"),
            MemoryError::InvalidMapping => write!(f, "MEMORY:InvalidMapping (region already used or HHDM conflict)"),
            MemoryError::CoWResolutionFailed => write!(f, "MEMORY:CoWResolutionFailed (cannot resolve Copy-on-Write)"),
            MemoryError::HhdmMissing => write!(f, "MEMORY:HhdmMissing (Limine did not provide higher-half direct map)"),
            MemoryError::HeapOOM => write!(f, "MEMORY:HeapOOM (linked_list_allocator returned None)"),
        }
    }
}

impl core::fmt::Display for PrivilegeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PrivilegeError::GeneralProtectionFault { error_code, is_user } => {
                write!(f, "PRIV:#GP error_code={:#x} ring={}", error_code, if *is_user { 3 } else { 0 })
            }
            PrivilegeError::InvalidOpcode { is_user } => {
                write!(f, "PRIV:#UD ring={}", if *is_user { 3 } else { 0 })
            }
            PrivilegeError::DivideError { is_user } => {
                write!(f, "PRIV:#DE ring={}", if *is_user { 3 } else { 0 })
            }
            PrivilegeError::DoubleFault => write!(f, "PRIV:#DF double-fault (panic-inevitable)"),
            PrivilegeError::PermissionDenied => write!(f, "PRIV:PermissionDenied"),
        }
    }
}

impl core::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HardwareError::IoFailure => write!(f, "HW:IoFailure"),
            HardwareError::ControllerMissing => write!(f, "HW:ControllerMissing (not on PCI bus or unresponsive)"),
            HardwareError::UnsupportedProtocol => write!(f, "HW:UnsupportedProtocol (version mismatch)"),
            HardwareError::DeviceNotFound => write!(f, "HW:DeviceNotFound (class/id mismatch)"),
        }
    }
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FsError::BlockRead => write!(f, "FS:BlockRead (device read_block failed)"),
            FsError::BlockWrite => write!(f, "FS:BlockWrite (device write_block failed)"),
            FsError::BadMagic { expected, found } => {
                write!(f, "FS:BadMagic expected=0x{:04x} found=0x{:04x}", expected, found)
            }
            FsError::MbrInvalid => write!(f, "FS:MbrInvalid (missing 0x55 0xAA boot signature)"),
            FsError::GptOnly => write!(f, "FS:GptOnly (protective MBR; GPT parser not yet implemented)"),
            FsError::UnknownPartition(t) => write!(f, "FS:UnknownPartition type=0x{:02x}", t),
            FsError::VfsEntryMissing => write!(f, "FS:VfsEntryMissing (lookup/open_vnode failed)"),
            FsError::Elf(inner) => write!(f, "FS:Elf({:?})", inner),
            FsError::OutOfInodes => write!(f, "FS:OutOfInodes (bitmap full)"),
            FsError::OutOfBlocks => write!(f, "FS:OutOfBlocks (bitmap full)"),
            FsError::CorruptedDirectory => write!(f, "FS:CorruptedDirectory (rec_len=0 / overflow)"),
            FsError::NoSpace => write!(f, "FS:NoSpace (directory or block full)"),
        }
    }
}

impl core::fmt::Display for TaskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TaskError::StackAllocation => write!(f, "TASK:StackAllocation (64 KiB heap reservation failed)"),
            TaskError::EntryMissing => write!(f, "TASK:EntryMissing (null or unresolved fn())"),
            TaskError::PrivilegeMismatch => write!(f, "TASK:PrivilegeMismatch (ring selector vs entry point incompatible)"),
            TaskError::TaskCreationFailure => write!(f, "TASK:TaskCreationFailure (generic spawn failure)"),
        }
    }
}

impl core::fmt::Display for SystemError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SystemError::ElfParseFailed(inner) => write!(f, "SYSTEM:ElfParseFailed({:?})", inner),
            SystemError::TaskCreationFailure => write!(f, "SYSTEM:TaskCreationFailure (legacy; migrates to TaskError)"),
        }
    }
}

impl core::fmt::Display for KernelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Estructura plana: "KERNEL:<capa>:<resto>". Una sola linea.
        match self {
            KernelError::Memory(e) => write!(f, "KERNEL:{}", e),
            KernelError::Privilege(e) => write!(f, "KERNEL:{}", e),
            KernelError::Hardware(e) => write!(f, "KERNEL:{}", e),
            KernelError::Fs(e) => write!(f, "KERNEL:{}", e),
            KernelError::Task(e) => write!(f, "KERNEL:{}", e),
            KernelError::System(e) => write!(f, "KERNEL:{}", e),
        }
    }
}

// ============================================================================
// `core::error::Error` — Trait estandar de Rust para encadenamiento (Fase 1.8)
// ============================================================================
//
// `core::error::Error::source()` permite recorrer la cadena causal
// desde el wrapper raiz `KernelError` hasta la causa original
// (p. ej. `ElfError` dentro de `FsError::Elf` o `SystemError::ElfParseFailed`).
//
// Reglas:
// - Cada sub-enum implementa `Error` trivialmente (sin `source`)
//   **excepto `FsError` y `SystemError`**, que delegan al `ElfError`
//   interno cuando la variante `Elf(_)`/`ElfParseFailed(_)` esta presente.
// - `KernelError::source()` siempre devuelve `Some(&sub_enum)` para
//   preservar la categoria del fallo. Quien recorra la cadena
//   recibira el sub-error completo como siguiente nodo.

impl core::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        // `MemoryError` no contiene errores anidados: siempre None.
        None
    }
}

impl core::error::Error for PrivilegeError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::error::Error for HardwareError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::error::Error for TaskError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::error::Error for FsError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            // Solo `FsError::Elf(inner)` contiene un error anidado
            // (ElfError). El resto son variantes sin causa.
            FsError::Elf(inner) => Some(inner),
            _ => None,
        }
    }
}

impl core::error::Error for SystemError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            SystemError::ElfParseFailed(inner) => Some(inner),
            SystemError::TaskCreationFailure => None,
        }
    }
}

impl core::error::Error for KernelError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        // Cada variante expone su sub-enum completo como causa
        // inmediata. Quien llame a `.source()` obtendra
        // `Some(&MemoryError)` / `Some(&PrivilegeError)` / etc.
        match self {
            KernelError::Memory(e)    => Some(e),
            KernelError::Privilege(e) => Some(e),
            KernelError::Hardware(e)  => Some(e),
            KernelError::Fs(e)        => Some(e),
            KernelError::Task(e)      => Some(e),
            KernelError::System(e)    => Some(e),
        }
    }
}