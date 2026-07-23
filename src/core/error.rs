// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed error tree for the kernel.
//!
//! `KernelError` is the single root enum returned by every fallible
//! kernel function. Each variant wraps a sub-enum that owns the
//! payload:
//!
//! At the Ring 3 boundary the typed value is reduced to a POSIX `errno`
//! via [`KernelError::to_errno`]. `Display` produces a single-line
//! diagnostic with the `KERNEL:<layer>:` prefix.

/// Root error type returned by every fallible kernel function.
#[derive(Debug, Clone)]
pub enum KernelError {
    Memory(MemoryError),
    Privilege(PrivilegeError),
    Hardware(HardwareError),
    Fs(FsError),
    Task(TaskError),
    System(SystemError),
}

/// Memory subsystem failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    /// Raw `#PF` payload: faulting address plus the `PageFaultErrorCode`
    /// bits captured verbatim so downstream consumers can inspect the
    /// present / write / user / instruction-fetch flags without
    /// re-decoding.
    PageFault { addr: usize, flags: u64 },
    /// The bitmap frame allocator is exhausted.
    OutOfFrames,
    /// Attempt to map a region already occupied or conflicting with HHDM.
    InvalidMapping,
    /// The `#PF` handler could not resolve a Copy-on-Write fault.
    CoWResolutionFailed,
    /// HHDM was requested before Limine provisioned it
    /// (`HHDM_REQUEST.response()` was `None`).
    HhdmMissing,
    /// Kernel heap (`linked_list_allocator`) returned `None`.
    HeapOOM,
}

/// CPU privilege violations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeError {
    /// `#GP`. `error_code` mirrors the value pushed by the CPU;
    /// `is_user` is pre-decoded from the code-segment RPL by the
    /// IDT handler.
    GeneralProtectionFault { error_code: u64, is_user: bool },
    /// `#UD`. The task executed an instruction reserved for the
    /// current privilege ring (e.g. `hlt` in Ring 3).
    InvalidOpcode { is_user: bool },
    /// `#DE`. Integer divide-by-zero or friends.
    DivideError { is_user: bool },
    /// `#DF`. Always escalates to `panic!`.
    DoubleFault,
    /// Reserved for future protection failures (e.g. IOPL-denied
    /// I/O port access from Ring 3).
    PermissionDenied,
}

/// Physical device and bus controller failures.
///
/// Note: `#DE`, `#UD`, `#MC` migrated to [`PrivilegeError`]. This
/// enum now exclusively covers I/O controller faults, matching
/// `KernelError::Hardware`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    /// Generic I/O fault (DMA, PIO, controller reply).
    IoFailure,
    /// The whole controller (HBA, NIC, GPU) is missing from the bus.
    ControllerMissing,
    /// Protocol revision not supported by this driver.
    UnsupportedProtocol,
    /// Bus responds but the requested device was not found.
    DeviceNotFound,
}

use crate::task::elf::ElfError;

/// File-system failures. Each variant carries enough context to
/// diagnose without losing information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    // Block device
    BlockRead,
    BlockWrite,
    // Geometry
    /// Boot-sector magic mismatch; both values are kept for debugging.
    BadMagic { expected: u16, found: u16 },
    /// Sector 0 fails the `0x55 0xAA` boot-signature check.
    MbrInvalid,
    /// Disk has a protective GPT MBR; caller should parse GPT.
    GptOnly,
    /// MBR entry with an unknown type. Holds the raw type byte.
    UnknownPartition(u8),
    // VFS
    /// `open_vnode`/`lookup`/`resolve_path` did not find the target inode.
    VfsEntryMissing,
    // ext4 / FAT / format
    /// Directory entry malformed (`rec_len == 0`, overflow, etc.).
    CorruptedDirectory,
    /// Directory / block full; no padding to shrink.
    NoSpace,
    /// Inode bitmap exhausted.
    OutOfInodes,
    /// Block bitmap exhausted.
    OutOfBlocks,
    /// ELF loader failed; underlying cause lives in [`ElfError`].
    Elf(ElfError),
}

