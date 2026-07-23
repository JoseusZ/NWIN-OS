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

#[derive(Debug, Clone)]
pub enum MemoryError {
    /// Fallo de página estructurado (El #PF de x86_64)
    PageFault {
        vaddr: u64,
        is_user: bool,
        is_write: bool,
        is_instruction_fetch: bool,
    },
    /// El asignador físico (Bitmap) se quedó sin RAM
    OutOfFrames,
    /// Intento de mapear memoria que ya estaba ocupada o choca con el HHDM
    InvalidMapping,
}

#[derive(Debug, Clone)]
pub enum PrivilegeError {
    /// Violación de protección general (#GP)
    GeneralProtectionFault {
        error_code: u64,
        is_user: bool,
    },
    /// El proceso de Ring 3 llamó a un número de Syscall que no está en la tabla
    InvalidSyscall {
        number: u64,
    },
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