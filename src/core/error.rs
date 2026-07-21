// src/error.rs

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