/// Scheduler / task subsystem failures.
///
/// The `TaskCreationFailure` variant here is the canonical source of
/// truth for spawn failures. It co-exists with the homonymous variant
/// in [`SystemError`] for the duration of the migration period; the
/// two names refer to independent types in rustc's eyes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    /// `try_reserve_exact(STACK_SIZE)` failed: no 64 KiB contiguous
    /// heap region for the new task stack. Maps to `-ENOMEM`.
    StackAllocation,
    /// Entry point `fn()` is `None` / unresolvable. Should panic
    /// before reaching `spawn`.
    EntryMissing,
    /// UserMode task spawned with a Ring 0 entry point (or vice
    /// versa), detected via the segment selectors.
    PrivilegeMismatch,
    /// Generic spawn failure.
    TaskCreationFailure,
}

/// Legacy bridge variants retained while call sites migrate to the
/// typed tree.
#[derive(Debug, Clone)]
pub enum SystemError {
    /// ELF parse failure. Equivalent to `FsError::Elf` once the
    /// caller-side migration completes.
    ElfParseFailed(crate::task::elf::ElfError),
    /// Legacy `TaskCreationFailure` sink.
    TaskCreationFailure,
}

// `core::fmt::Display` implementations.
//
// Format policy: one line per variant, no trailing newline. The
// `<SUBSYSTEM>:` prefix lets an operator locate the origin in a dense
// serial log without scanning. We never use `format!` (no heap) —
// only stack-local `write!` calls.

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
        // Flat structure: `KERNEL:<layer>:<rest>`, single line.
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

// `core::error::Error` impls. Every sub-enum impls `Error` with a
// trivial `source()` that returns `None`, except `FsError` /
// `SystemError`, which surface the underlying [`ElfError`]. The
// root [`KernelError`] always chains to its sub-enum so the cause
// preserves its category.

impl core::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
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

// Bridge from the driver-layer `DriverError` to the FS-layer
// `FsError`. With this `From` impl, the `?` operator applied to any
// `BlockDevice::read_block` / `write_block` call transparently
// produces the FS-layer variant that matches the failure mode.
impl From<crate::drivers::block::DriverError> for FsError {
    fn from(e: crate::drivers::block::DriverError) -> Self {
        use crate::drivers::block::DriverError as DE;
        match e {
            DE::IoFailure           => FsError::BlockRead,
            DE::Timeout             => FsError::BlockRead,
            DE::BufferTooSmall      => FsError::CorruptedDirectory,
            DE::BufferTooLarge      => FsError::CorruptedDirectory,
            DE::NoDmaMemory         => FsError::OutOfBlocks,
            DE::ControllerMissing   => FsError::VfsEntryMissing,
            DE::DeviceNotFound      => FsError::VfsEntryMissing,
            DE::UnsupportedProtocol => FsError::VfsEntryMissing,
        }
    }
}

// `From<...>` impls. They let every `?` operator lift a sub-error
// into `KernelError`. `ElfError` lands in `FsError::Elf` because the
// ELF loader reads bytes from the VFS, so its failure is
// semantically filesystem-shaped.

impl From<MemoryError> for KernelError {
    fn from(e: MemoryError) -> Self {
        KernelError::Memory(e)
    }
}

impl From<PrivilegeError> for KernelError {
    fn from(e: PrivilegeError) -> Self {
        KernelError::Privilege(e)
    }
}

impl From<HardwareError> for KernelError {
    fn from(e: HardwareError) -> Self {
        KernelError::Hardware(e)
    }
}

impl From<FsError> for KernelError {
    fn from(e: FsError) -> Self {
        KernelError::Fs(e)
    }
}

impl From<TaskError> for KernelError {
    fn from(e: TaskError) -> Self {
        KernelError::Task(e)
    }
}

