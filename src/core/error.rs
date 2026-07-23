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

#[derive(Debug, Clone)]
pub enum HardwareError {
    DivideByZero { is_user: bool },
    InvalidOpcode { is_user: bool },
    MachineCheck,
}

#[derive(Debug, Clone)]
pub enum SystemError {
    /// Falla al parsear un binario. 
    /// Reutilizamos tu enum existente en elf.rs.
    ElfParseFailed(crate::task::elf::ElfError),
    
    /// Falla al intentar crear una tarea nueva (ej. sin memoria para su pila)
    TaskCreationFailure,
}