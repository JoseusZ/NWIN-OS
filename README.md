# NWIN OS

> **NWIN is Neither Windows Interface nor Native Linux**

A monolithic x86_64 operating system kernel written in **Rust**, built from scratch
without relying on any existing OS codebase. It boots via the **Limine** bootloader,
isolates user processes in Ring 3 with per-task address spaces, and provides a
POSIX-leaning system call ABI so Linux-compatible tooling (ELF binaries, syscalls)
runs natively.

This repository contains the kernel, userspace samples, build scripts, and test
artifacts. It is a personal research / portfolio project with the explicit goal of
becoming a fully featured hobby OS.

---

## Table of Contents

- [Project Goal](#project-goal)
- [Current Capabilities](#current-capabilities)
- [Architecture Overview](#architecture-overview)
- [Roadmap / Future Work](#roadmap--future-work)
- [Repository Layout](#repository-layout)
- [Building and Running](#building-and-running)
- [Testing](#testing)
- [Contributing](#contributing)
  - [Reporting Issues](#reporting-issues)
  - [Sending Patches](#sending-patches)
  - [Coding Style](#coding-style)
  - [License and Sign-off](#license-and-sign-off)
- [Maintainer](#maintainer)

---

## Project Goal

NWIN OS is an exercise in building a modern Unix-like operating system from the
ground up, in a memory-safe systems language. The project is shaped by three
guiding principles:

1. **POSIX familiarity.** System calls follow the Linux x86-64 ABI (`syscall`
   via `sysretq`) so that existing toolchains (musl, glibc) and prebuilt
   binaries can be adapted with minimal effort.
2. **Modern boot stack.** The kernel uses the **Limine** bootloader (Stivale2
   protocol), supports a UEFI boot path, and requests the higher-half direct
   map (HHDM) plus a Limine module for the initramfs TAR.
3. **Rust everywhere.** No C in the kernel proper. `#![no_std]` with
   `build-std`, no global allocator leaks, and unsafe blocks are isolated to
   well-justified hardware interfaces.

The acronym **NWIN** stands for **N**either **W**indows **I**nterface nor
**N**ative **L**inux - it is neither of them, it is its own thing.

---

## Current Capabilities

Implemented subsystems and what they actually do today:

### Boot and Platform
- Boots via **Limine** (Stivale2 requests: `Framebuffer`, `Memmap`, `HHDM`,
  `Modules`, `BaseRevision`).
- Custom Rust target spec (`x86_64-kernel.json`) with `build-std` for
  `core`, `alloc`, `compiler_builtins`.
- Static PML4 paging with the higher-half direct map.
- Initramfs delivered as a Limine module, parsed as a TAR stream.

### CPU and Architecture
- SSE/SSE2 enabled via `CR0`/`CR4`.
- GDT with **dual Ring 0 / Ring 3 selectors** and a TSS that exposes a
  privilege stack (RSP0) and an IST stack for double faults.
- Local APIC disabled (legacy PIC routing active).

### Interrupts and System Calls
- Full **IDT** with `extern "x86-interrupt"` handlers for: breakpoint,
  page fault, GPF, divide error, invalid opcode, double fault (IST),
  and the chained PIC IRQs (timer, keyboard, spurious).
- PIT driven at **100 Hz**, used as the scheduler tick.
- **System calls** configured via `STAR`/`LStar`/`SFMask` MSRs.
- Naked-assembly trampoline that enters kernel context, validates
  user pointers against `is_valid_user_memory()`, and returns via
  `sysretq`.
- Zero-trust validation: every user pointer is checked against the
  caller's PML4 before dereference.

### Memory Management
- Bitmap frame allocator with **reference counts per frame** for
  Copy-on-Write.
- OffsetPageTable built on the HHDM.
- 1 MiB kernel heap at `0x4444_4444_0000`, **interrupt-safe** via
  `interrupts::without_interrupts`.
- `allocate_contiguous_frames()` for DMA regions.
- Per-process address spaces with **per-task CR3 switching**.
- `destroy_user_address_space()` for clean teardown of terminated
  tasks.

### Multitasking
- Round-robin preemptive scheduler (PIT-driven).
- Task states: `Ready`, `Running`, `Blocked`, `Dead` (Linux-style).
- Privilege levels: `KernelMode` (Ring 0) and `UserMode` (Ring 3).
- 64 KiB kernel stack per task, dedicated per-task user stack.
- `iretq` trampoline that synthesises an interrupt frame for the
  initial jump into Ring 3.
- Heap base, `program_break`, and `mmap_base` tracked per process.
- Per-process **file descriptor table** (stdin/stdout/stderr preallocated).
- "Reaper" daemon that purges `Dead` tasks and frees their address
  spaces.
- `Ctrl+C` from the keyboard marks the foreground task `Dead`.

### ELF Loader
- ELF64 parser (magic, type, ph_offset, ph_count).
- Segments mapped into a **remote PML4** without touching CR3 of the
  kernel; data copied page-by-page via HHDM.
- Returns the entry point to the scheduler.

### Drivers
- **Serial 8250 UART** (COM1) at 38400 baud 8N1, with `serial_println!`
  macro for low-level telemetry.
- **PIT** timer at 100 Hz.
- **PCI** bus scanner with vendor/device-class identification.
- **AHCI SATA** driver with DMA, command lists, FIS, and LBA-48
  read/write.
- **Framebuffer** (Limine-provided) with pixel-level drawing, rectangle
  fills, clear, and 8x8 font rendering.
- **TTY** with a basic **ANSI CSI** parser (clear screen, colour,
  cursor reset).
- **PS/2 Keyboard** with Scancode Set 1 decoding, US-104 layout, and
  a 256-slot **lock-free ring buffer** for key events.

### Filesystems
- **VFS** with `trait VNode` (file and directory operations).
- `MountTable` for named mounts.
- **MBR partition** parser with type detection (FAT16/32, Linux, GPT
  protective).
- **FAT16 / FAT32** reader with `list_root_dir()` and `lookup()`.
- **Ext4** reader and writer (superblock, BGD, inode allocation,
  block allocation, extent tree, dir-entry insertion, file creation
  with POSIX round-trip test).
- **TAR initramfs** reader for finding embedded files by name.
- `BlockDevice` trait with synchronous `read_block` / `write_block`.

### Error Handling
- Hierarchical `KernelError` enum: `MemoryError`, `PrivilegeError`,
  `HardwareError`, `SystemError`. Functions return
  `Result<T, KernelError>` instead of panicking on recoverable
  failures.

---

## Architecture Overview

```
+-----------------------------------------------------------+
|             Userspace (Ring 3) - ELF binaries             |
|        shell.elf, test_mem.elf, future programs           |
+----------------------------+------------------------------+
                             | iretq / sysretq
+----------------------------v------------------------------+
|                     Kernel (Ring 0)                      |
+-----------------------------------------------------------+
|  Scheduler  |  Task Manager  |  Syscalls  |  VFS         |
|  ELF loader |  FD table      |  Reaper    |  Ext4 + FAT  |
+-----------------------------------------------------------+
|  Paging + Bitmap Allocator + Heap + CoW refcounts        |
+-----------------------------------------------------------+
|  GDT/TSS  IDT/PIC  PIT  PCI  AHCI  TTY  Keyboard  Serial |
+-----------------------------------------------------------+
                             |
+----------------------------v------------------------------+
|             Limine Bootloader (UEFI / Stivale2)          |
+-----------------------------------------------------------+
```

---

## Roadmap / Future Work

The project is being developed against a staged roadmap. Every numbered
item below maps to a tracked milestone in the maintainer's plan.

### Block 1 - Almost Done (>= 80%)

- **1.1 Native `fork` syscall** (57). Clones the parent PML4, marks
  pages CoW, returns 0 to the child and the child PID to the parent.
- **1.2 Robust `free_block` / `free_inode`** in Ext4, including
  multi-group bookkeeping and `s_free_blocks_count_lo` refresh.
- **1.3 Minimal `PT_DYNAMIC` support** in the ELF loader.
- **1.4 `futex`** (syscall 202) on top of the existing semaphore
  queue, required for musl/glibc POSIX threads.

### Block 2 - In Progress (30 - 70%)

- **2.1 Scheduler priorities and affinity** (Linux nice, 0-139).
- **2.2 SMP** - re-enable Local APIC, parse ACPI MADT, trampoline
  for application processors, per-CPU state.
- **2.3 ACPI parser** - RSDP, RSDT/XSDT, MADT, MCFG, HPET.
- **2.4 GPT partition parser** alongside the existing MBR path.
- **2.5 TTY completeness** - save/restore cursor, scroll regions,
  double buffering, 24-bit colour.
- **2.6 NVMe driver** - Admin queues, Identify, I/O queues with PRP.
- **2.7 FAT write support** and LFN (Long File Names).
- **2.8 POSIX signals** - `rt_sigaction`, `kill`, `rt_sigreturn`.
- **2.9 Pipes and `dup`/`dup2`/`fcntl`** for shell plumbing.
- **2.10 HPET timer** as the primary clock, with PIT fallback.

### Block 3 - To Start (0%)

- **3.1 LinuxKPI shim** under `compat/lkpi/` compiled via
  `build.rs` + the `cc` crate.
- **3.2 Formal IPC** - dedicated syscall range for message passing.
- **3.3 LKL server** - Ring 3 host that runs Linux-driver blobs.
- **3.4 Networking** - `e1000` driver, IPv4, UDP, TCP, BSD sockets.
- **3.5 GPU/DRM/KMS** - VGA text mode, Bochs stdvga, Intel i915.
- **3.6 Full dynamic ELF** + `ld-linux` and relocations
  (`R_X86_64_RELATIVE`, `R_X86_64_64`, `R_X86_64_JUMP_SLOT`, ...).
- **3.7 Full ACPI** - FADT (real shutdown), SRAT/SLIT for NUMA,
  WAET.
- **3.8 USB stack (XHCI)** - host controller, HID, storage, hub.
- **3.9 Functional shell** - builtins (`ls`, `cat`, `cd`, ...),
  pipes, redirects, globbing, history.
- **3.10 C build integration** - `build.rs` + `cc` crate to
  compile C fragments linked into the kernel.

### Definition of Done

Each block ships with an empirical test: `nproc`, `dmesg`, `vim`,
`ls -la /boot`, `ifconfig`, stress tests that round-trip the
filesystem. See `CONTEXTO_KERNEL.md` (Spanish) for the same
roadmap with extra detail.

---

## Repository Layout

```
NWIN-OS/
|-- .cargo/config.toml         Toolchain pinning, target selection
|-- .gitignore                 /target, *.log, /.vscode/, image files
|-- Cargo.toml                 Crate manifest (bin name "NWIN_OS")
|-- Cargo.lock                 Reproducible build state
|-- CONTEXTO_KERNEL.md         Spanish-language kernel design notes
|-- README.md                  This file (English + Spanish below)
|-- linker.ld                  Custom linker script
|-- x86_64-kernel.json         Custom target spec
|
|-- scripts/
|   |-- build.bat              cargo build wrapper (cd to repo root)
|   `-- rung_debug.bat         Launches QEMU with the right drives
|
|-- esp/                       EFI System Partition - boot artifacts
|   |-- EFI/BOOT/BOOTX64.EFI
|   |-- limine.conf
|   |-- startup.nsh
|   |-- NWIN_OS                The kernel binary
|   |-- initramfs.tar          Initial userspace TAR
|   |-- shell.elf              Default Ring 3 shell
|   `-- test_mem.elf           Ring 3 smoke test (brk / mmap / write)
|
|-- src/                       Kernel source (no_std Rust)
|   |-- main.rs                Limine requests, init sequence
|   |-- core/                  CPU, GDT, IDT, syscalls, panic, errors
|   |-- drivers/
|   |   |-- block/ahci/        AHCI SATA driver
|   |   |-- bus/pci.rs         PCI enumeration
|   |   |-- char/serial.rs     8250 UART
|   |   |-- display/           Framebuffer + TTY
|   |   |-- input/keyboard.rs  PS/2 keyboard + ring buffer
|   |   `-- timer/pit.rs       PIT @ 100 Hz
|   |-- fs/                    VFS, MBR, FAT, Ext4, FD table
|   |-- mm/                    Bitmap allocator + CoW + heap
|   `-- task/                  Scheduler, TaskManager, ELF loader
|
|-- userspace/
|   `-- test_mem.rs            Source of esp/test_mem.elf
|
`-- tests/
    `-- disk-images/
        |-- ext4_test.img      32 MiB ext4 fixture for QEMU
        `-- README.md          How to regenerate the image
```

---

## Building and Running

### Requirements

- **Rust nightly** (the project uses `#![feature(abi_x86_interrupt)]`).
- **QEMU** for x86_64 (tested with qemu-system-x86_64 11.x).
- **NASM** and the `cc` crate if you plan to enable the LinuxKPI
  shim (Block 3.1).
- A POSIX shell on the host (PowerShell works on Windows; bash is
  required for `tests/disk-images/README.md` regeneration).

### Toolchain

The toolchain is pinned via `.cargo/config.toml`:

```toml
[unstable]
build-std = ["core", "compiler_builtins", "alloc"]
build-std-features = ["compiler-builtins-mem"]
json-target-spec = true

[build]
target = "x86_64-kernel.json"

[target.x86_64-kernel]
rustflags = [
    "-C", "link-arg=-Tlinker.ld",
    "-C", "link-arg=-zmax-page-size=0x1000",
]
```

`rustup` and the nightly toolchain are not required to be installed
system-wide; whatever Rust you invoke `cargo` with must be nightly.

### Build

From the repository root:

```bash
# Linux / macOS / WSL
cargo build

# Windows
scripts\build.bat
```

The build emits the ELF kernel image at
`target/x86_64-kernel/debug/NWIN_OS`. Copy it to the ESP:

```bash
cp target/x86_64-kernel/debug/NWIN_OS esp/NWIN_OS
```

### Run under QEMU

```bash
"C:/Program Files/qemu/qemu-system-x86_64.exe" \
    -machine q35 \
    -drive if=pflash,format=raw,readonly=on,file="C:/Program Files/qemu/share/edk2-x86_64-code.fd" \
    -drive format=raw,file=fat:rw:esp \
    -drive format=raw,file=tests/disk-images/ext4_test.img \
    -m 512 \
    -d guest_errors \
    -serial stdio
```

Or simply run `scripts\rung_debug.bat` on Windows. The script
changes directory to the repo root first so relative paths resolve
correctly from any caller location.

You should see boot logs on the serial port showing the init
sequence: serial -> framebuffer -> GDT/IDT/syscalls -> memory ->
PCI/AHCI -> filesystem -> multitasking -> PIT. The Reaper daemon
(idle) and the user shell run after boot.

### Regenerating the ext4 test image

See `tests/disk-images/README.md`. The short version on Linux:

```bash
dd if=/dev/zero of=tests/disk-images/ext4_test.img bs=1M count=32
mkfs.ext4 -O ^has_journal,^extent,^64bit -b 4096 tests/disk-images/ext4_test.img
```

---

## Testing

There is no `cargo test` suite yet (`harness = false` and `#![no_main]`
make the standard test harness unusable). Tests today are:

- **Boot smoke test**: just `cargo build` and run under QEMU; the
  boot banner reports `Finished dev profile` and the kernel prints
  its init trace on serial.
- **userspace smoke test**: `esp/test_mem.elf` is loaded by Limine
  and exercises `write`, `brk`, `mmap`, and `exit` in Ring 3.
- **POSIX round-trip**: the kernel itself creates `nwin_core.txt`
  on the ext4 fixture at boot, writes a string, then reads it
  back via the VFS and prints the result to serial.

Block-level unit tests (using `cargo test` against `no_std` crates
that wrap kernel primitives) are planned as part of Block 2.1
(scheduler) and Block 3.1 (LinuxKPI).

---

## Contributing

Contributions are welcome and follow a Linux-kernel-style workflow:
plain text patches sent by email, reviewed by the maintainer, then
merged. This keeps the change history readable and avoids GitHub-only
artefacts in the project archive.

### Reporting Issues

- Open an issue at <https://github.com/JoseusZ/NWIN-OS/issues>.
- Include: target commit (`git rev-parse HEAD`), QEMU version,
  Rust toolchain (`rustc --version --verbose`), and a serial log
  excerpt.
- For crashes, attach the backtrace from QEMU's `-d guest_errors`
  flag and the panic message.

### Sending Patches

1. Make your changes on a topic branch:

   ```bash
   git checkout -b topic/short-description
   ```

2. Write good commit messages:

   ```
   subsystem: short imperative summary (<= 72 chars)

   More detailed explanatory text. Wrap at 72 columns. Explain
   the problem being solved, the approach taken, and any
   trade-offs or follow-ups.

   Signed-off-by: Your Name <you@example.com>
   ```

3. Format your series as a single file per patch, in `git
   format-patch` style:

   ```bash
   git format-patch -M -C -o outgoing/ master..topic/short-description
   ```

4. Send the patches by email to **joseuszoficial@gmail.com** with
   the subject prefix `[NWIN-OS]`. Use `git send-email` if you have
   it configured, or attach the `.patch` files directly.

5. Wait for review. Expect one or more rounds of review comments;
   address them with follow-up patches (`git send-email -v2 ...`).
   When the maintainer says `Acked-by`, the patches will be applied
   to the master branch.

### Coding Style

- **`rustfmt`** with the default settings (`cargo fmt` before
  sending).
- **`clippy`** clean at the default level (`cargo clippy`).
- One logical change per commit.
- Do **not** mix refactors with feature work.
- Honour the **immutability rules** (see `CONTEXTO_KERNEL.md`):
  - `core/gdt.rs`, `core/idt.rs`, `core/cpu.rs` are stable.
  - `mm/memory.rs` CoW and refcount machinery is stable.
  - `task/task.rs::Task::new` stack-frame layout is stable.
  - The Linux x86-64 syscall ABI is stable.

### License and Sign-off

NWIN OS is released under the **MIT License**. By sending a patch
you agree to license your contribution under the same terms. Add a
`Signed-off-by:` line to your commit message to certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/).

---

## Maintainer

Joseus Z - <joseuszoficial@gmail.com>

Project home: <https://github.com/JoseusZ/NWIN-OS>

---

---

# README en Espanol

> **NWIN es Neither Windows Interface nor Native Linux** (No es
> Interfaz de Windows ni Linux Nativo)

Un kernel de sistema operativo monolitico para **x86_64** escrito en
**Rust**, construido desde cero sin depender de ningun nucleo
existente. Arranca con el bootloader **Limine**, aísla procesos de
usuario en Ring 3 con espacios de direcciones por tarea, y expone una
ABI de llamadas al sistema estilo POSIX para que el tooling
compatible con Linux (binarios ELF, syscalls) corra de forma nativa.

Este repositorio contiene el kernel, ejemplos de espacio de usuario,
scripts de construccion y artefactos de prueba.

---

## Objetivo del Proyecto

NWIN OS es un ejercicio de ingenieria: construir un sistema operativo
moderno estilo Unix desde cero, en un lenguaje con seguridad de
memoria. Tres principios rectores:

1. **Familiaridad POSIX.** Las syscalls siguen la ABI Linux x86-64
   (`syscall` via `sysretq`).
2. **Stack de arranque moderno.** Bootloader **Limine** (protocolo
   Stivale2), soporte UEFI, HHDM y modulo Limine para el initramfs TAR.
3. **Rust en todo el kernel.** `#![no_std]` con `build-std`, bloques
   `unsafe` solo donde el hardware lo exige.

---

## Capacidades Actuales

(Ver seccion en ingles arriba - mismas capacidades: boot Limine, GDT
dual Ring 0/3, IDT completa, syscalls Zero-Trust, CoW con refcounts,
scheduler round-robin con CR3 por tarea, driver AHCI, VFS + FAT +
Ext4, TTY con ANSI, etc.)

### Resumen rapido por capa

| Capa | Estado |
|---|---|
| Memoria + Paginacion + CoW | 92% |
| Teclado PS/2 | 90% |
| IDT / GDT / TSS / Syscalls Ring 3 | 88% |
| VFS + MountTable | 85% |
| Ext4 (lectura) | 85% |
| FAT16/32 (lectura) | 80% |
| Scheduler preemptivo | 75% |
| Ext4 (escritura) | 70% |
| PCI Scan | 70% |
| Framebuffer / TTY | 70% |
| AHCI Driver | 65% |
| IPC / LKL Server | 0% (pendiente) |
| Networking | 0% (pendiente) |
| GPU / DRM | 0% (pendiente) |

---

## Roadmap (Plan Detallado)

### Bloque 1 - Casi terminado (>= 80%)

- **1.1** `fork` nativo (syscall 57)
- **1.2** `free_block` / `free_inode` robustos en Ext4
- **1.3** Soporte minimo de `PT_DYNAMIC` en el ELF loader
- **1.4** `futex` (syscall 202)

### Bloque 2 - En progreso (30 - 70%)

- **2.1** Scheduler con prioridades y afinidad
- **2.2** SMP - habilitar Local APIC + arranque de APs
- **2.3** Parser ACPI (RSDP, MADT, MCFG, HPET)
- **2.4** Parser GPT
- **2.5** TTY completo (cursor save/restore, scroll, double buffer)
- **2.6** Driver NVMe (PCIe)
- **2.7** Escritura FAT + LFN
- **2.8** Senales POSIX (`kill`, `sigaction`, `sigreturn`)
- **2.9** Pipes + `dup`/`dup2`/`fcntl`
- **2.10** HPET como reloj primario

### Bloque 3 - Por iniciar (0%)

- **3.1** LinuxKPI Shim bajo `compat/lkpi/` con `build.rs` + crate `cc`
- **3.2** IPC formal (rango propio de syscalls)
- **3.3** Servidor LKL en Ring 3 para blobs
- **3.4** Networking: driver `e1000`, IPv4, UDP, TCP, sockets BSD
- **3.5** GPU/DRM/KMS (VGA, Bochs, Intel i915)
- **3.6** ELF dinamico completo + `ld-linux` + relocations
- **3.7** ACPI completo (FADT, SRAT/SLIT, WAET)
- **3.8** Stack USB (XHCI)
- **3.9** Shell funcional en `shell.elf`
- **3.10** Build system C (`build.rs` + `cc`)

### Criterios de "Terminado"

Cada bloque tiene una prueba empirica verificable:
- `fork`: `cargo test` con test Ring 3 que ejecuta `fork + execve`.
- `futex`: `pthread_mutex_lock` disputado por dos hilos sin congelarse.
- SMP: `nproc` reporta el numero de nucleos fisicos.
- ACPI: `dmesg` muestra el parseo de MADT.
- GPT: disco GPT se monta como raiz (`/`).
- TTY: `vim` se renderiza sin artefactos.
- NVMe: SSD NVMe aparece como `/dev/nvme0n1`.
- LinuxKPI: `e1000.ko` carga y `ifconfig` muestra la interfaz.
- LKL: un blob de driver (p. ej. NVIDIA) corre sin Kernel Panic.
- Shell: `ls -la /boot` funciona interactivamente.

---

## Compilacion y Ejecucion

### Requisitos

- **Rust nightly** (usa `#![feature(abi_x86_interrupt)]`).
- **QEMU** para x86_64 (probado con qemu-system-x86_64 11.x).
- NASM + crate `cc` cuando se habilite el shim LinuxKPI (Bloque 3.1).

### Compilar

Desde la raiz del repositorio:

```bash
cargo build
# o en Windows:
scripts\build.bat
```

El binario ELF queda en `target/x86_64-kernel/debug/NWIN_OS`. Copialo
a la ESP:

```bash
cp target/x86_64-kernel/debug/NWIN_OS esp/NWIN_OS
```

### Ejecutar bajo QEMU

```bash
"C:/Program Files/qemu/qemu-system-x86_64.exe" \
    -machine q35 \
    -drive if=pflash,format=raw,readonly=on,file="C:/Program Files/qemu/share/edk2-x86_64-code.fd" \
    -drive format=raw,file=fat:rw:esp \
    -drive format=raw,file=tests/disk-images/ext4_test.img \
    -m 512 \
    -d guest_errors \
    -serial stdio
```

O simplemente `scripts\rung_debug.bat` en Windows. El script cambia
al directorio raiz del repo primero.

Deberias ver en el puerto serie la traza de arranque: serial ->
framebuffer -> GDT/IDT/syscalls -> memoria -> PCI/AHCI -> filesystem
-> multitarea -> PIT. El daemon Reaper (idle) y el shell de usuario
arrancan tras el boot.

### Regenerar la imagen ext4

Ver `tests/disk-images/README.md`. Version corta en Linux:

```bash
dd if=/dev/zero of=tests/disk-images/ext4_test.img bs=1M count=32
mkfs.ext4 -O ^has_journal,^extent,^64bit -b 4096 tests/disk-images/ext4_test.img
```

---

## Estructura del Repositorio

(Misma estructura que la seccion en ingles; resumen clave)

```
NWIN-OS/
|-- scripts/           Scripts de build y QEMU
|-- esp/               Particion EFI (artefactos de boot)
|-- src/               Codigo fuente del kernel
|   |-- core/          CPU, GDT, IDT, syscalls
|   |-- drivers/       AHCI, PCI, serial, framebuffer, TTY, teclado
|   |-- fs/            VFS, MBR, FAT, Ext4, FD table
|   |-- mm/            Bitmap allocator + CoW + heap
|   `-- task/          Scheduler, TaskManager, ELF loader
|-- userspace/         Programas Ring 3 (test_mem.rs)
`-- tests/
    `-- disk-images/   Imagen ext4 de prueba (regenerable)
```

---

## Como Contribuir (estilo kernel Linux)

Las contribuciones siguen el flujo del kernel de Linux: parches en
texto plano enviados por correo, revisados por el maintainer, y
fusionados a master.

### Reportar Issues

- Abre un issue en <https://github.com/JoseusZ/NWIN-OS/issues>.
- Incluye: commit (`git rev-parse HEAD`), version de QEMU, version
  de Rust (`rustc --version --verbose`), y un extracto del log serie.
- Para crashes, adjunta el backtrace de QEMU con `-d guest_errors` y
  el mensaje de panic.

### Enviar Parches

1. Crea una rama de trabajo:

   ```bash
   git checkout -b topic/descripcion-corta
   ```

2. Escribe buenos mensajes de commit:

   ```
   subsistema: resumen imperativo corto (<= 72 chars)

   Texto explicativo mas detallado. Wrap a 72 columnas. Explica
   el problema, la solucion y los trade-offs.

   Signed-off-by: Tu Nombre <tu@ejemplo.com>
   ```

3. Formatea la serie con `git format-patch`:

   ```bash
   git format-patch -M -C -o outgoing/ master..topic/descripcion-corta
   ```

4. Envia los parches por correo a **joseuszoficial@gmail.com** con
   el prefijo `[NWIN-OS]` en el subject. Usa `git send-email` si lo
   tienes configurado, o adjunta los `.patch` directamente.

5. Espera la revision. Es normal una o mas rondas de comentarios;
   responde con parches follow-up (`git send-email -v2 ...`). Cuando
   el maintainer indique `Acked-by`, los parches se aplican a master.

### Estilo de Codigo

- **`rustfmt`** con la configuracion por defecto (`cargo fmt`
  antes de enviar).
- **`clippy`** limpio en nivel por defecto (`cargo clippy`).
- Un cambio logico por commit.
- **No** mezclar refactors con trabajo de funcionalidad.
- Respetar las **reglas de inmutabilidad** (ver
  `CONTEXTO_KERNEL.md`):
  - `core/gdt.rs`, `core/idt.rs`, `core/cpu.rs` son estables.
  - La maquinaria de CoW y refcounts en `mm/memory.rs` es estable.
  - El stack-frame layout en `task/task.rs::Task::new` es estable.
  - La ABI de syscalls Linux x86-64 es estable.

### Licencia y Sign-off

NWIN OS se distribuye bajo la **Licencia MIT**. Al enviar un parche
aceptas licenciarlo bajo los mismos terminos. Agrega una linea
`Signed-off-by:` a tu commit para certificar el
[Developer Certificate of Origin 1.1](https://developercertificate.org/).

---

## Maintainer

Joseus Z - <joseuszoficial@gmail.com>

Project home: <https://github.com/JoseusZ/NWIN-OS>