impl From<SystemError> for KernelError {
    fn from(e: SystemError) -> Self {
        KernelError::System(e)
    }
}

impl From<ElfError> for KernelError {
    fn from(e: ElfError) -> Self {
        KernelError::Fs(FsError::Elf(e))
    }
}

// Compatibility bridge that lets legacy `Err("...")` call sites keep
// compiling while they migrate to typed errors. The string itself is
// intentionally discarded for now and reseeded into the generic
// `SystemError::TaskCreationFailure` sink; once every call site has
// been migrated, this impl is removed.
impl From<&'static str> for KernelError {
    fn from(_msg: &'static str) -> Self {
        KernelError::System(SystemError::TaskCreationFailure)
    }
}

// POSIX `errno` codes emitted at the Ring 3 boundary. The enum
// stores positive values; `to_neg_i32` returns the negative form
// required by the `syscall/retq` ABI.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Errno {
    /// 1 — Operation not permitted.
    EPERM  = 1,
    /// 2 — No such file or directory.
    ENOENT = 2,
    /// 5 — Hardware I/O error.
    EIO    = 5,
    /// 6 — No such device or address; used for missing controllers
    /// and missing LUNs.
    ENXIO  = 6,
    /// 8 — Invalid executable format; returned by the ELF loader.
    ENOEXEC = 8,
    /// 9 — Bad file descriptor.
    EBADF  = 9,
    /// 12 — Out of memory.
    ENOMEM = 12,
    /// 13 — Permission denied.
    EACCES = 13,
    /// 14 — Bad address (invalid memory reference).
    EFAULT = 14,
    /// 17 — File already exists.
    EEXIST = 17,
    /// 22 — Invalid argument.
    EINVAL = 22,
    /// 28 — No space left on device.
    ENOSPC = 28,
    /// 38 — Function not implemented.
    ENOSYS = 38,
}

impl Errno {
    /// Returns the negative form of the errno, ready to be written
    /// into `rax` before `sysretq`.
    #[inline]
    pub fn to_neg_i32(self) -> i32 {
        // Safe: every variant has a non-zero discriminant that fits
        // in i32 and is negated without overflow.
        -(self as i32)
    }
}

// `to_errno()` mapping. Each sub-enum implements its own, routed
// here by the root enum. The returned value is negative so the
// syscall handler can do `regs.rax = e.to_errno() as u64`.
fn mt(inner: Errno) -> i32 {
    inner.to_neg_i32()
}

impl MemoryError {
    pub fn to_errno(&self) -> i32 {
        match self {
            // Linux returns -EFAULT when the syscall argument is an
            // invalid address (the corresponding SIGSEGV signal is
            // delivered separately by the kernel).
            MemoryError::PageFault { .. }                  => mt(Errno::EFAULT),
            MemoryError::OutOfFrames                      => mt(Errno::ENOMEM),
            MemoryError::HeapOOM                          => mt(Errno::ENOMEM),
            MemoryError::InvalidMapping                   => mt(Errno::EFAULT),
            MemoryError::CoWResolutionFailed              => mt(Errno::EFAULT),
            MemoryError::HhdmMissing                      => mt(Errno::EFAULT),
        }
    }
}

impl PrivilegeError {
    pub fn to_errno(&self) -> i32 {
        match self {
            // The IDT handler terminates the offending task before
            // reaching this path for #GP/#UD/#DE/#DF, so the mapping
            // here is defensive for future paths that may surface
            // these errors through the syscall ABI.
            PrivilegeError::GeneralProtectionFault { .. } => mt(Errno::EFAULT),
            PrivilegeError::InvalidOpcode { is_user }     => {
                if *is_user { mt(Errno::ENOSYS) } else { mt(Errno::EFAULT) }
            }
            PrivilegeError::DivideError { .. }             => mt(Errno::EFAULT),
            PrivilegeError::DoubleFault                   => mt(Errno::EFAULT),
            PrivilegeError::PermissionDenied               => mt(Errno::EACCES),
        }
    }
}

impl HardwareError {
    pub fn to_errno(&self) -> i32 {
        match self {
            HardwareError::IoFailure              => mt(Errno::EIO),
            HardwareError::ControllerMissing      => mt(Errno::ENXIO),
            HardwareError::UnsupportedProtocol    => mt(Errno::ENOSYS),
            HardwareError::DeviceNotFound         => mt(Errno::ENXIO),
        }
    }
}

impl FsError {
    pub fn to_errno(&self) -> i32 {
        match self {
            FsError::BlockRead                       => mt(Errno::EIO),
            FsError::BlockWrite                      => mt(Errno::EIO),
            FsError::BadMagic { .. }                 => mt(Errno::EINVAL),
            FsError::MbrInvalid                      => mt(Errno::EINVAL),
            FsError::GptOnly                         => mt(Errno::EINVAL),
            FsError::UnknownPartition(_)             => mt(Errno::EINVAL),
            FsError::VfsEntryMissing                 => mt(Errno::ENOENT),
            FsError::OutOfInodes                     => mt(Errno::ENOSPC),
            FsError::OutOfBlocks                     => mt(Errno::ENOSPC),
            FsError::CorruptedDirectory              => mt(Errno::EIO),
            FsError::NoSpace                         => mt(Errno::ENOSPC),
            FsError::Elf(inner)                     => inner.to_errno(),
        }
    }
}

impl TaskError {
    pub fn to_errno(&self) -> i32 {
        match self {
            TaskError::StackAllocation      => mt(Errno::ENOMEM),
            TaskError::EntryMissing         => mt(Errno::EINVAL),
            TaskError::PrivilegeMismatch    => mt(Errno::EACCES),
            TaskError::TaskCreationFailure  => mt(Errno::EINVAL),
        }
    }
}

impl SystemError {
    pub fn to_errno(&self) -> i32 {
        match self {
            SystemError::ElfParseFailed(inner) => inner.to_errno(),
            SystemError::TaskCreationFailure   => mt(Errno::EINVAL),
        }
    }
}

impl ElfError {
    /// Maps each variant to the POSIX `errno` value returned when an
    /// `execve` syscall fails because of an ELF loading problem.
    ///
    /// - `InvalidMagicNumber`, `Not64Bit` → `-ENOEXEC` (8).
    /// - `MemoryMappingFailed` → `-ENOMEM` (12).
    /// - `TooSmall` → `-EINVAL` (22).
    pub fn to_errno(&self) -> i32 {
        mt(match self {
            ElfError::TooSmall            => Errno::EINVAL,
            ElfError::InvalidMagicNumber  => Errno::ENOEXEC,
            ElfError::Not64Bit            => Errno::ENOEXEC,
            ElfError::MemoryMappingFailed => Errno::ENOMEM,
        })
    }
}

impl KernelError {
    /// Public entry point for the syscall ABI. Reduces any
    /// `KernelError` to the negative POSIX `errno` value that user
    /// space will read from `rax` after `sysretq`.
    ///
    /// Canonical call site in `handle_syscall_rust`:
    /// ```ignore
    /// regs.rax = kernel_error.to_errno() as u64;
    /// ```
    pub fn to_errno(&self) -> i32 {
        match self {
            KernelError::Memory(e)    => e.to_errno(),
            KernelError::Privilege(e) => e.to_errno(),
            KernelError::Hardware(e)  => e.to_errno(),
            KernelError::Fs(e)        => e.to_errno(),
            KernelError::Task(e)      => e.to_errno(),
            KernelError::System(e)    => e.to_errno(),
        }
    }
}

/// Logs a [`KernelError`] to the serial console with the
/// `[KERNEL ERROR LOG]` prefix.
///
/// Panic-free, allocation-free, and re-entrant, making it safe to
/// invoke from exception handlers or deep kernel routines.
pub fn log_kernel_error(e: &KernelError) {
    crate::serial_println!("[KERNEL ERROR LOG] {}", e);
}