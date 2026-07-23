# NWIN OS

> **NWIN is Neither Windows Interface nor Native Linux**

NWIN OS is a monolithic **x86_64** kernel written entirely in **Rust**
(`#![no_std]`, with `alloc`). It is built from scratch with **no dependency on
any existing OS codebase**, boots via the **Limine** bootloader (Stivale2
protocol / UEFI), and is designed to be deployed on **real x86_64 hardware**
(not only emulators). The kernel isolates userland in Ring 3 with per-task
address spaces, exposes a Linux-compatible syscall ABI (`syscall`/`sysretq`),
and ships native drivers for PCI, AHCI/SATA, framebuffer consoles, PS/2 input,
and legacy PIT timers.

This repository contains the kernel source, an initramfs TAR, sample
userspace programs (Ring 3), the Limine bootloader artifacts, disk-image
fixtures for filesystem testing, and the build/run scripts used during
development.

---

## English

### Project Goal

NWIN OS is designed with a single, concrete objective: **run on real
x86_64 machines**. It is not a toy kernel, not a teaching toy, and not a
clone of Linux. It targets hardware (and faithful QEMU emulation as a
testing surrogate) with the explicit end-goal of producing a self-hosting,
Unix-leaning hobby operating system.

Three guiding principles drive the design:

1. **Hardware-first reality.** Every driver and subsystem is written to
   talk to real silicon: I/O ports, MMIO registers, MSRs, interrupt
   controllers, and PCI configuration space. QEMU is used as a
   reproduction environment; the code does not assume QEMU niceties.
2. **POSIX-leaning syscall ABI.** System calls use the Linux x86_64
   ABI (`syscall` / `sysretq`, registers `rax`/`rdi`/`rsi`/`rdx`/`r10`/`r8`/`r9`)
   so that userspace tooling (musl, glibc, hand-written assembly, ELF
   binaries) can be adapted with minimal effort.
3. **Rust everywhere.** No C in the kernel proper. `#![no_std]` with
   `build-std`, `unsafe` strictly confined to hardware-facing interfaces,
   and `spin::Mutex` / `Atomic*` / bare-metal patterns used instead of
   standard-library abstractions.

The acronym **NWIN** stands for **N**either **W**indows **I**nterface nor
**N**ative **L**inux: it is neither of them, it is its own thing.

### Runtime Identification

On boot, the kernel prints the following banner over COM1 (serial port):

```
>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<<
```

The string `NWIN OS (VERSIÓN VFS-FAT32)` is the canonical internal
identifier for the current development milestone.

---

## Espanol

### Objetivo del Proyecto

NWIN OS esta diseñado con un unico objetivo concreto: **correr en
hardware x86_64 real**. No es un kernel de juguete, no es un juguete
didactico y no es un clon de Linux. Apunta a hardware (y a una
emulacion fiel de QEMU como entorno de prueba sustituto) con la meta
explicita de producir un sistema operativo hobby auto-hospedado y de
estilo Unix.

Tres principios rectores guian el diseño:

1. **Realidad primero, hardware primero.** Cada driver y subsistema
   esta escrito para hablar con silicio real: puertos de E/S, registros
   MMIO, MSRs, controladores de interrupcion y espacio de configuracion
   PCI. QEMU se usa como entorno de reproduccion; el codigo no asume
   caprichos de QEMU.
2. **ABI de syscalls estilo POSIX.** Las llamadas al sistema usan la
   ABI Linux x86_64 (`syscall` / `sysretq`, registros
   `rax`/`rdi`/`rsi`/`rdx`/`r10`/`r8`/`r9`) para que el tooling de
   espacio de usuario (musl, glibc, ensamblador escrito a mano,
   binarios ELF) pueda adaptarse con un esfuerzo minimo.
3. **Rust en todo el kernel.** No hay C en el kernel propiamente
   dicho. `#![no_std]` con `build-std`, `unsafe` estrictamente
   confinado a interfaces de hardware, y patrones `spin::Mutex` /
   `Atomic*` / bare-metal en lugar de abstracciones de la libreria
   estandar.

El acronimo **NWIN** significa **N**either **W**indows **I**nterface
nor **N**ative **L**inux: no es ninguno de los dos, es su propia cosa.

### Identificacion en Tiempo de Ejecucion

Al arrancar, el kernel imprime el siguiente banner sobre COM1 (puerto
serie):

```
>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<<
```

La cadena `NWIN OS (VERSIÓN VFS-FAT32)` es el identificador interno
canonico del actual hito de desarrollo.

---

## Current Capabilities

The following subsystems are implemented today and working. Each item
below is cited against the source file that implements it.

### Boot and Platform

- **Limine / Stivale2 boot.** The Limine bootloader hands control to
  the kernel at `_start` (`src/main.rs`). Boot protocol requests are
  declared in the `.requests` ELF section and tagged between
  `RequestsStartMarker` / `RequestsEndMarker`.
  (`src/main.rs`)
- Requests currently issued:
  - `BaseRevision` (verified with `assert!(BASE_REVISION.is_supported())`),
  - `FramebufferRequest`,
  - `MemmapRequest`,
  - `HhdmRequest` (Higher-Half Direct Map),
  - `ModulesRequest` (the initramfs TAR arrives as a Limine module).
- **Custom Rust target.** `x86_64-kernel.json` defines a
  `x86_64-unknown-none` triple with `panic-strategy = abort`,
  `disable-redzone = true`, `linker-flavor = ld.lld`,
  `executables = true`. ([x86_64-kernel.json](x86_64-kernel.json))
- **Custom linker script.** Entry point `_start`. VMA anchored at
  `0xffffffff80200000`. Explicit PHDRs enforcing W^X (`text = RX`,
  `rodata = R`, `data = RW`). Limine requests are forced to be the
  very first entries of `.rodata` via `KEEP(*(.requests_start_marker))`,
  `KEEP(*(.requests))`, `KEEP(*(.requests_end_marker))`. Unused
  `.eh_frame` and `.note` sections are discarded in `/DISCARD/`.
  ([linker.ld](linker.ld))
- **Static PML4 + HHDM paging.** `OffsetPageTable` is built on top of
  the HHDM offset returned by Limine, and per-process PML4 frames can
  later be created with `create_isolated_pml4()`.
  ([`src/mm/memory.rs`](src/mm/memory.rs))
- **Initramfs as a Limine module.** `fs::init_filesystem()` reads the
  first `ModulesRequest` module, dereferences two `u64` slots after
  the module pointer to obtain a `*const u8` base and a `size`, and
  feeds them to `vfs::init` + `vfs::build_vfs_tree_from_tar`.
  ([`src/fs/mod.rs`](src/fs/mod.rs))

### CPU and Architecture

- **SSE / SSE2 enable.** `core::cpu::init()` removes
  `Cr0Flags::EMULATE_COPROCESSOR`, inserts
  `Cr0Flags::MONITOR_COPROCESSOR`, then sets `Cr4Flags::OSFXSR` and
  `Cr4Flags::OSXMMEXCPT_ENABLE`. ([`src/core/cpu.rs`](src/core/cpu.rs))
- **Local APIC disabled.** The MSR `0x1B` (`IA32_APIC_BASE`) has its
  bit 11 cleared so the legacy 8259 PIC is the sole interrupt
  controller in use. ([`src/core/cpu.rs`](src/core/cpu.rs))

---

## Capacidades Actuales

Los siguientes subsistemas estan implementados hoy y funcionan. Cada
punto se cita contra el archivo fuente que lo implementa.

### Arranque y Plataforma

- **Arranque Limine / Stivale2.** El bootloader Limine entrega el
  control al kernel en `_start` (`src/main.rs`). Las peticiones del
  protocolo de arranque se declaran en la seccion `.requests` del ELF
  y se delimitan entre `RequestsStartMarker` / `RequestsEndMarker`.
  ([`src/main.rs`](src/main.rs))
- Peticiones emitidas actualmente:
  - `BaseRevision` (verificada con
    `assert!(BASE_REVISION.is_supported())`),
  - `FramebufferRequest`,
  - `MemmapRequest`,
  - `HhdmRequest` (Higher-Half Direct Map),
  - `ModulesRequest` (el initramfs TAR llega como modulo de Limine).
- **Target Rust personalizado.** `x86_64-kernel.json` define el
  triple `x86_64-unknown-none` con `panic-strategy = abort`,
  `disable-redzone = true`, `linker-flavor = ld.lld`,
  `executables = true`. ([x86_64-kernel.json](x86_64-kernel.json))
- **Linker script personalizado.** Punto de entrada `_start`. VMA
  anclado en `0xffffffff80200000`. PHDRs explicitos que imponen W^X
  (`text = RX`, `rodata = R`, `data = RW`). Las peticiones de Limine
  se fuerzan a ser las primeras entradas de `.rodata` mediante
  `KEEP(*(.requests_start_marker))`, `KEEP(*(.requests))`,
  `KEEP(*(.requests_end_marker))`. Las secciones inutilizadas
  `.eh_frame` y `.note` se descartan en `/DISCARD/`.
  ([linker.ld](linker.ld))
- **PML4 estatico + paginacion con HHDM.** `OffsetPageTable` se
  construye sobre el offset del HHDM devuelto por Limine, y mas
  adelante se crean PML4s por proceso con `create_isolated_pml4()`.
  ([`src/mm/memory.rs`](src/mm/memory.rs))
- **Initramfs como modulo Limine.** `fs::init_filesystem()` lee el
  primer modulo de `ModulesRequest`, desreferencia dos `u64` despues
  del puntero al modulo para obtener una base `*const u8` y un
  `size`, y se los pasa a `vfs::init` + `vfs::build_vfs_tree_from_tar`.
  ([`src/fs/mod.rs`](src/fs/mod.rs))

### CPU y Arquitectura

- **Habilitacion de SSE / SSE2.** `core::cpu::init()` quita
  `Cr0Flags::EMULATE_COPROCESSOR`, inserta
  `Cr0Flags::MONITOR_COPROCESSOR`, y luego activa `Cr4Flags::OSFXSR`
  y `Cr4Flags::OSXMMEXCPT_ENABLE`.
  ([`src/core/cpu.rs`](src/core/cpu.rs))
- **Local APIC deshabilitado.** El MSR `0x1B` (`IA32_APIC_BASE`)
  tiene su bit 11 a cero, de modo que el 8259 PIC legado es el unico
  controlador de interrupciones en uso.
  ([`src/core/cpu.rs`](src/core/cpu.rs))

---

## Interrupts and System Calls

### Interrupt Descriptor Table

The IDT (`src/core/idt.rs`) exposes the following `extern "x86-interrupt"`
handlers. The double-fault handler is installed on the dedicated IST index
`DOUBLE_FAULT_IST_INDEX` defined in `src/core/gdt.rs`.

- `breakpoint_handler` (`#BP`, vector 3) — logs the stack frame over serial
  for debugging.
- `divide_error_handler` (`#DE`, vector 0) — divide-by-zero. Ring 3 faults
  are recovered by `exit_current_task()` (the task is marked `Dead` and
  the Reaper daemon will reclaim its address space). Ring 0 faults raise
  a structured `panic!`.
- `general_protection_fault_handler` (`#GP`, vector 13) — same Ring 3 / Ring
  0 split. Privilege is detected via `stack_frame.code_segment & 0b11 == 3`.
- `invalid_opcode_handler` (`#UD`, vector 6) — illegal instructions. Same
  Ring 3 / Ring 0 split.
- `page_fault_handler` (`#PF`, vector 14) — reads the faulting address
  from `Cr2`, attempts to resolve a Copy-on-Write fault via
  `mm::memory::resolve_cow_fault` when the page is present and written,
  then dispatches to a `KernelError::Memory::PageFault` value. Same Ring
  3 / Ring 0 split. The serial-side message
  `"[WARNING] Task Terminated: Unhandled Page Fault"` is emitted on Ring 3.
- `double_fault_handler` (`#DF`, vector 8) — diverging `panic!`. It runs
  on the IST stack configured by `TaskStateSegment::interrupt_stack_table`
  in `src/core/gdt.rs`, isolating double faults from the regular kernel
  stack.
- Hardware IRQ handlers: `timer_interrupt_handler` (IRQ 0 / vector 32)
  sends End-Of-Interrupt to the chained PIC and calls
  `task::scheduler::schedule()`; `keyboard_interrupt_handler` (IRQ 1 /
  vector 33) reads the scancode from port `0x60`, calls
  `drivers::keyboard::process_scancode`, and sends EOI;
  `spurious_interrupt_handler` (vector 39) is the empty no-EOI sink.

### Programmable Interrupt Controller

A single `ChainedPics` is wrapped in `spin::Mutex<ChainedPics>` and exposed
through `pub static PICS`. The master PIC is offset to `32` (`PIC_1_OFFSET`)
and the slave to `40` (`PIC_2_OFFSET`). Inside `idt::init()`:

1. PIT Channel 0 command byte `0x36` is sent to port `0x43`, then the
   divider low byte `0x9B` and high byte `0x2E` to port `0x40`. These
   values are an early-init mask; the actual 100 Hz divisor is later
   programmed in `drivers::pit::init()`.
2. `PICS.lock().initialize()` and `PICS.lock().write_masks(0xFC, 0xFF)`
   are called after `io_wait()` idle strokes to port `0x80`.

### System Calls

System calls are routed through `core/syscall.rs` using the hardware
`syscall` / `sysretq` mechanism:

- `Efer::update` enables `EferFlags::SYSTEM_CALL_EXTENSIONS`.
- `Star::write` loads GDT selectors (user code, user data, kernel code,
  kernel data).
- `LStar::write` stores the entry point
  `VirtAddr::new(syscall_entry as *const () as u64)`.
- `SFMask::write(RFlags::INTERRUPT_FLAG)` clears the IF bit in RFLAGS on
  kernel entry, disabling interrupts atomically.

The naked-assembly trampoline (`#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry`) does the following:

1. Saves user RSP into the global `SCRATCH_RSP` and loads
   `KERNEL_RSP`.
2. Pushes `rcx` (saved RIP), `r11` (saved RFLAGS), then `r9`/`r8`/`r10`/
   `rdx`/`rsi`/`rdi`/`rax` — the canonical `SyscallRegisters` layout.
3. Aligns RSP to 16 bytes, calls `handle_syscall_rust(regs: &mut
   SyscallRegisters)` (so `rdi` holds `rsp` from step 1 onward), then
   restores all registers, recovers the user RSP, and issues `sysretq`.

Argument registers match the **Linux x86-64 ABI**: `rax` = syscall number,
`rdi`/`rsi`/`rdx`/`r10`/`r8`/`r9` = arguments 1..6.

#### Zero-Trust user-pointer validation

`is_valid_user_memory(ptr: u64, len: usize) -> bool` (in `src/core/syscall.rs`)
checks that:

- `ptr != 0` and `len <= isize::MAX`.
- `ptr + len <= 0x00007FFFFFFFFFFF` (i.e. stays in the lower-half user
  range).
- For every page touched by the range, `mm::memory::is_user_page_mapped`
  returns `true`. This is enforced for `read`, `write`, and `open`/`execve`.

#### Currently handled syscall numbers

The `match` in `handle_syscall_rust` covers (per the source):

- **0 `sys_read`** — fd 0 uses `drivers::keyboard::pop_key()` (with a
  busy-wait and `enable_and_hlt` between attempts), fd ≥ 3 reads through
  the task's `fd_table`. fd 1/2 returns `-EBADF` (`-9`). User buffers must
  pass `is_valid_user_memory`, otherwise `-EFAULT` (`-14`).
- **1 `sys_write`** — fd 1 calls `print!(s)`; fd 2 wraps the slice in
  the red ANSI SGR (`\x1b[31m…\x1b[0m`); fds 0 or RegularFile return
  `-EBADF`. Buffers must pass `is_valid_user_memory`.
- **2 `sys_open`** — reads a NUL-terminated path from user memory (up to
  256 bytes), looks it up via `fs::vfs::open_vnode`, inserts the
  resulting `Arc<dyn VNode>` into the task's `FdTable`, returns the
  numeric fd or `-EMFILE` (`-24`) / `-ENOENT` (`-2`).
- **3 `sys_close`** — `task.fd_table.close(fd)`. Returning `false`
  yields `-EBADF`.
- **9 `sys_mmap`** — accepts `MAP_ANONYMOUS | MAP_PRIVATE`
  (`flags & 0x20 != 0`), page-aligns `length` to 4 KiB, allocates pages
  via `mm::memory::allocate_and_map_user_page(target_pml4, …)` in the
  task's `pml4_frame`. Returns the chosen base, or `-EINVAL` (`-22`),
  `-ENOMEM` (`-12`) on failure.
- **12 `sys_brk`** — manages the task's `program_break`. Zero returns the
  current break; values outside `[heap_start, 0x700000000000)` are
  rejected by returning the current break. Growth calls
  `allocate_and_map_user_page` to back new pages with the HHDM-pinned
  `SystemFrameAllocator`.
- **59 `sys_execve`** — copies `filename` and `args` from user memory
  (validated), uses `vfs::find_file` + `task::elf::load_elf`, builds an
  argv vector on a fresh user stack at `0x7FFFF0000000`, spawns the task
  with `tm.spawn_dynamic`. Returns the new `TaskId`, or `-ENOENT`,
  `-EFAULT`, `-ENOMEM`, `-EAGAIN` (`-11`), `-ENOEXEC` (`-8`).
- **60 `sys_exit`** — marks the running task `Dead` via
  `tm.exit_current_task()`, then spins on `enable_and_hlt`.
- **61 `sys_wait4`** — invokes `tm.wait_for_child(TaskId(pid))`. If the
  child is still alive the caller is blocked and `schedule()` yields;
  returns the child PID on success, `-ECHILD` (`-10`) otherwise.
- **88 (custom)** — ACPI / QEMU power-off. Writes `0x10` to port `0xf4`
  (QEMU `isa-debug-exit`) and `0x2000` to port `0x604` (ACPI power-off),
  then loops `enable_and_hlt`.
- **500 (custom)** — `without_interrupts` clear-screen + cursor-reset on
  the framebuffer `WRITER`. Returns `0`.
- Any other number: returns `-ENOSYS` (`-38`).

Syscall numbers **57 (`sys_fork`)** and **39 (`sys_getpid`)** are listed in
the project roadmap but are not yet wired into the `match` of `handle_syscall_rust`
at the time of this writing.

---

## Interrupciones y Llamadas al Sistema

### Tabla de Descriptores de Interrupcion (IDT)

La IDT ([`src/core/idt.rs`](src/core/idt.rs)) expone los siguientes
manejadores `extern "x86-interrupt"`. El manejador de doble falla se
instala en el indice IST dedicado `DOUBLE_FAULT_IST_INDEX` definido en
[`src/core/gdt.rs`](src/core/gdt.rs).

- `breakpoint_handler` (`#BP`, vector 3) — registra el stack frame por
  puerto serie para depuracion.
- `divide_error_handler` (`#DE`, vector 0) — division por cero. Si ocurre
  en Ring 3 se recupera con `exit_current_task()` (la tarea pasa a
  `Dead` y el daemon Reaper reclamara su espacio de direcciones). Si
  ocurre en Ring 0 dispara un `panic!` estructurado.
- `general_protection_fault_handler` (`#GP`, vector 13) — misma division
  Ring 3 / Ring 0. El nivel de privilegio se detecta via
  `stack_frame.code_segment & 0b11 == 3`.
- `invalid_opcode_handler` (`#UD`, vector 6) — instruccion ilegal. Misma
  division Ring 3 / Ring 0.
- `page_fault_handler` (`#PF`, vector 14) — lee la direccion que provoco
  el fallo desde `Cr2`, intenta resolver un Copy-on-Write mediante
  `mm::memory::resolve_cow_fault` cuando la pagina esta presente y la
  operacion es de escritura, y finalmente construye un valor
  `KernelError::Memory::PageFault`. Misma division Ring 3 / Ring 0. En
  Ring 3 se emite por puerto serie el mensaje
  `"[WARNING] Task Terminated: Unhandled Page Fault"`.
- `double_fault_handler` (`#DF`, vector 8) — `panic!` divergente. Corre
  sobre la pila IST configurada por
  `TaskStateSegment::interrupt_stack_table` en `src/core/gdt.rs`,
  aislando el doble fallo de la pila regular del kernel.
- Manejadores de IRQ de hardware: `timer_interrupt_handler` (IRQ 0 /
  vector 32) envia End-Of-Interrupt al PIC en cadena y llama a
  `task::scheduler::schedule()`; `keyboard_interrupt_handler` (IRQ 1 /
  vector 33) lee el scancode del puerto `0x60`, llama a
  `drivers::keyboard::process_scancode` y envia EOI;
  `spurious_interrupt_handler` (vector 39) es el sumidero vacio sin EOI.

### Controlador de Interrupciones Programable

Un unico `ChainedPics` se envuelve en `spin::Mutex<ChainedPics>` y se
expone como `pub static PICS`. El PIC maestro se desplaza a `32`
(`PIC_1_OFFSET`) y el esclavo a `40` (`PIC_2_OFFSET`). Dentro de
`idt::init()`:

1. Se envia el byte de comando `0x36` del Canal 0 del PIT al puerto
   `0x43`, y luego el byte bajo `0x9B` y el byte alto `0x2E` del divisor
   al puerto `0x40`. Estos valores son una mascara temprana; el divisor
   real de 100 Hz se programa despues en `drivers::pit::init()`.
2. Se llaman a `PICS.lock().initialize()` y
   `PICS.lock().write_masks(0xFC, 0xFF)` tras pulsos `io_wait()` al
   puerto `0x80`.

### Llamadas al Sistema

Las llamadas al sistema se enrutan por `core/syscall.rs` usando el
mecanismo hardware `syscall` / `sysretq`:

- `Efer::update` activa `EferFlags::SYSTEM_CALL_EXTENSIONS`.
- `Star::write` carga los selectores de la GDT (codigo de usuario,
  datos de usuario, codigo de kernel, datos de kernel).
- `LStar::write` almacena el punto de entrada
  `VirtAddr::new(syscall_entry as *const () as u64)`.
- `SFMask::write(RFlags::INTERRUPT_FLAG)` limpia el bit IF de RFLAGS al
  entrar al kernel, deshabilitando interrupciones atomicamente.

El trampolin en ensamblador desnudo (`#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry`) hace lo siguiente:

1. Guarda el RSP de usuario en la global `SCRATCH_RSP` y carga
   `KERNEL_RSP`.
2. Apila `rcx` (RIP guardado), `r11` (RFLAGS guardado) y luego
   `r9`/`r8`/`r10`/`rdx`/`rsi`/`rdi`/`rax` — el layout canonico de
   `SyscallRegisters`.
3. Alinea RSP a 16 bytes, llama a
   `handle_syscall_rust(regs: &mut SyscallRegisters)` (de modo que `rdi`
   recibe `rsp` desde el paso 1), restaura todos los registros, recupera
   el RSP de usuario y emite `sysretq`.

Los registros de argumentos coinciden con la **ABI Linux x86-64**:
`rax` = numero de syscall, `rdi`/`rsi`/`rdx`/`r10`/`r8`/`r9` =
argumentos 1..6.

#### Validacion Zero-Trust de punteros de usuario

`is_valid_user_memory(ptr: u64, len: usize) -> bool` (en
[`src/core/syscall.rs`](src/core/syscall.rs)) verifica que:

- `ptr != 0` y `len <= isize::MAX`.
- `ptr + len <= 0x00007FFFFFFFFFFF` (es decir, se mantiene en el rango
  user de la mitad inferior).
- Para cada pagina tocada por el rango,
  `mm::memory::is_user_page_mapped` devuelve `true`. Esto se aplica a
  `read`, `write`, `open` y `execve`.

#### Numeros de syscall manejados actualmente

El `match` de `handle_syscall_rust` cubre (segun el codigo):

- **0 `sys_read`** — fd 0 usa `drivers::keyboard::pop_key()` (con
  busy-wait y `enable_and_hlt` entre intentos), fd ≥ 3 lee a traves de
  la `fd_table` de la tarea. fd 1/2 devuelve `-EBADF` (`-9`). Los buffers
  de usuario deben pasar `is_valid_user_memory`, si no `-EFAULT` (`-14`).
- **1 `sys_write`** — fd 1 llama a `print!(s)`; fd 2 envuelve la cadena
  en SGR ANSI rojo (`\x1b[31m…\x1b[0m`); fd 0 o `RegularFile` devuelve
  `-EBADF`. Los buffers deben pasar `is_valid_user_memory`.
- **2 `sys_open`** — lee una ruta terminada en NUL desde la memoria de
  usuario (hasta 256 bytes), la busca mediante `fs::vfs::open_vnode`,
  inserta el `Arc<dyn VNode>` resultante en la `FdTable` de la tarea y
  devuelve el fd numerico, o `-EMFILE` (`-24`) / `-ENOENT` (`-2`).
- **3 `sys_close`** — `task.fd_table.close(fd)`. Si devuelve `false`,
  se entrega `-EBADF`.
- **9 `sys_mmap`** — acepta `MAP_ANONYMOUS | MAP_PRIVATE`
  (`flags & 0x20 != 0`), alinea `length` a 4 KiB, reserva paginas con
  `mm::memory::allocate_and_map_user_page(target_pml4, …)` en la
  `pml4_frame` de la tarea. Devuelve la base elegida, o `-EINVAL`
  (`-22`), `-ENOMEM` (`-12`) si falla.
- **12 `sys_brk`** — gestiona el `program_break` de la tarea. Cero
  devuelve el break actual; los valores fuera de
  `[heap_start, 0x700000000000)` se rechazan devolviendo el break actual.
  El crecimiento llama a `allocate_and_map_user_page` para respaldar
  las paginas nuevas con el `SystemFrameAllocator` anclado al HHDM.
- **59 `sys_execve`** — copia `filename` y `args` desde la memoria de
  usuario (validados), usa `vfs::find_file` + `task::elf::load_elf`,
  construye un vector argv en una pila de usuario fresca en
  `0x7FFFF0000000` y arranca la tarea con `tm.spawn_dynamic`. Devuelve
  el nuevo `TaskId`, o `-ENOENT`, `-EFAULT`, `-ENOMEM`, `-EAGAIN`
  (`-11`), `-ENOEXEC` (`-8`).
- **60 `sys_exit`** — marca la tarea actual como `Dead` mediante
  `tm.exit_current_task()`, luego gira con `enable_and_hlt`.
- **61 `sys_wait4`** — invoca `tm.wait_for_child(TaskId(pid))`. Si el
  hijo sigue vivo se bloquea al llamante y `schedule()` cede la CPU;
  devuelve el PID hijo en exito, `-ECHILD` (`-10`) en caso contrario.
- **88 (custom)** — apagado ACPI / QEMU. Escribe `0x10` al puerto `0xf4`
  (`isa-debug-exit` de QEMU) y `0x2000` al puerto `0x604` (apagado ACPI),
  y luego entra en bucle `enable_and_hlt`.
- **500 (custom)** — `without_interrupts` clear-screen + reset del cursor
  en el `WRITER` del framebuffer. Devuelve `0`.
- Cualquier otro numero: devuelve `-ENOSYS` (`-38`).

Los numeros de syscall **57 (`sys_fork`)** y **39 (`sys_getpid`)**
aparecen en la hoja de ruta del proyecto pero aun no estan cableados
en el `match` de `handle_syscall_rust` al momento de escribir esto.

---

## Memory Management

### Frame Allocator (bitmap + reference counts)

Defined in [`src/mm/memory.rs`](src/mm/memory.rs). The static
`ALLOCATOR: spin::Mutex<BitmapFrameAllocator>` is private; access
goes through `get_allocator()` and `SystemFrameAllocator`.

`BitmapFrameAllocator` keeps two parallel metadata arrays:

- A bitmap (1 bit per frame). `test_bit`, `force_set_bit`,
  `force_free_bit` manage it. Frame index `0` is always protected
  (no page mapped at physical address `0`).
- A `&'static mut [u16]` reference-count array used by the
  Copy-on-Write path.

`init()` walks the `MemmapRequest` entries, computes `max_addr` from
the union of `MEMMAP_USABLE`, `MEMMAP_BOOTLOADER_RECLAIMABLE`,
`MEMMAP_EXECUTABLE_AND_MODULES` and `MEMMAP_FRAMEBUFFER` (MMIO
ranges are deliberately ignored so they don't stretch the bitmap),
allocates `bitmap_size_aligned + (total_frames * 2)` bytes from
the first `MEMMAP_USABLE` region large enough, zeroes metadata,
initialises reference counts to 1, then frees every usable frame
back into the bitmap. The metadata frames themselves are reserved
(`force_set_bit`) afterwards.

Allocation primitives:

- `allocate_frame()` — first-free search starting at
  `last_free_frame_hint`, with wrap-around. Returns the physical
  address.
- `deallocate_frame()` — decrements `ref_counts[frame]`. The frame is
  freed in the bitmap **only** when the counter reaches 0. Double-free
  panics.
- `reference_frame()` — increments `ref_counts[frame]`. Asserts the
  frame is currently held.
- `get_ref_count()` — exposes the counter.
- `allocate_contiguous_frames(count)` — used for AHCI DMA bounce
  buffers. Searches for a contiguous run of `count` free frames and
  marks them all reserved.

`SystemFrameAllocator` (`unsafe impl PagingFrameAllocator<Size4KiB>`
for `SystemFrameAllocator`) zeroes each newly-allocated frame and
adds the HHDM offset before returning, so the paging layer sees a
ready-to-use frame.

### Paging setup

`mm::init()` (called from `main.rs`):

1. Reads the HHDM offset from `HHDM_REQUEST`.
2. Initialises `ALLOCATOR` from `MEMMAP_REQUEST`.
3. Calls `memory::isolate_and_init_paging(phys_offset, &mut
   SystemFrameAllocator)` which:
   - Reads the current `Cr3` (the Limine-installed PML4).
   - Allocates a new PML4 frame, zeroes it.
   - Copies entries `[256..512]` from Limine (kernel half).
   - Copies any non-unused entry in `[0..256]` that **lacks**
     `USER_ACCESSIBLE` — preserving HHDM and framebuffer mappings
     placed by Limine in the lower half.
   - Writes the new PML4 into `Cr3`.
   - Returns an `OffsetPageTable` rooted at the new PML4.
4. Maps the heap and initialises the global heap allocator.

### Heap

[`src/mm/allocator.rs`](src/mm/allocator.rs) defines:

- `pub const HEAP_START: usize = 0x_4444_4444_0000`.
- `pub const HEAP_SIZE: usize = 1024 * 1024` — **1 MiB** initial heap.
- `InterruptSafeHeap`, a thin wrapper around `linked_list_allocator::Heap`
  whose `alloc`/`dealloc` use `interrupts::without_interrupts` to avoid
  deadlocks against IRQ-driven alloc paths.
- `pub static ALLOCATOR: InterruptSafeHeap` is registered as the
  `#[global_allocator]` for the kernel crate.

### Copy-on-Write

Implemented in `mm::memory::resolve_cow_fault(fault_addr: VirtAddr) ->
bool`:

1. Builds a transient `OffsetPageTable` from the current `Cr3`.
2. If `ref_count == 0`, returns `false` (caller goes to the panic path).
3. If `ref_count == 1` (private page), the entry is *not* cloned —
   `mapper.update_flags(page, current_flags | WRITABLE)` is enough.
4. Otherwise (`ref_count > 1`): a fresh frame is allocated, the old page
   is copied into it via `copy_nonoverlapping` (HHDM pointer arithmetic),
   the page is unmapped and remapped with the new frame + `WRITABLE`,
   and `cow_deallocate_frame` decrements the old frame's refcount.

### Per-process address spaces

- `create_isolated_pml4() -> Option<PhysFrame>` — allocates a fresh
  PML4, zeroes it, copies entries `[256..512]` (kernel half) verbatim,
  and for `[0..255]` copies only the entries that **lack**
  `USER_ACCESSIBLE` (so HHDM/Framebuffer mappings survive but Ring 3
  memory is excluded). This surgical copy is documented in the function.
- `allocate_and_map_user_page(target_pml4, virtual_address)` — maps a
  4 KiB page **inside the supplied `target_pml4`** without touching
  `Cr3`. If the page is already mapped it returns `Ok(())` (idempotent
  for ELF segments that share a page boundary).
- `translate_in_pml4(target_pml4, virtual_address)` — translates a
  virtual address inside a foreign PML4. Used by the ELF loader to
  copy segments into a process under construction.
- `destroy_user_address_space(pml4_frame)` — walks the PML4 of a dead
  Ring 3 task from level 4 down to level 1, releases each
  `USER_ACCESSIBLE` data frame with `cow_deallocate_frame`, handles
  1 GiB / 2 MiB huge pages (they short-circuit to a single frame
  release), recycles the level 3 / 2 / 1 page tables themselves, and
  finally releases the master PML4 frame. Only user-space entries are
  touched; the kernel half is left intact.

---

## Multitasking and Scheduler

### Task representation

`src/task/task.rs`:

- `TaskId(u64)` — globally unique ID, allocated from a static
  `AtomicU64::new(1)`.
- `PrivilegeLevel` — `KernelMode` (Ring 0) or `UserMode` (Ring 3).
- `TaskContext` — the callee-saved layout used by `context_switch`:
  `rflags`, `rbp`, `rbx`, `r12`, `r13`, `r14`, `r15` (in that exact
  push order).
- `Task` — full per-task state. Important fields:
  - `parent_id: Option<TaskId>` (enables `wait4` semantics),
  - `pml4_frame: PhysFrame` (per-task address space),
  - `stack_start`, `stack_end`, `_stack: Box<[u8]>` (64 KiB stack),
  - `privilege: PrivilegeLevel`,
  - `heap_start` (default `0x0000_0002_0000_0000`),
  - `program_break`,
  - `mmap_base` (default `0x0000_4000_0000_0000`),
  - `fd_table: FdTable` (per-task).

`Task::new(entry_point, pml4_frame, privilege, user_stack_top,
parent_id)` builds the initial 64 KiB stack manually. For `UserMode`
it pushes the synthetic interrupt frame in order:
`SS`, `RSP`, `RFLAGS=0x202`, `CS`, `RIP`, then pushes the
`TaskContext`. The trampoline that consumes this frame is
`user_mode_trampoline` (one instruction: `iretq`). For `KernelMode`
it pushes a sentinel (`0xDEAD_C0DE_DEAD_C0DE`) and the entry point.

A publicly exported `unsafe fn set_tss_rsp0(rsp0: VirtAddr)` in
`src/core/gdt.rs` is what the scheduler uses to point the TSS
privilege stack table at the kernel stack of the next Ring 3 task.

`context_switch(old_rsp: *mut u64, new_rsp: u64)` is a
`#[unsafe(naked)] pub unsafe extern "sysv64"` function. It tests
that neither pointer is null, pushes callee-saved registers in the
order required by `TaskContext`, forces bit `0x200` (IF) in the saved
RFLAGS, stores `rsp` into `*old_rsp`, restores the incoming RSP and
registers, and returns. The trailing error branch issues `int3; hlt`.

### Task Manager

`src/task/task_manager.rs`:

- `TaskState` — `Ready`, `Running`, `Blocked`, `Dead` (Linux-style).
- `TaskManager` — holds `ready_queue: VecDeque<TaskId>`,
  `task_registry: BTreeMap<TaskId, Task>`,
  `task_states: BTreeMap<TaskId, TaskState>`,
  `current_task: Option<TaskId>`,
  `ticks: AtomicU64`.
- `pub static TASK_MANAGER: spin::Mutex<TaskManager> = Mutex::new(TaskManager::empty())`.
- `with_task_manager<F, R>(f: F) where F: FnOnce(&mut TaskManager) -> R`
  is the safe entry point. It wraps the call in
  `interrupts::without_interrupts` to make IPC handlers re-entrant
  against IRQ0.

`TaskManager::spawn` / `spawn_dynamic` allocate a new `Task` and push
it to `Ready` with the current task recorded as `parent_id`. Both
return `Result<TaskId, KernelError>` and reserve the 64 KiB stack
explicitly with `try_reserve_exact` (failure surfaces as
`KernelError::System::TaskCreationFailure`).

`TaskManager::kill` flips the state to `Dead`, wakes any `Blocked`
parent (so that `wait4` resumes), removes the task from
`task_states` and `task_registry`, and returns the removed `Task`
so callers can release resources.

### Scheduler

`src/task/scheduler.rs`:

- The boot RSP is a private `static mut BOOT_RSP: u64` so the very
  first context switch can target an existing frame.
- `pub unsafe fn schedule()` is invoked from `timer_interrupt_handler`.
  It:
  1. Bumps `manager.ticks`.
  2. If the current task was `Running`/`Ready`, marks it `Ready`.
  3. Asks the manager for the next task. If it differs from the current
     one:
     - Updates `manager.current_task`.
     - For `UserMode` next-tasks it points `tss.privilege_stack_table[0]`
       at `stack_end` via `set_tss_rsp0`.
     - Captures `(old_rsp_ptr, new_rsp)` and the next task's `pml4_frame`.
  4. Sends EOI to PIC (`PICS.lock().notify_end_of_interrupt(TIMER_INTERRUPT)`).
  5. Writes the next task's `stack_end` into `core::syscall::KERNEL_RSP`
     so future `syscall` entries land on the right kernel stack.
  6. If the next task's `pml4_frame` differs from the current `Cr3`, writes
     the new PML4 via `Cr3::write(pml4_frame, Cr3Flags::empty())`.
  7. Calls `context_switch(old_rsp_ptr, new_rsp)`.

`get_stats()` returns a non-locking snapshot via the `SchedulerStats`
struct (`total_tasks`, `ready_tasks`, `current_task`, `ticks`).

### Usermode trampoline (helper)

`src/task/usermode.rs` exposes `jump_to_user_mode(entry_point,
user_stack)` — a divergent `asm!` macro that synthesises an
`iretq` frame (`SS`, `RSP`, `RFLAGS=0x202`, `CS`, `RIP`) and uses
the user data/code selectors. This is the *fallback* helper used for
direct jumps; in the normal `execve` path the trampoline is
materialised by `Task::new` and consumed by `user_mode_trampoline`.

### Reaper daemon

`init_multitasking()` in [`src/task/mod.rs`](src/task/mod.rs) is the
canonical bootstrap. It:

1. Reads the current PML4.
2. Initialises the task manager.
3. Spawns `hilo_segador` as a kernel-mode task with the current PML4.
   The Reaper runs in a loop, sleeping on `enable_and_hlt`, and on
   every wake-up it scans `task_states` for `Dead` tasks. For each
   one it calls `TaskManager::kill` and, if the task was `UserMode`,
   invokes `mm::memory::destroy_user_address_space` to reclaim the
   user half of the PML4 and all its data frames.
4. Spawns the default userspace shell (`shell.elf`) using
   `tm.spawn_dynamic` with a freshly created isolated PML4. If
   `shell.elf` cannot be found in the initramfs TAR, the kernel
   `panic!`s — boot cannot continue without a userspace entry
   point.

### Synchronisation primitives

[`src/core/sync.rs`](src/core/sync.rs) provides:

- `Semaphore { count: AtomicU32, waiting_tasks: SpinMutex<VecDeque<TaskId>> }`.
  `wait()` performs `compare_exchange` loops; if the count is zero
  it disables interrupts, re-checks for a lost wake-up, then
  registers the running task in `waiting_tasks` and calls
  `tm.block_current_task()`. `signal()` increments the count and
  unblocks the head waiter.
- `TaskMutex<T>` wraps a `Semaphore(1)` and an `UnsafeCell<T>` and
  implements `Send`/`Sync` for `T: Send`.

These primitives underpin the roadmap item **futex (syscall 202)**.

---

## ELF Loader

[`src/task/elf.rs`](src/task/elf.rs) provides `pub fn load_elf(elf_slice:
&[u8], target_pml4: PhysFrame) -> Result<u64, ElfError>` with the
following `ElfError` variants: `TooSmall`, `InvalidMagicNumber`,
`Not64Bit`, `MemoryMappingFailed`.

The function:

1. Checks `elf_slice.len() >= 64`, the magic `0x7F 0x45 0x4C 0x46`, and
   that `elf_slice[4] == 2` (64-bit).
2. Reads (with `read_unaligned`) the ELF header fields directly:
   - `entry_point` from offset `0x18`,
   - `ph_offset` from offset `0x20`,
   - `ph_entry_size` from offset `0x36`,
   - `ph_entries` from offset `0x38`.
3. Walks every program header and processes only those with
   `p_type == 1` (PT_LOAD). **Cr3 is never touched** during this
   loop.
4. For each PT_LOAD segment:
   - Allocates pages in `target_pml4` via
     `allocate_and_map_user_page(target_pml4, …)`, walking from
     `p_vaddr & !0xFFF` to `(p_vaddr + p_memsz + 0xFFF) & !0xFFF`.
   - Copies `p_filesz` bytes from the ELF buffer into the new
     pages. The copy is **split at page boundaries** so the HHDM
     pointer arithmetic never wraps a page: each chunk is at most
     `4096 - offset_in_page` bytes long. Each chunk is looked up via
     `mm::memory::translate_in_pml4(target_pml4, page_base)` and the
     destination pointer is `(hhdm_offset + phys_addr + offset_in_page)`.
5. Returns `Ok(entry_point)` on success; returns
   `Err(MemoryMappingFailed)` if any allocation or translation
   failed.

`PT_DYNAMIC` is **not** yet processed; it is listed in the roadmap
(Block 1.3). Relocations, the program interpreter, and dynamic
linking remain to be implemented.

---

## Gestion de Memoria

### Asignador de Marcos (bitmap + referencias)

Definido en [`src/mm/memory.rs`](src/mm/memory.rs). El `ALLOCATOR`
estatico `spin::Mutex<BitmapFrameAllocator>` es privado; el acceso
se hace mediante `get_allocator()` y `SystemFrameAllocator`.

`BitmapFrameAllocator` mantiene dos arreglos paralelos de metadatos:

- Un bitmap (1 bit por marco). `test_bit`, `force_set_bit`,
  `force_free_bit` lo gestionan. El marco 0 esta siempre
  protegido.
- Un `&'static mut [u16]` con los contadores de referencia
  utilizados por la ruta Copy-on-Write.

`init()` recorre las entradas de `MemmapRequest`, calcula `max_addr`
a partir de la union de `MEMMAP_USABLE`, `MEMMAP_BOOTLOADER_RECLAIMABLE`,
`MEMMAP_EXECUTABLE_AND_MODULES` y `MEMMAP_FRAMEBUFFER` (se ignoran
rangos MMIO para no estirar el bitmap), reserva
`bitmap_size_aligned + (total_frames * 2)` bytes de la primera
region `MEMMAP_USABLE` lo bastante grande, pone la metadata a cero,
inicializa los contadores a 1 y devuelve a libres todos los marcos
usables. Los marcos de la propia metadata se reservan despues
(`force_set_bit`).

Primitivas de asignacion:

- `allocate_frame()` — busqueda first-free desde
  `last_free_frame_hint`, con wrap-around. Devuelve la direccion
  fisica.
- `deallocate_frame()` — decrementa `ref_counts[frame]`. El marco
  se libera en el bitmap **solo** cuando el contador llega a 0. Una
  doble liberacion provoca `panic!`.
- `reference_frame()` — incrementa `ref_counts[frame]`. Asegura que
  el marco esta en uso.
- `get_ref_count()` — expone el contador.
- `allocate_contiguous_frames(count)` — usado para buffers DMA de
  AHCI. Busca una corrida contigua de `count` marcos libres y los
  marca como reservados.

`SystemFrameAllocator` (`unsafe impl PagingFrameAllocator<Size4KiB>`
for `SystemFrameAllocator`) pone a cero cada marco recien asignado
sumando el offset HHDM antes de devolverlo.

### Configuracion de paginacion

`mm::init()` (llamado desde `main.rs`):

1. Lee el offset HHDM de `HHDM_REQUEST`.
2. Inicializa `ALLOCATOR` desde `MEMMAP_REQUEST`.
3. Llama a `memory::isolate_and_init_paging(phys_offset, &mut
   SystemFrameAllocator)` que:
   - Lee la `Cr3` actual (la PML4 instalada por Limine).
   - Reserva un nuevo marco para PML4 y lo pone a cero.
   - Copia las entradas `[256..512]` de Limine (mitad kernel).
   - Copia cualquier entrada no usada en `[0..256]` que **carezca**
     de `USER_ACCESSIBLE` — preservando los mapeos del HHDM y del
     framebuffer que Limine puso en la mitad inferior.
   - Escribe la nueva PML4 en `Cr3`.
   - Devuelve un `OffsetPageTable` enraizado en la nueva PML4.
4. Mapea el heap y lo inicializa.

### Heap

[`src/mm/allocator.rs`](src/mm/allocator.rs) define:

- `pub const HEAP_START: usize = 0x_4444_4444_0000`.
- `pub const HEAP_SIZE: usize = 1024 * 1024` — **1 MiB** de heap inicial.
- `InterruptSafeHeap`, una envoltura delgada sobre
  `linked_list_allocator::Heap` cuyo `alloc`/`dealloc` se ejecuta
  dentro de `interrupts::without_interrupts` para evitar interbloqueos
  contra rutas de asignacion disparadas por IRQ.
- `pub static ALLOCATOR: InterruptSafeHeap` se registra como
  `#[global_allocator]` para el crate del kernel.

### Copy-on-Write

Implementado en `mm::memory::resolve_cow_fault(fault_addr: VirtAddr) ->
bool`:

1. Construye un `OffsetPageTable` transitorio desde la `Cr3` actual.
2. Si `ref_count == 0`, devuelve `false`.
3. Si `ref_count == 1`, no se clona la pagina — basta con
   `mapper.update_flags(page, current_flags | WRITABLE)`.
4. En otro caso (`ref_count > 1`): se reserva un marco nuevo, se copia
   la pagina antigua en el con `copy_nonoverlapping` (aritmetica de
   punteros sobre HHDM), se desmapea y se remapea con el nuevo marco +
   `WRITABLE`, y `cow_deallocate_frame` decrementa el contador del
   marco antiguo.

### Espacios de direcciones por proceso

- `create_isolated_pml4() -> Option<PhysFrame>` — reserva una PML4
  nueva, la pone a cero, copia literalmente las entradas
  `[256..512]` (mitad kernel), y para `[0..255]` solo copia las
  entradas **sin** `USER_ACCESSIBLE` (de modo que el HHDM y el
  framebuffer sobreviven pero la memoria Ring 3 queda excluida).
  Este filtrado quirurgico esta documentado en la propia funcion.
- `allocate_and_map_user_page(target_pml4, virtual_address)` — mapea
  una pagina de 4 KiB **dentro del `target_pml4` indicado** sin tocar
  la `Cr3`. Si la pagina ya esta mapeada devuelve `Ok(())` (idempotente
  para segmentos ELF que comparten limite de pagina).
- `translate_in_pml4(target_pml4, virtual_address)` — traduce una
  direccion virtual dentro de una PML4 ajena. El cargador ELF lo usa
  para copiar segmentos a un proceso en construccion.
- `destroy_user_address_space(pml4_frame)` — recorre la PML4 de una
  tarea Ring 3 muerta desde el nivel 4 al nivel 1, libera cada marco
  de datos `USER_ACCESSIBLE` con `cow_deallocate_frame`, maneja
  paginas gigantes de 1 GiB / 2 MiB (cortocircuitando a un unico
  release), recicla las propias tablas de nivel 3 / 2 / 1 y al final
  libera el marco de la PML4 maestra. Solo se tocan entradas de
  espacio de usuario; la mitad kernel queda intacta.

---

## Multitarea y Planificador

### Representacion de tareas

`src/task/task.rs`:

- `TaskId(u64)` — ID global unico, asignado desde un `AtomicU64::new(1)`
  estatico.
- `PrivilegeLevel` — `KernelMode` (Ring 0) o `UserMode` (Ring 3).
- `TaskContext` — layout de registros callee-saved usado por
  `context_switch`: `rflags`, `rbp`, `rbx`, `r12`, `r13`, `r14`,
  `r15` (en ese orden exacto de push).
- `Task` — estado completo por tarea. Campos importantes:
  - `parent_id: Option<TaskId>` (habilita la semantica de `wait4`),
  - `pml4_frame: PhysFrame` (espacio de direcciones por tarea),
  - `stack_start`, `stack_end`, `_stack: Box<[u8]>` (pila de 64 KiB),
  - `privilege: PrivilegeLevel`,
  - `heap_start` (por defecto `0x0000_0002_0000_0000`),
  - `program_break`,
  - `mmap_base` (por defecto `0x0000_4000_0000_0000`),
  - `fd_table: FdTable` (por tarea).

`Task::new(entry_point, pml4_frame, privilege, user_stack_top,
parent_id)` construye la pila inicial de 64 KiB a mano. Para
`UserMode` apila el frame sintetico de interrupcion en orden:
`SS`, `RSP`, `RFLAGS=0x202`, `CS`, `RIP`, y luego empuja el
`TaskContext`. El trampolin que consume ese frame es
`user_mode_trampoline` (una unica instruccion: `iretq`). Para
`KernelMode` apila un centinela (`0xDEAD_C0DE_DEAD_C0DE`) y el punto
de entrada.

Hay un `pub unsafe fn set_tss_rsp0(rsp0: VirtAddr)` exportado en
`src/core/gdt.rs` que es lo que usa el planificador para apuntar la
tabla de pilas privilegiadas del TSS a la pila de kernel de la
siguiente tarea Ring 3.

`context_switch(old_rsp: *mut u64, new_rsp: u64)` es una funcion
`#[unsafe(naked)] pub unsafe extern "sysv64"`. Comprueba que ninguno
de los punteros es nulo, apila los registros callee-saved en el orden
requerido por `TaskContext`, fuerza el bit `0x200` (IF) en las RFLAGS
guardadas, almacena `rsp` en `*old_rsp`, restaura el RSP entrante y
los registros, y retorna. La rama de error final emite `int3; hlt`.

### Gestor de Tareas

`src/task/task_manager.rs`:

- `TaskState` — `Ready`, `Running`, `Blocked`, `Dead` (estilo Linux).
- `TaskManager` — contiene `ready_queue: VecDeque<TaskId>`,
  `task_registry: BTreeMap<TaskId, Task>`,
  `task_states: BTreeMap<TaskId, TaskState>`,
  `current_task: Option<TaskId>`,
  `ticks: AtomicU64`.
- `pub static TASK_MANAGER: spin::Mutex<TaskManager> = Mutex::new(TaskManager::empty())`.
- `with_task_manager<F, R>(f: F) where F: FnOnce(&mut TaskManager) -> R`
  es el punto de entrada seguro. Envuelve la llamada en
  `interrupts::without_interrupts` para hacer reentrantes los
  manejadores IPC contra IRQ0.

`TaskManager::spawn` / `spawn_dynamic` crean una nueva `Task` y la
ponen en `Ready` registrando la tarea actual como `parent_id`.
Ambos devuelven `Result<TaskId, KernelError>` y reservan la pila de
64 KiB explicitamente con `try_reserve_exact` (un fallo se convierte
en `KernelError::System::TaskCreationFailure`).

`TaskManager::kill` cambia el estado a `Dead`, despierta a cualquier
padre `Blocked` (para que `wait4` continue), elimina la tarea de
`task_states` y `task_registry` y devuelve la `Task` removida para
que el caller libere recursos.

### Planificador

`src/task/scheduler.rs`:

- El RSP de arranque es un `static mut BOOT_RSP: u64` privado para
  que el primer `context_switch` tenga un destino valido.
- `pub unsafe fn schedule()` se invoca desde
  `timer_interrupt_handler`. Hace:
  1. Incrementa `manager.ticks`.
  2. Si la tarea actual estaba `Running`/`Ready`, la marca como
     `Ready`.
  3. Pide al gestor la siguiente tarea. Si difiere de la actual:
     - Actualiza `manager.current_task`.
     - Para tareas siguientes `UserMode` apunta
       `tss.privilege_stack_table[0]` a `stack_end` mediante
       `set_tss_rsp0`.
     - Captura `(old_rsp_ptr, new_rsp)` y la `pml4_frame` de la
       siguiente tarea.
  4. Envia EOI al PIC
     (`PICS.lock().notify_end_of_interrupt(TIMER_INTERRUPT)`).
  5. Escribe `stack_end` de la siguiente tarea en
     `core::syscall::KERNEL_RSP` para que futuros `syscall` caigan
     en la pila correcta.
  6. Si la `pml4_frame` siguiente difiere de la `Cr3` actual, escribe
     la nueva PML4 con `Cr3::write(pml4_frame, Cr3Flags::empty())`.
  7. Llama a `context_switch(old_rsp_ptr, new_rsp)`.

`get_stats()` devuelve una instantanea sin bloqueo a traves de
`SchedulerStats` (`total_tasks`, `ready_tasks`, `current_task`,
`ticks`).

### Trampolin a modo usuario (helper)

`src/task/usermode.rs` expone `jump_to_user_mode(entry_point,
user_stack)` — un `asm!` divergente que sintetiza un frame `iretq`
(`SS`, `RSP`, `RFLAGS=0x202`, `CS`, `RIP`) usando los selectores
user data/code. Es el helper de *salto directo*; en el camino
normal de `execve` el trampolin se materializa en `Task::new` y lo
consume `user_mode_trampoline`.

### Daemon Reaper

`init_multitasking()` en [`src/task/mod.rs`](src/task/mod.rs) es el
bootstrap canonico. Hace:

1. Lee la PML4 actual.
2. Inicializa el gestor de tareas.
3. Crea `hilo_segador` como tarea kernel-mode con la PML4 actual.
   El Reaper corre en bucle, durmiendo con `enable_and_hlt` y, en
   cada despertar, recorre `task_states` buscando tareas `Dead`. Por
   cada una llama a `TaskManager::kill` y, si era `UserMode`, invoca
   `mm::memory::destroy_user_address_space` para reclamar la mitad
   user de la PML4 y todos sus marcos de datos.
4. Crea el shell de espacio de usuario por defecto (`shell.elf`)
   con `tm.spawn_dynamic` y una PML4 aislada nueva. Si `shell.elf`
   no se encuentra en el TAR del initramfs, el kernel hace
   `panic!` — el arranque no puede continuar sin un punto de entrada
   en espacio de usuario.

### Primitivas de sincronizacion

[`src/core/sync.rs`](src/core/sync.rs) provee:

- `Semaphore { count: AtomicU32, waiting_tasks: SpinMutex<VecDeque<TaskId>> }`.
  `wait()` hace bucles `compare_exchange`; si el contador es cero
  deshabilita interrupciones, re-comprueba por un wake-up perdido,
  registra la tarea actual en `waiting_tasks` y llama a
  `tm.block_current_task()`. `signal()` incrementa el contador y
  desbloquea al primero de la cola.
- `TaskMutex<T>` envuelve un `Semaphore(1)` y un `UnsafeCell<T>`, e
  implementa `Send`/`Sync` para `T: Send`.

Estas primitivas apuntalan el elemento `futex (syscall 202)` de la
hoja de ruta.

---

## Cargador ELF

[`src/task/elf.rs`](src/task/elf.rs) provee
`pub fn load_elf(elf_slice: &[u8], target_pml4: PhysFrame) -> Result<u64,
ElfError>` con las siguientes variantes de `ElfError`: `TooSmall`,
`InvalidMagicNumber`, `Not64Bit`, `MemoryMappingFailed`.

La funcion:

1. Verifica `elf_slice.len() >= 64`, la magia `0x7F 0x45 0x4C 0x46` y
   que `elf_slice[4] == 2` (64 bits).
2. Lee (con `read_unaligned`) los campos del ELF directamente:
   - `entry_point` desde el offset `0x18`,
   - `ph_offset` desde el offset `0x20`,
   - `ph_entry_size` desde el offset `0x36`,
   - `ph_entries` desde el offset `0x38`.
3. Recorre todos los program headers y procesa solo los
   `p_type == 1` (PT_LOAD). **Nunca se toca la `Cr3`** durante este
   bucle.
4. Para cada segmento PT_LOAD:
   - Reserva paginas en `target_pml4` mediante
     `allocate_and_map_user_page(target_pml4, …)`, desde
     `p_vaddr & !0xFFF` hasta `(p_vaddr + p_memsz + 0xFFF) & !0xFFF`.
   - Copia `p_filesz` bytes del buffer ELF a las paginas nuevas. La
     copia se **parte en los limites de pagina** para que la aritmetica
     de punteros sobre HHDM nunca se salga de una pagina: cada trozo
     ocupa como maximo `4096 - offset_in_page` bytes. Cada trozo se
     resuelve con `mm::memory::translate_in_pml4(target_pml4,
     page_base)` y el puntero destino es
     `(hhdm_offset + phys_addr + offset_in_page)`.
5. Devuelve `Ok(entry_point)` en exito; si alguna asignacion o
   traduccion falla, devuelve `Err(MemoryMappingFailed)`.

`PT_DYNAMIC` **no** se procesa todavia; figura en la hoja de ruta
(Bloque 1.3). Las reubicaciones, el interprete de programa y el
enlazado dinamico quedan por implementar.

---

## Drivers

### Serial 8250 UART (COM1)

[`src/drivers/char/serial.rs`](src/drivers/char/serial.rs). A
`SerialPort` wraps the six I/O ports at base `0x3F8` (COM1): `data`,
`int_en`, `fifo_ctrl`, `line_ctrl`, `modem_ctrl`, `line_sts`. The
initialisation sequence in `init()` is exactly:

1. `int_en.write(0x00)` — disable UART-generated interrupts.
2. `line_ctrl.write(0x80)` — set DLAB to expose the baud-rate divisor.
3. `data.write(0x03)` — divisor low byte (`0x0300` ⇒ 38400 baud
   on a 1.8432 MHz crystal); `int_en.write(0x00)` — divisor high
   byte.
4. `line_ctrl.write(0x03)` — 8 data bits, no parity, 1 stop bit.
5. `fifo_ctrl.write(0xC7)` — enable FIFOs, clear TX/RX buffers,
   threshold at 14 bytes.
6. `modem_ctrl.write(0x0B)` — raise DTR / RTS / OUT2 (the chip is
   "ready").

`send(data)` waits in `wait_for_tx_empty` (poll bit 5 of
`line_sts`), then writes the byte. The `'\n'` case prepends a
`'\r'` for classic terminal semantics.

`serial_print!` / `serial_println!` macros are defined in this file.
The backing `lazy_static! SERIAL1: Mutex<SerialPort>` is wrapped in
`interrupts::without_interrupts` inside `_print`, preventing the
classic dead-lock of "timer IRQ prints while SERIAL1 is held".

### PIT (Programmable Interval Timer)

[`src/drivers/timer/pit.rs`](src/drivers/timer/pit.rs). `init()`
sends the command byte `0x36` to port `0x43` (Channel 0, Access
lo/hi, Mode 3 — square wave, binary), then writes the divisor
`11931` as low / high bytes to port `0x40`. The math:

```
divisor = 1_193_182 / 100 = 11_931   →   IRQ0 every ~10 ms (100 Hz)
```

The kernel prints `[OK] Reloj PIT arrancado a 100Hz.` on success.
This `init` runs last in `_start`, before the boot thread executes
`x86_64::instructions::interrupts::enable_and_hlt`.

### PCI bus scanner

[`src/drivers/bus/pci.rs`](src/drivers/bus/pci.rs). The scanner
uses the legacy I/O pair `0xCF8` / `0xCFC`
(`PCI_CONFIG_ADDRESS_PORT` / `PCI_CONFIG_DATA_PORT`).

`pci_config_read_u32(bus, slot, func, offset)` and
`pci_config_write_u32(...)` build the address as:

```
address = 0x80000000
        | (bus        << 16)
        | (slot       << 11)
        | (func       <<  8)
        | (offset & 0xFC)
```

`Vendor::new(id)` recognises `0x8086` (Intel), `0x1022` (AMD),
`0x10DE` (NVIDIA), `0x1234` (QEMU); `0xFFFF` is rejected as an
unpopulated slot. `DeviceType::new(class, subclass)` maps:

| Class / Subclass | Meaning |
|---|---|
| `0x01 / 0x01` | IDE Controller |
| `0x01 / 0x06` | SATA Controller |
| `0x02 / 0x00` | Ethernet Controller |
| `0x03 / 0x00` | VGA-compatible Controller |
| `0x06 / 0x00` | Host Bridge |
| `0x06 / 0x01` | ISA Bridge |

`PciDevice::enable_mmio` sets bit 1 of the command register
(offset `0x04`); `enable_bus_mastering` sets bit 2; `get_bar5`
reads offset `0x24` and masks the low 4 bits.

`scan_pci_bus()` enumerates buses `0..=255`, slots `0..32`,
functions `0..8` (only scanning `8` functions when the header type
indicates a multi-function device). The result is a `Vec<PciDevice>`
logged via serial.

`pci::init() -> Option<u32>` iterates the discovered devices, looks
for a `SataController`, reads its BAR5, enables MMIO and Bus
Mastering, prints the result over serial and forwards BAR5 to
`crate::drivers::block::ahci::init(bar5)`. Returns `Some(bar5)` or
`None`.

### AHCI / SATA driver

[`src/drivers/block/ahci/`](src/drivers/block/ahci/) is split into
three modules.

`regs.rs` defines:

- `pub struct Volatile<T>(T)` wrapping `core::ptr::read_volatile` /
  `core::ptr::write_volatile`.
- `bitflags! HbaHostCont: u32 { HR, IE, MRSM, AE }`.
- `bitflags! HbaPortCmd: u32 { ST, SUD, POD, CLO, FRE, FR, CR }`.
- `enum AtaCommand { ReadDma = 0xC8, ReadDmaExt = 0x25, WriteDma =
  0xCA, WriteDmaExt = 0x35, IdentifyDevice = 0xEC }`.

`port.rs` defines:

- `#[repr(C)] HbaPort` (clb, clbu, fb, fbu, is, ie, cmd, _reserved,
  tfd, sig, ssts, sctl, serr, sact, ci, sntf, fbs, devslp,
  _reserved_1[10], vendor[4]).
- `#[repr(C)] HbaCmdHeader` (flags, prdtl, prdbc, ctba, ctbau,
  _reserved[4]).
- `#[repr(C)] HbaPrdtEntry` (dba, dbau, _reserved, flags).
- `#[repr(C)] HbaCmdTbl` (cfis[64], acmd[16], _reserved[48],
  prdt_entry[1]).

`HbaPort::stop_cmd` clears `ST | FRE` from `cmd` and spins until
`!(CR | FR)`.

`HbaPort::read_sector(lba, buffer_phys_addr, hhdm_offset)`:

1. `is.write(0xFFFFFFFF)` — clear all pending interrupts.
2. Point at slot 0 of the command-list slice (32 headers).
3. Set `header.flags.write(5)` (5 DWORDS in the FIS), `prdtl.write(1)`,
   `prdbc.write(0)`.
4. Write the PRDT entry's `dba` / `dbau` to `buffer_phys_addr`,
   `flags.write(511)` (one 512-byte byte region).
5. Fill the host-to-device FIS at `cfis[0..13]`: `0x27` (H2D
   register), `0x80` (command bit), `0x25` (READ DMA EXT), 6-byte
   LBA (`cfis[4..10]`), LBA mode marker `0x40` at `cfis[7]`, sector
   count `1`.
6. Spin on `tfd & (0x80 | 0x08)` with a one-million-iteration
   safety break.
7. Issue `ci.write(1 << slot)` and poll until `ci & (1 << slot)`
   clears, checking `(is & (1 << 30))` (task-file error) for
   failures.

`write_sector` is the same shape with `header.flags.write(0x45)` (5
DWORDS + Write bit) and FIS command `0x35` (WRITE DMA EXT).

`mod.rs` defines `#[repr(C)] HbaMemory` (host_capability,
global_host_control, interrupt_status, ports_implemented, version,
ccc_control, ccc_ports, enclosure_management_location,
enclosure_management_control, host_capabilities_extended,
bios_handoff_ctrl_sts, _reserved [0xA0..0x2C], vendor[0x100..0xA0],
ports[32]).

`init(bar5_address: u32)`:

1. Resolves the HHDM offset.
2. Builds a PML4-rooted `OffsetPageTable` via
   `memory::SystemFrameAllocator`.
3. Maps 2 × 4096 bytes of the BAR5 region with
   `PRESENT | WRITABLE | NO_CACHE | WRITE_THROUGH`.
4. Sets `HbaHostCont::AE` in `global_host_control`.
5. Walks `ports_implemented` (32 bits). For each bit set:
   - Reads `port.ssts`. The combination `device_detection ==
     3 && power_management == 1` indicates a present device.
   - `sig == 0xEB140101` ⇒ ATAPI CD-ROM, ignored.
   - `sig == 0x00000101` ⇒ ATA disk. The driver:
     - Calls `port.stop_cmd()`.
     - Allocates 3 contiguous frames via
       `mm::memory::get_allocator().allocate_contiguous_frames(3)`
       and zeroes them. The first 4 KiB is the Command List (32
       headers × 32 bytes ⇒ 1 KiB but 4 KiB reserved); the second
       KiB is the FIS receive area.
     - Sets `port.clb / clbu` and `port.fb / fbu` accordingly.
     - For each of the 32 slots, sets `prdtl` to 8 and points
       `ctba / ctbau` at offset 4096 + slot × 256 inside the
       allocated block.
     - Issues `port.start_cmd()`.
     - Constructs `Arc<AhciDisk { port_index, bar5_virt }>` and
       hands it to `fs::manager::process_disk(disk)`.

`AhciDisk` (in [`src/drivers/block/mod.rs`](src/drivers/block/mod.rs))
implements `BlockDevice`:

- `read_block(lba, buffer)` — allocates a contiguous frame as a DMA
  bounce buffer, copies the data into `buffer`, then returns the
  frame to the allocator.
- `write_block(lba, buf)` — same shape: allocate, copy `buf` into the
  bounce buffer, fire the command, then deallocate.

### Framebuffer and TTY

[`src/drivers/display/fb.rs`](src/drivers/display/fb.rs). `FrameBuffer`
holds `ptr: *mut u8`, `width`, `height`, `pitch`, `bytes_per_pixel`.
The struct is `unsafe impl Send + Sync`. `draw_pixel` writes
`RGB` at byte offset `(y * pitch) + (x * bytes_per_pixel)`.
`fill_rect` and `clear` iterate the screen pixel by pixel.
`draw_char(x, y, byte: u8, fg, bg)` looks up
`font8x8::BASIC_FONTS.get(byte as char)`, iterates the 8×8 glyph
and paints each bit at `(x + bit_i, y + row_i)`.

[`src/drivers/display/tty.rs`](src/drivers/display/tty.rs).
`Writer` wraps a `FrameBuffer`, tracks `x_pos / y_pos` in 8-pixel
cells, manages a cursor (draw / erase as a 8×2 rectangle), and
runs an ANSI state machine (`Normal → Escape → Csi`).

`write_byte(byte)` handles `0x1B` (start escape), `'\n'`, `'\r'`,
`0x08` (backspace) and printable ASCII. In CSI mode it accumulates up
to four numeric parameters and dispatches:

- `m` — SGR colour codes `30..=37` (fg) and `40..=47` (bg); `0`
  resets, `39` / `49` are equivalent (not explicitly handled
  beyond index).
- `H` (or `f`) — cursor positioning at `(params[0]-1, params[1]-1)`,
  converted back to pixel coordinates by multiplying by 8.
- `J` — full clear when `params[0] == 2`.
- `K` — erase to end-of-line when mode `0`.

`backspace` steps the cursor left by 8 pixels (or wraps to the
previous line near the left margin) and fills the previous cell with
the background colour. `new_line` increments `y_pos` by 8; when
`y_pos + 8 >= height`, it wraps to the top of the screen and clears
the display (`clear_screen` is used as a wrap-around stand-in for
true scrolling).

`Writer` implements `fmt::Write` so `print!` / `println!` work on it.

[`src/drivers/display/mod.rs`](src/drivers/display/mod.rs) holds
`pub static WRITER: Mutex<Option<Writer>>`. The `_print(args)` helper
wraps the lock acquisition in
`interrupts::without_interrupts` and erases/draws the cursor around
the actual `write_fmt` call.

`init()` walks the `FramebufferRequest` response, takes the first
framebuffer, fills it black, builds a `Writer`, and stores it in
`WRITER`. `print!` / `println!` are exported from this module.

### PS/2 Keyboard

[`src/drivers/input/keyboard.rs`](src/drivers/input/keyboard.rs).

- A 256-slot ring buffer (`BUF_SIZE = 256`) of `char`. `HEAD` and
  `TAIL` are `AtomicUsize`. `push_key(c)` reads `TAIL` with
  `Relaxed`, advances it with `Release`, and discards the keystroke
  when the buffer is full. `pop_key()` reads `HEAD` with `Relaxed`,
  checks against `TAIL` loaded with `Acquire`, and advances `HEAD`
  with `Release`. The buffer is a `static mut`, not behind a
  `Mutex`, so it can be touched directly from IRQ1.
- The decoder is `pc_keyboard::Keyboard<layouts::Us104Key,
  ScancodeSet1>` wrapped in `lazy_static! Mutex<Keyboard<...>>` with
  `HandleControl::MapLettersToUnicode`.
- `process_scancode(scancode)` is the IRQ1 entry point. It pushes
  raw `Unicode` characters (including the `'\x03'` Ctrl+C check
  that calls `tm.exit_current_task()` for tasks with `id.0 > 1`
  and prints `^C`), and translates the raw arrow keys into
  three-byte ANSI sequences:
  - `ArrowUp`    → `\x1b[A`
  - `ArrowDown`  → `\x1b[B`
  - `ArrowRight` → `\x1b[C`
  - `ArrowLeft`  → `\x1b[D`
- `Return` pushes `'\n'` and `Backspace` pushes `'\x08'`.

---

## Controladores

### 8250 UART (puerto serie, COM1)

[`src/drivers/char/serial.rs`](src/drivers/char/serial.rs). Un
`SerialPort` envuelve los seis puertos de E/S con base `0x3F8`
(COM1): `data`, `int_en`, `fifo_ctrl`, `line_ctrl`, `modem_ctrl`,
`line_sts`. La secuencia de inicializacion en `init()` es
exactamente:

1. `int_en.write(0x00)` — deshabilita las interrupciones generadas
   por la UART.
2. `line_ctrl.write(0x80)` — activa DLAB para exponer el divisor
   de baud-rate.
3. `data.write(0x03)` — byte bajo del divisor (`0x0300` ⇒ 38400
   baud sobre un cristal de 1.8432 MHz); `int_en.write(0x00)` —
   byte alto del divisor.
4. `line_ctrl.write(0x03)` — 8 bits de datos, sin paridad, 1 bit
   de stop.
5. `fifo_ctrl.write(0xC7)` — activa las FIFOs, limpia los buffers
   TX/RX, umbral de 14 bytes.
6. `modem_ctrl.write(0x0B)` — levanta DTR / RTS / OUT2 (el chip
   esta "listo").

`send(data)` espera en `wait_for_tx_empty` (poll bit 5 de
`line_sts`) y luego escribe el byte. El caso `'\n'` antepone un
`'\r'` para la semantica clasica de terminal.

Las macros `serial_print!` / `serial_println!` se definen en este
archivo. El `lazy_static! SERIAL1: Mutex<SerialPort>` se envuelve
en `interrupts::without_interrupts` dentro de `_print`, evitando el
clasico interbloqueo "timer IRQ imprime mientras SERIAL1 esta
retenido".

### PIT (Temporizador de Intervalo Programable)

[`src/drivers/timer/pit.rs`](src/drivers/timer/pit.rs). `init()`
envia el byte de comando `0x36` al puerto `0x43` (Canal 0, acceso
lo/hi, Modo 3 — onda cuadrada, binario) y luego escribe el divisor
`11931` como bytes bajo / alto al puerto `0x40`. El calculo:

```
divisor = 1_193_182 / 100 = 11_931   →   IRQ0 cada ~10 ms (100 Hz)
```

El kernel imprime `[OK] Reloj PIT arrancado a 100Hz.` al terminar.
Este `init` corre al final en `_start`, antes de que el hilo de
arranque ejecute `x86_64::instructions::interrupts::enable_and_hlt`.

### Escaner del bus PCI

[`src/drivers/bus/pci.rs`](src/drivers/bus/pci.rs). El escaner usa
el par de E/S legado `0xCF8` / `0xCFC`
(`PCI_CONFIG_ADDRESS_PORT` / `PCI_CONFIG_DATA_PORT`).

`pci_config_read_u32(bus, slot, func, offset)` y
`pci_config_write_u32(...)` construyen la direccion asi:

```
address = 0x80000000
        | (bus        << 16)
        | (slot       << 11)
        | (func       <<  8)
        | (offset & 0xFC)
```

`Vendor::new(id)` reconoce `0x8086` (Intel), `0x1022` (AMD),
`0x10DE` (NVIDIA), `0x1234` (QEMU); `0xFFFF` se rechaza como slot
no poblado. `DeviceType::new(class, subclass)` mapea:

| Clase / Subclase | Significado |
|---|---|
| `0x01 / 0x01` | Controlador IDE |
| `0x01 / 0x06` | Controlador SATA |
| `0x02 / 0x00` | Controlador Ethernet |
| `0x03 / 0x00` | Controlador compatible VGA |
| `0x06 / 0x00` | Host Bridge |
| `0x06 / 0x01` | ISA Bridge |

`PciDevice::enable_mmio` activa el bit 1 del registro de comandos
(offset `0x04`); `enable_bus_mastering` activa el bit 2;
`get_bar5` lee el offset `0x24` y enmascara los 4 bits bajos.

`scan_pci_bus()` enumera buses `0..=255`, slots `0..32`,
funciones `0..8` (solo escanea 8 funciones cuando el header type
indica un dispositivo multifuncion). El resultado es un
`Vec<PciDevice>` que se registra por puerto serie.

`pci::init() -> Option<u32>` itera los dispositivos descubiertos,
busca un `SataController`, lee su BAR5, activa MMIO y Bus
Mastering, imprime el resultado por puerto serie y le pasa el
BAR5 a `crate::drivers::block::ahci::init(bar5)`. Devuelve
`Some(bar5)` o `None`.

### Controlador AHCI / SATA

[`src/drivers/block/ahci/`](src/drivers/block/ahci/) se divide en
tres modulos.

`regs.rs` define:

- `pub struct Volatile<T>(T)` envolviendo
  `core::ptr::read_volatile` / `core::ptr::write_volatile`.
- `bitflags! HbaHostCont: u32 { HR, IE, MRSM, AE }`.
- `bitflags! HbaPortCmd: u32 { ST, SUD, POD, CLO, FRE, FR, CR }`.
- `enum AtaCommand { ReadDma = 0xC8, ReadDmaExt = 0x25,
  WriteDma = 0xCA, WriteDmaExt = 0x35, IdentifyDevice = 0xEC }`.

`port.rs` define:

- `#[repr(C)] HbaPort` (clb, clbu, fb, fbu, is, ie, cmd,
  _reserved, tfd, sig, ssts, sctl, serr, sact, ci, sntf, fbs,
  devslp, _reserved_1[10], vendor[4]).
- `#[repr(C)] HbaCmdHeader` (flags, prdtl, prdbc, ctba, ctbau,
  _reserved[4]).
- `#[repr(C)] HbaPrdtEntry` (dba, dbau, _reserved, flags).
- `#[repr(C)] HbaCmdTbl` (cfis[64], acmd[16], _reserved[48],
  prdt_entry[1]).

`HbaPort::stop_cmd` limpia `ST | FRE` de `cmd` y gira hasta que
`!(CR | FR)`.

`HbaPort::read_sector(lba, buffer_phys_addr, hhdm_offset)`:

1. `is.write(0xFFFFFFFF)` — limpia todas las interrupciones
   pendientes.
2. Apunta al slot 0 del slice de la command-list (32 headers).
3. Pone `header.flags.write(5)` (5 DWORDS en el FIS),
   `prdtl.write(1)`, `prdbc.write(0)`.
4. Escribe la entrada PRDT con `dba / dbau` apuntando a
   `buffer_phys_addr`, `flags.write(511)` (una region de 512 bytes).
5. Llena el FIS Host-to-Device en `cfis[0..13]`: `0x27` (registro
   H2D), `0x80` (bit de comando), `0x25` (READ DMA EXT), LBA de 6
   bytes (`cfis[4..10]`), marcador LBA `0x40` en `cfis[7]`, sector
   count `1`.
6. Hace spin en `tfd & (0x80 | 0x08)` con un corte de seguridad
   de un millon de iteraciones.
7. Emite `ci.write(1 << slot)` y sondea hasta que `ci & (1 << slot)`
   se limpie, comprobando tambien `(is & (1 << 30))` (task-file
   error).

`write_sector` tiene la misma forma con
`header.flags.write(0x45)` (5 DWORDS + bit de Write) y comando FIS
`0x35` (WRITE DMA EXT).

`mod.rs` define `#[repr(C)] HbaMemory` (host_capability,
global_host_control, interrupt_status, ports_implemented, version,
ccc_control, ccc_ports, enclosure_management_location,
enclosure_management_control, host_capabilities_extended,
bios_handoff_ctrl_sts, _reserved [0xA0..0x2C], vendor[0x100..0xA0],
ports[32]).

`init(bar5_address: u32)`:

1. Resuelve el offset HHDM.
2. Construye un `OffsetPageTable` enraizado en la PML4 con
   `memory::SystemFrameAllocator`.
3. Mapea 2 × 4096 bytes de la region BAR5 con
   `PRESENT | WRITABLE | NO_CACHE | WRITE_THROUGH`.
4. Activa `HbaHostCont::AE` en `global_host_control`.
5. Recorre `ports_implemented` (32 bits). Para cada bit activo:
   - Lee `port.ssts`. La combinacion `device_detection == 3 &&
     power_management == 1` indica un dispositivo presente.
   - `sig == 0xEB140101` ⇒ ATAPI CD-ROM, se ignora.
   - `sig == 0x00000101` ⇒ disco ATA. El driver:
     - Llama a `port.stop_cmd()`.
     - Reserva 3 marcos contiguos con
       `mm::memory::get_allocator().allocate_contiguous_frames(3)`
       y los pone a cero. El primer 4 KiB es la Command List
       (32 headers × 32 bytes ⇒ 1 KiB pero se reservan 4 KiB); el
       segundo KiB es el area de recepcion FIS.
     - Pone `port.clb / clbu` y `port.fb / fbu` correspondientemente.
     - Para cada uno de los 32 slots pone `prdtl` a 8 y apunta
       `ctba / ctbau` al offset 4096 + slot × 256 dentro del bloque
       reservado.
     - Emite `port.start_cmd()`.
     - Construye `Arc<AhciDisk { port_index, bar5_virt }>` y lo
       entrega a `fs::manager::process_disk(disk)`.

`AhciDisk` (en [`src/drivers/block/mod.rs`](src/drivers/block/mod.rs))
implementa `BlockDevice`:

- `read_block(lba, buffer)` — reserva un marco contiguo como
  bounce buffer DMA, copia los datos al `buffer` y devuelve el
  marco al asignador.
- `write_block(lba, buf)` — misma forma: reservar, copiar `buf` al
  bounce buffer, disparar el comando, liberar.

### Framebuffer y TTY

[`src/drivers/display/fb.rs`](src/drivers/display/fb.rs).
`FrameBuffer` contiene `ptr: *mut u8`, `width`, `height`, `pitch`,
`bytes_per_pixel`. Implementa `unsafe Send + Sync`. `draw_pixel`
escribe `RGB` en el offset `(y * pitch) + (x * bytes_per_pixel)`.
`fill_rect` y `clear` iteran la pantalla pixel a pixel.
`draw_char(x, y, byte: u8, fg, bg)` consulta
`font8x8::BASIC_FONTS.get(byte as char)`, recorre el glifo 8×8 y
pinta cada bit en `(x + bit_i, y + row_i)`.

[`src/drivers/display/tty.rs`](src/drivers/display/tty.rs).
`Writer` envuelve un `FrameBuffer`, mantiene `x_pos / y_pos` en
celdas de 8 pixeles, gestiona el cursor (dibujar / borrar como un
rectangulo de 8×2) y corre una maquina de estados ANSI
(`Normal → Escape → Csi`).

`write_byte(byte)` trata `0x1B` (iniciar escape), `'\n'`, `'\r'`,
`0x08` (backspace) y ASCII imprimible. En modo CSI acumula hasta
cuatro parametros numericos y dispatcha:

- `m` — codigos SGR `30..=37` (fg) y `40..=47` (bg); `0` resetea,
  `39` / `49` son equivalentes (no se manejan explicitamente mas
  alla del indice).
- `H` (o `f`) — posicionamiento del cursor en
  `(params[0]-1, params[1]-1)`, vuelto a pixeles multiplicando por 8.
- `J` — limpieza completa cuando `params[0] == 2`.
- `K` — borrar hasta fin de linea cuando el modo es `0`.

`backspace` retrocede el cursor 8 pixeles (o envuelve a la linea
anterior cerca del margen izquierdo) y rellena la celda anterior
con el color de fondo. `new_line` incrementa `y_pos` en 8; cuando
`y_pos + 8 >= height`, vuelve al tope de la pantalla y limpia el
display (`clear_screen` se usa como sustituto de un scroll real).

`Writer` implementa `fmt::Write` para que `print!` / `println!`
funcionen sobre el.

[`src/drivers/display/mod.rs`](src/drivers/display/mod.rs) expone
`pub static WRITER: Mutex<Option<Writer>>`. El helper `_print(args)`
envuelve la adquisicion del candado en `interrupts::without_interrupts`
y borra/dibuja el cursor alrededor de la llamada real a `write_fmt`.

`init()` recorre la respuesta de `FramebufferRequest`, toma el
primer framebuffer, lo pinta de negro, construye un `Writer` y lo
guarda en `WRITER`. Las macros `print!` / `println!` se exportan
desde este modulo.

### Teclado PS/2

[`src/drivers/input/keyboard.rs`](src/drivers/input/keyboard.rs).

- Un anillo de 256 entradas (`BUF_SIZE = 256`) de `char`. `HEAD` y
  `TAIL` son `AtomicUsize`. `push_key(c)` lee `TAIL` con `Relaxed`,
  lo avanza con `Release` y descarta la pulsacion si el buffer esta
  lleno. `pop_key()` lee `HEAD` con `Relaxed`, lo compara con
  `TAIL` cargado con `Acquire` y avanza `HEAD` con `Release`. El
  buffer es `static mut`, no esta detras de un `Mutex`, y por tanto
  puede tocarse directamente desde IRQ1.
- El decodificador es `pc_keyboard::Keyboard<layouts::Us104Key,
  ScancodeSet1>` envuelto en `lazy_static! Mutex<Keyboard<...>>` con
  `HandleControl::MapLettersToUnicode`.
- `process_scancode(scancode)` es el punto de entrada de IRQ1.
  Empuja caracteres `Unicode` crudos (incluido el chequeo de `'\x03'`
  para Ctrl+C que llama a `tm.exit_current_task()` para tareas con
  `id.0 > 1` e imprime `^C`), y traduce las teclas flecha crudas a
  secuencias ANSI de 3 bytes:
  - `ArrowUp`    → `\x1b[A`
  - `ArrowDown`  → `\x1b[B]
  - `ArrowRight` → `\x1b[C]`
  - `ArrowLeft`  → `\x1b[D]`
- `Return` empuja `'\n'` y `Backspace` empuja `'\x08'`.

---

## Filesystems

The filesystems layer lives under [`src/fs/`](src/fs/). The
top-level file [`src/fs/mod.rs`](src/fs/mod.rs) declares the
public submodules (`vfs`, `fd`, `manager`, `partition`, `fat`,
`ext4`) and exposes the `BlockDevice` trait, which is the only
abstraction drivers must implement to feed storage into the rest
of the kernel:

```rust
pub trait BlockDevice: Send + Sync {
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), &'static str>;
    fn write_block(&self, lba: u64, buf: &[u8])   -> Result<(), &'static str>;
}
```

`init_filesystem()` (called from `main.rs`) reads the first
`ModulesRequest` module, dereferences the two `u64` slots that
follow `*module` to obtain `tar_ptr` and `tar_size`, then invokes
`vfs::init(tar_ptr, tar_size)` and
`vfs::build_vfs_tree_from_tar()`. If the TAR is missing or
`ModulesRequest` was not honoured, an `[ERROR]` log line is
emitted and the VFS stays empty (boot does **not** panic on an
empty initramfs at this stage; `init_multitasking()` will panic if
`shell.elf` is later not found).

### File Descriptor Table

[`src/fs/fd.rs`](src/fs/fd.rs) is **just over 70 lines** today. It
defines:

- `enum FileDescriptor { Stdin, Stdout, Stderr, RegularFile
  { vnode: Arc<dyn VNode>, offset: usize } }`.
- `struct FdTable { descriptors: BTreeMap<usize, FileDescriptor>,
  next_fd: usize }`.
- `FdTable::new()` — preallocates the POSIX trinity (fd 0, 1, 2)
  and sets `next_fd = 3`.
- `FdTable::get_mut(fd)` — mut borrow of a single descriptor.
- `FdTable::insert(file)` — auto-allocates an fd, inserts it,
  bumps `next_fd`.
- `FdTable::close(fd)` — refuses to close `fd < 3` and returns
  `false` otherwise.

Syscalls interact with this table **directly through `task.fd_table`
methods**; the kernel does **not** currently expose any wrapper
called `read_fd` or `write_fd`. The `sys_read` and `sys_write`
handlers match on `task.fd_table.get_mut(fd)` and dispatch to
`vnode.read(...)` or `print!()`.

### VFS core

[`src/fs/vfs.rs`](src/fs/vfs.rs). The trait the whole FS layer
implements is:

```rust
pub trait VNode: Send + Sync {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
    fn get_size(&self) -> usize { 0 }
    fn is_dir(&self) -> bool { false }
    fn lookup(&self, _name: &str) -> Option<Arc<dyn VNode>> { None }
}
```

`TarContext { ptr: *const u8, size: usize }` is `unsafe Send +
Sync`. The kernel keeps three VFS globals:

```rust
pub static ref INITRAMFS: Mutex<Option<TarContext>> = Mutex::new(None);
pub static ref MOUNT_TABLE: MountTable = MountTable::new();
pub static ref VFS_ROOT: Arc<DirVNode> = Arc::new(DirVNode::new());
```

`vfs::init(tar_address, tar_size)` stores the TAR pointer in
`INITRAMFS`.

`find_file(filename: &str) -> Option<&'static [u8]>` walks the TAR,
reading 512-byte headers. It uses `usize::from_str_radix(size_str, 8)`
on bytes `124..135` for size parsing and returns a `&'static [u8]`
slice that points straight at the loaded image.

`TarVNode { data: &'static [u8] }` implements `VNode::read` /
`get_size` over that slice.

`DirVNode { children: Mutex<BTreeMap<String, Arc<dyn VNode>>> }`
implements `is_dir() = true` and `lookup() = children.lock()…`.

`MountTable` exposes `mount(path, node)`, `get_mount(path)` and
`unmount(path)`. It uses a `SpinMutex<BTreeMap<String, Arc<dyn
VNode>>>` so it is callable from IRQ context.

`resolve_path(path: &str) -> Option<Arc<dyn VNode>>` is the POSIX
resolver. It starts at the root returned by
`MOUNT_TABLE.get_mount("/")` (or `VFS_ROOT` if none), then walks
the path component by component. For each `/component` it first
checks `MOUNT_TABLE.get_mount(current_path)` (allowing a sub-mount
to override the directory at that exact path), and otherwise falls
back to `current.lookup(component)`.

`build_vfs_tree_from_tar()` parses the same TAR format as
`find_file`, sanitises filenames by removing any leading `/`, and
mounts every non-empty `TarVNode` directly under `VFS_ROOT`.

`open_vnode(path)` is a thin alias for `resolve_path(path)`.

### Block device manager

[`src/fs/manager.rs`](src/fs/manager.rs) hosts `process_disk(disk:
Arc<dyn BlockDevice>)`. The routine:

1. Calls `Mbr::read_from(disk.clone())`. If the magic `0x55 0xAA`
   is missing it logs and returns.
2. If `mbr.is_gpt_protective()` is true, logs `[VFS] -> Disco GPT
   detectado. Omitiendo MBR.` and stops.
3. Otherwise it iterates the four MBR entries. Empty slots are
   skipped.
4. `is_fat()` dispatches to `FatVolume::mount(disk, start_lba)`,
   prints `fat.debug_info()` and `fat.list_root_dir()`.
5. `is_linux()` dispatches to `Ext4Volume::mount(disk, start_lba)`,
   prints the volume info, then **exercises a POSIX round-trip
   test**: it allocates a fresh inode and block, writes
   `"¡Hola Mundo! Este archivo fue creado, mapeado y escrito 100%
   por NWIN OS."` into the data block, builds a file inode of the
   same length, persists the inode to the on-disk location, and
   re-links the file under the name `nwin_core.txt` in the
   directory. Immediately afterwards it opens the freshly created
   file via the VFS and reads it back, printing the recovered
   string.

### MBR partition table

[`src/fs/partition/mbr.rs`](src/fs/partition/mbr.rs). Constants:

```rust
pub const PART_TYPE_EMPTY:           u8 = 0x00;
pub const PART_TYPE_FAT16_CHS:       u8 = 0x06;
pub const PART_TYPE_FAT32_CHS:       u8 = 0x0B;
pub const PART_TYPE_FAT32_LBA:       u8 = 0x0C;
pub const PART_TYPE_FAT16:           u8 = 0x0E;
pub const PART_TYPE_LINUX:           u8 = 0x83;
pub const PART_TYPE_GPT_PROTECTIVE:  u8 = 0xEE;
```

> Note: in the source the GPT constant is duplicated on the same
> line by accident (`0xEE; // ...0xEE; // Crucial para ...`).
> The constant value is still what is used at compile time.

`PartitionEntry` is `#[repr(packed)]` with the layout:
`bootable: u8`, `start_chs: [u8; 3]`, `partition_type: u8`,
`end_chs: [u8; 3]`, `start_lba: u32`, `total_sectors: u32`.

Helpers:

- `is_empty()` — `partition_type == PART_TYPE_EMPTY`.
- `is_fat()` — type is one of the FAT16/FAT32 codes.
- `is_linux()` — type is `PART_TYPE_LINUX`.

`Mbr::read_from(disk)` reads a 512-byte sector, checks the magic,
then `copy_nonoverlapping`'s four 16-byte records at offset `446`
into `[PartitionEntry; 4]`.

`is_gpt_protective()` checks **only** `partitions[0]`:
`!partitions[0].is_empty() && partitions[0].partition_type ==
PART_TYPE_GPT_PROTECTIVE`.

### FAT16 / FAT32

[`src/fs/fat/bpb.rs`](src/fs/fat/bpb.rs) declares the packed
on-disk structures:

- `Fat16BootSector` (`bytes_per_sector`, `sectors_per_cluster`,
  `reserved_sector_count`, `table_count`, `root_entry_count`,
  `total_sectors_16`, `media_type`, `table_size_16`, then CHS and
  hidden counts, plus the FAT16 tail: `drive_number`,
  `reserved_1`, `boot_signature`, `volume_id`, `volume_label[11]`,
  `fat_type_label[8]`).
- `Fat32BootSector` reuses the FAT16 layout and appends
  `table_size_32`, `extended_flags`, `fat_version`, `root_cluster`,
  `fat_info`, `backup_bs_sector`, `reserved_0[12]`, then the
  FAT16 tail.
- `DirectoryEntry` is 32 bytes packed: `name[11]`, `attributes`,
  `reserved`, `creation_time_tenths`, `creation_time`,
  `creation_date`, `last_access_date`, `first_cluster_high`,
  `write_time`, `write_date`, `first_cluster_low`, `file_size`.
  Attribute bits observed in code: `0x01` read-only, `0x02`
  hidden, `0x04` system, `0x08` volume label, `0x10` directory,
  `0x0F` LFN entry. Cluster addresses combine
  `first_cluster_high` (high 16) and `first_cluster_low` (low 16)
  to address up to $2^{32}$ clusters.
- `enum FatType { Fat16(Fat16BootSector), Fat32(Fat32BootSector) }`.

[`src/fs/fat/volume.rs`](src/fs/fat/volume.rs):

- `FatVolume { device: Arc<dyn BlockDevice>, start_lba: u64, fat_type:
  FatType }`.
- `mount(device, start_lba)` reads the boot sector, verifies the
  `0x55 0xAA` magic and dispatches on `table_size_16`: when 0,
  the volume is FAT32 and the `root_cluster` plus `table_size_32`
  are exposed; otherwise the volume is FAT16 and `root_entry_count`
  / `table_size_16` apply.
- `root_dir_sector()`:
  - FAT16: `start_lba + reserved_sector_count + table_count *
    table_size_16`.
  - FAT32: `cluster_to_sector(root_cluster)` (computed against
    `first_data_sector`).
- `first_data_sector()`:
  - FAT16: `root_dir_sector() + ceil(root_entry_count * 32 /
    bytes_per_sector)`.
  - FAT32: `root_dir_sector()`.
- `cluster_to_sector(cluster)` is the canonical mapping:

```
sector = first_data_sector + (cluster as u64 - 2) * sectors_per_cluster
```

- `next_cluster(current_cluster)`:
  - FAT16: $\ge 0xFFF8$ ⇒ end of chain.
  - FAT32: read 4 bytes, mask off the high 4 bits (`& 0x0FFFFFFF`)
    and treat $\ge 0x0FFFFFF8$ as EOF.
  - Bounds are computed from `bytes_per_sector`, with the FAT
    table starting at `start_lba + reserved_sector_count`.
- `debug_info()` and `list_root_dir()` are used by the
  `process_disk` flow above.

[`src/fs/fat/mod.rs`](src/fs/fat/mod.rs) implements `FatNode` —
a `VNode` that holds an `Arc<FatVolume>`, a `start_cluster: u32`,
and the cached `is_directory` / `size`. `new_root` passes
`start_cluster = 0` for FAT16 and the volume's `root_cluster`
for FAT32. `lookup(name)` walks the directory sectors, skipping
entries whose first byte is `0x00` (end of directory) or `0xE5`
(deleted), entries with `attributes == 0x0F` (LFN fragments), and
volume-label entries (`attributes & 0x08`). It reassembles the
8.3 name and compares case-insensitively. `read(offset, buf)`
chains through clusters (using `volume.next_cluster`), reading
sector by sector until the requested byte count is satisfied or
the cluster chain ends.

### Ext4

[`src/fs/ext4/mod.rs`](src/fs/ext4/mod.rs). `Ext4Node` holds a
`volume: Arc<Ext4Volume>`, `inode_num`, an in-memory `inode`,
plus the cached `is_directory` / `size`. `new_root(volume)`
materialises the inode 2 root, which is the canonical ext4 root
in Linux. `add_entry(name, inode_num)` shrinks the last entry's
`rec_len` in a 4 KiB block and writes a new aligned entry in the
spare space.

`lookup(name)` reads the directory through `self.read(...)` and
walks `Ext4DirEntryHeader` records, skipping zero-length records
and matching names byte for byte.

`read(offset, buf)` is the I/O primitive; it delegates to the
extent-driven translator inside `Ext4Volume`.

The rest of the ext4 modules (`super_block`, `inode`, `extents`,
`block_group`, `dir_entry`) live in [`src/fs/ext4/`](src/fs/ext4/).
The full ext4 feature matrix (read + write, plain directory
entry insertion, POSIX round-trip test against an actual
`tests/disk-images/ext4_test.img` fixture) is exercised by the
`process_disk` path.

---

## Error Handling

[`src/core/error.rs`](src/core/error.rs) declares the kernel's
master `KernelError` enum plus four sub-enums:

```rust
pub enum KernelError {
    Memory(MemoryError),
    Privilege(PrivilegeError),
    Hardware(HardwareError),
    System(SystemError),
}

pub enum MemoryError {
    PageFault { vaddr: u64, is_user: bool, is_write: bool,
                is_instruction_fetch: bool },
    OutOfFrames,
    InvalidMapping,
}

pub enum PrivilegeError {
    GeneralProtectionFault { error_code: u64, is_user: bool },
    InvalidSyscall         { number: u64 },
}

pub enum HardwareError {
    DivideByZero   { is_user: bool },
    InvalidOpcode  { is_user: bool },
    MachineCheck,
}

pub enum SystemError {
    ElfParseFailed(crate::task::elf::ElfError),
    TaskCreationFailure,
}
```

Recoverable failures all return `Result<T, KernelError>`. The
`page_fault_handler` and `general_protection_fault_handler` raise
typed `KernelError::Memory::PageFault` and
`KernelError::Privilege::GeneralProtectionFault` values so the
panic message is structured. `TaskManager::spawn` /
`spawn_dynamic` convert stack-reservation failures into
`KernelError::System::TaskCreationFailure`. The ELF loader surfaces
its own `ElfError`, lifted into `KernelError::System::ElfParseFailed`
when forwarded out of `init_multitasking()`.

`src/core/panic.rs` holds the `#[panic_handler]`. On panic it:

1. `interrupts::disable()`.
2. `unsafe { crate::drivers::serial::SERIAL1.force_unlock() }`.
3. Prints a banner, the `PanicInfo` via `Debug`, and a
   `Sistema detenido (HALT).` footer — all on serial.
4. Enters `loop { x86_64::instructions::hlt() }`.

---

## Sistemas de Archivos

La capa de sistemas de archivos vive en [`src/fs/`](src/fs/).
El archivo superior [`src/fs/mod.rs`](src/fs/mod.rs) declara los
submodulos publicos (`vfs`, `fd`, `manager`, `partition`, `fat`,
`ext4`) y expone el trait `BlockDevice`, la unica abstraccion
que los drivers deben implementar para alimentar al resto del
kernel:

```rust
pub trait BlockDevice: Send + Sync {
    fn read_block(&self, lba: u64, buffer: &mut [u8]) -> Result<(), &'static str>;
    fn write_block(&self, lba: u64, buf: &[u8])   -> Result<(), &'static str>;
}
```

`init_filesystem()` (llamado desde `main.rs`) lee el primer modulo
de `ModulesRequest`, desreferencia los dos `u64` que siguen a
`*module` para obtener `tar_ptr` y `tar_size`, y luego invoca
`vfs::init(tar_ptr, tar_size)` y `vfs::build_vfs_tree_from_tar()`.
Si el TAR no esta o `ModulesRequest` no fue atendido, se emite
una linea `[ERROR]` y el VFS queda vacio (el arranque **no**
entra en panico en esta fase; `init_multitasking()` si lo hara si
mas tarde no encuentra `shell.elf`).

### Tabla de descriptores de archivo

[`src/fs/fd.rs`](src/fs/fd.rs) tiene **poco mas de 70 lineas** a
dia de hoy. Define:

- `enum FileDescriptor { Stdin, Stdout, Stderr, RegularFile
  { vnode: Arc<dyn VNode>, offset: usize } }`.
- `struct FdTable { descriptors: BTreeMap<usize, FileDescriptor>,
  next_fd: usize }`.
- `FdTable::new()` — preasigna la trinidad POSIX (fd 0, 1, 2) y
  pone `next_fd = 3`.
- `FdTable::get_mut(fd)` — prestamo mutable de un descriptor.
- `FdTable::insert(file)` — autoasigna fd, inserta, incrementa
  `next_fd`.
- `FdTable::close(fd)` — rechaza cerrar `fd < 3` y devuelve
  `false` en caso contrario.

Las syscalls interactuan con esta tabla **directamente mediante
los metodos de `task.fd_table`**; el kernel **no** expone hoy
ningun envoltorio llamado `read_fd` ni `write_fd`. Los handlers
de `sys_read` y `sys_write` hacen match sobre
`task.fd_table.get_mut(fd)` y dispatchan a `vnode.read(...)` o
`print!()`.

### Nucleo VFS

[`src/fs/vfs.rs`](src/fs/vfs.rs). El trait que implementa toda
la capa FS es:

```rust
pub trait VNode: Send + Sync {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
    fn get_size(&self) -> usize { 0 }
    fn is_dir(&self) -> bool { false }
    fn lookup(&self, _name: &str) -> Option<Arc<dyn VNode>> { None }
}
```

`TarContext { ptr: *const u8, size: usize }` es `unsafe Send +
Sync`. El kernel mantiene tres globales VFS:

```rust
pub static ref INITRAMFS: Mutex<Option<TarContext>> = Mutex::new(None);
pub static ref MOUNT_TABLE: MountTable = MountTable::new();
pub static ref VFS_ROOT: Arc<DirVNode> = Arc::new(DirVNode::new());
```

`vfs::init(tar_address, tar_size)` guarda el puntero del TAR en
`INITRAMFS`.

`find_file(filename: &str) -> Option<&'static [u8]>` recorre el
TAR leyendo cabeceras de 512 bytes. Usa
`usize::from_str_radix(size_str, 8)` sobre los bytes `124..135`
para parsear el tamaño y devuelve un slice `&'static [u8]` que
apunta directamente a la imagen cargada.

`TarVNode { data: &'static [u8] }` implementa `VNode::read` /
`get_size` sobre ese slice.

`DirVNode { children: Mutex<BTreeMap<String, Arc<dyn VNode>>> }`
implementa `is_dir() = true` y
`lookup() = children.lock()…`.

`MountTable` expone `mount(path, node)`, `get_mount(path)` y
`unmount(path)`. Internamente usa un `SpinMutex<BTreeMap<String,
Arc<dyn VNode>>>` por lo que es invocable desde contexto IRQ.

`resolve_path(path: &str) -> Option<Arc<dyn VNode>>` es el
resolvedor POSIX. Arranca en la raiz devuelta por
`MOUNT_TABLE.get_mount("/")` (o `VFS_ROOT` si no hay), y luego
camina el path componente a componente. Para cada `/component`
primero consulta `MOUNT_TABLE.get_mount(current_path)` (permitiendo
que un sub-mount sobreescriba el directorio en esa ruta exacta),
y en caso contrario cae al `current.lookup(component)` habitual.

`build_vfs_tree_from_tar()` parsea el mismo formato TAR que
`find_file`, sanea los nombres eliminando cualquier `/` inicial,
y monta cada `TarVNode` no vacio directamente bajo `VFS_ROOT`.

`open_vnode(path)` es un alias de `resolve_path(path)`.

### Gestor de dispositivos de bloque

[`src/fs/manager.rs`](src/fs/manager.rs) aloja
`process_disk(disk: Arc<dyn BlockDevice>)`. La rutina:

1. Llama a `Mbr::read_from(disk.clone())`. Si falta la magia
   `0x55 0xAA`, registra y vuelve.
2. Si `mbr.is_gpt_protective()` es true, registra
   `[VFS] -> Disco GPT detectado. Omitiendo MBR.` y termina.
3. En caso contrario itera las cuatro entradas MBR. Las entradas
   vacias se saltan.
4. `is_fat()` despacha a `FatVolume::mount(disk, start_lba)`,
   imprime `fat.debug_info()` y `fat.list_root_dir()`.
5. `is_linux()` despacha a `Ext4Volume::mount(disk, start_lba)`,
   imprime la info del volumen y luego **ejecuta una prueba
   POSIX round-trip**: reserva un inodo y un bloque nuevos,
   escribe
   `"¡Hola Mundo! Este archivo fue creado, mapeado y escrito 100%
   por NWIN OS."` en el bloque de datos, construye un inodo de
   archivo con esa longitud, persiste el inodo en disco y vuelve
   a enlazar el archivo bajo el nombre `nwin_core.txt` en el
   directorio. Inmediatamente despues lo abre a traves del VFS y
   lo relee, imprimiendo la cadena recuperada.

### Tabla de particiones MBR

[`src/fs/partition/mbr.rs`](src/fs/partition/mbr.rs).
Constantes:

```rust
pub const PART_TYPE_EMPTY:           u8 = 0x00;
pub const PART_TYPE_FAT16_CHS:       u8 = 0x06;
pub const PART_TYPE_FAT32_CHS:       u8 = 0x0B;
pub const PART_TYPE_FAT32_LBA:       u8 = 0x0C;
pub const PART_TYPE_FAT16:           u8 = 0x0E;
pub const PART_TYPE_LINUX:           u8 = 0x83;
pub const PART_TYPE_GPT_PROTECTIVE:  u8 = 0xEE;
```

> Nota: en el codigo la constante GPT esta duplicada en la misma
> linea por un descuido (`0xEE; // ...0xEE; // Crucial para
> ...`). El valor que se usa en tiempo de compilacion es el
> mismo.

`PartitionEntry` es `#[repr(packed)]` con el layout:
`bootable: u8`, `start_chs: [u8; 3]`, `partition_type: u8`,
`end_chs: [u8; 3]`, `start_lba: u32`, `total_sectors: u32`.

Helpers:

- `is_empty()` — `partition_type == PART_TYPE_EMPTY`.
- `is_fat()` — el tipo es uno de los codigos FAT16/FAT32.
- `is_linux()` — el tipo es `PART_TYPE_LINUX`.

`Mbr::read_from(disk)` lee un sector de 512 bytes, comprueba la
magia y luego con `copy_nonoverlapping` copia los cuatro registros
de 16 bytes en el offset `446` a `[PartitionEntry; 4]`.

`is_gpt_protective()` revisa **solo** `partitions[0]`:
`!partitions[0].is_empty() && partitions[0].partition_type ==
PART_TYPE_GPT_PROTECTIVE`.

### FAT16 / FAT32

[`src/fs/fat/bpb.rs`](src/fs/fat/bpb.rs) declara las estructuras
packed en disco:

- `Fat16BootSector` (`bytes_per_sector`, `sectors_per_cluster`,
  `reserved_sector_count`, `table_count`, `root_entry_count`,
  `total_sectors_16`, `media_type`, `table_size_16`, luego CHS y
  conteos ocultos, mas la cola FAT16: `drive_number`,
  `reserved_1`, `boot_signature`, `volume_id`, `volume_label[11]`,
  `fat_type_label[8]`).
- `Fat32BootSector` reutiliza el layout FAT16 y le añade
  `table_size_32`, `extended_flags`, `fat_version`,
  `root_cluster`, `fat_info`, `backup_bs_sector`,
  `reserved_0[12]`, y luego la cola FAT16.
- `DirectoryEntry` son 32 bytes packed: `name[11]`, `attributes`,
  `reserved`, `creation_time_tenths`, `creation_time`,
  `creation_date`, `last_access_date`, `first_cluster_high`,
  `write_time`, `write_date`, `first_cluster_low`, `file_size`.
  Bits de atributo observados en codigo: `0x01` read-only,
  `0x02` hidden, `0x04` system, `0x08` volumen, `0x10`
  directorio, `0x0F` entrada LFN. Las direcciones de cluster
  combinan `first_cluster_high` (16 altos) y
  `first_cluster_low` (16 bajos) hasta $2^{32}$ clusters.
- `enum FatType { Fat16(Fat16BootSector), Fat32(Fat32BootSector) }`.

[`src/fs/fat/volume.rs`](src/fs/fat/volume.rs):

- `FatVolume { device: Arc<dyn BlockDevice>, start_lba: u64,
  fat_type: FatType }`.
- `mount(device, start_lba)` lee el sector de arranque, verifica
  la magia `0x55 0xAA` y despacha segun `table_size_16`: cuando
  es 0, el volumen es FAT32 y expone `root_cluster` y
  `table_size_32`; de lo contrario es FAT16 y aplican
  `root_entry_count` / `table_size_16`.
- `root_dir_sector()`:
  - FAT16: `start_lba + reserved_sector_count + table_count *
    table_size_16`.
  - FAT32: `cluster_to_sector(root_cluster)`.
- `first_data_sector()`:
  - FAT16: `root_dir_sector() + ceil(root_entry_count * 32 /
    bytes_per_sector)`.
  - FAT32: `root_dir_sector()`.
- `cluster_to_sector(cluster)` es el mapeo canonico:

```
sector = first_data_sector + (cluster as u64 - 2) * sectors_per_cluster
```

- `next_cluster(current_cluster)`:
  - FAT16: $\ge 0xFFF8$ ⇒ fin de cadena.
  - FAT32: lee 4 bytes, enmascara los 4 bits altos
    (`& 0x0FFFFFFF`) y trata $\ge 0x0FFFFFF8$ como EOF.
  - Los limites se calculan desde `bytes_per_sector`, con la
    tabla FAT empezando en `start_lba + reserved_sector_count`.
- `debug_info()` y `list_root_dir()` se usan por el flujo de
  `process_disk` descrito arriba.

[`src/fs/fat/mod.rs`](src/fs/fat/mod.rs) implementa `FatNode` —
un `VNode` que tiene `Arc<FatVolume>`, un `start_cluster: u32` y
los `is_directory` / `size` cacheados. `new_root` pasa
`start_cluster = 0` para FAT16 y el `root_cluster` del volumen
para FAT32. `lookup(name)` recorre los sectores del directorio,
saltando entradas cuyo primer byte sea `0x00` (fin de directorio)
o `0xE5` (borradas), entradas con `attributes == 0x0F`
(fragmentos LFN) y entradas de etiqueta de volumen
(`attributes & 0x08`). Reensambla el nombre 8.3 y compara sin
distinguir mayusculas/minusculas. `read(offset, buf)` encadena
clusters (usando `volume.next_cluster`), leyendo sector a sector
hasta completar el conteo solicitado o terminar la cadena.

### Ext4

[`src/fs/ext4/mod.rs`](src/fs/ext4/mod.rs). `Ext4Node` almacena
`volume: Arc<Ext4Volume>`, `inode_num`, un `inode` en memoria y
los `is_directory` / `size` cacheados. `new_root(volume)`
materializa la raiz del inodo 2, que es la raiz canonica de ext4
en Linux. `add_entry(name, inode_num)` encoge el `rec_len` de la
ultima entrada en un bloque de 4 KiB y escribe una nueva entrada
alineada en el espacio sobrante.

`lookup(name)` lee el directorio mediante `self.read(...)` y
recorre los registros `Ext4DirEntryHeader`, saltando los registros
de longitud cero y comparando nombres byte a byte.

`read(offset, buf)` es la primitiva de E/S; delega al traductor
basado en extents dentro de `Ext4Volume`.

Los demas modulos ext4 (`super_block`, `inode`, `extents`,
`block_group`, `dir_entry`) viven en [`src/fs/ext4/`](src/fs/ext4/).
La matriz completa de funcionalidades ext4 (lectura + escritura,
insercion de entradas de directorio, prueba round-trip POSIX
contra la fixture real `tests/disk-images/ext4_test.img`) se
ejercita por la ruta `process_disk`.

---

## Manejo de Errores

[`src/core/error.rs`](src/core/error.rs) declara el enum maestro
`KernelError` del kernel y cuatro sub-enums:

```rust
pub enum KernelError {
    Memory(MemoryError),
    Privilege(PrivilegeError),
    Hardware(HardwareError),
    System(SystemError),
}

pub enum MemoryError {
    PageFault { vaddr: u64, is_user: bool, is_write: bool,
                is_instruction_fetch: bool },
    OutOfFrames,
    InvalidMapping,
}

pub enum PrivilegeError {
    GeneralProtectionFault { error_code: u64, is_user: bool },
    InvalidSyscall         { number: u64 },
}

pub enum HardwareError {
    DivideByZero   { is_user: bool },
    InvalidOpcode  { is_user: bool },
    MachineCheck,
}

pub enum SystemError {
    ElfParseFailed(crate::task::elf::ElfError),
    TaskCreationFailure,
}
```

Los fallos recuperables devuelven todos `Result<T, KernelError>`.
Los handlers `page_fault_handler` y
`general_protection_fault_handler` construyen valores tipados
`KernelError::Memory::PageFault` y
`KernelError::Privilege::GeneralProtectionFault` para que el
mensaje de panico sea estructurado. `TaskManager::spawn` /
`spawn_dynamic` convierten los fallos de reserva de pila en
`KernelError::System::TaskCreationFailure`. El cargador ELF expone
su propio `ElfError`, elevado a `KernelError::System::ElfParseFailed`
cuando se reenvia desde `init_multitasking()`.

`src/core/panic.rs` contiene el `#[panic_handler]`. Ante un
panico:

1. `interrupts::disable()`.
2. `unsafe { crate::drivers::serial::SERIAL1.force_unlock() }`.
3. Imprime un banner, el `PanicInfo` via `Debug` y un footer
   `Sistema detenido (HALT).` — todo por puerto serie.
4. Entra en `loop { x86_64::instructions::hlt() }`.

---

## Architecture Overview

The boot order in `_start` (`src/main.rs`) traces the layered
design end to end:

1. `serial_println!(">>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN
   VFS-FAT32) <<<")` over COM1.
2. `assert!(BASE_REVISION.is_supported())` — Limine protocol
   compatibility gate.
3. `drivers::display::init()` — framebuffer + `WRITER` global.
4. `core::cpu::init()` — SSE/SSE2 + Local APIC off (legacy PIC).
5. `core::gdt::init()` — GDT / TSS / IST stack / user selectors.
6. `core::idt::init()` — IDT, chained PIC at offsets 32/40,
   PIT Channel 0 mask early-divisor primed.
7. `core::syscall::init()` — EFER + STAR + LSTAR + SFMask.
8. `mm::init()` — bitmap allocator init → new PML4 with HHDM
   preserved → kernel heap (`0x_4444_4444_0000`, 1 MiB).
9. `drivers::pci::init() -> Option<u32>` — returns AHCI BAR5 to
   `_ahci_base`, which forwards it to `block::ahci::init(bar5)`.
10. `fs::init_filesystem()` — pulls the initramfs TAR off
    `MODULES_REQUEST` and parses it into `VFS_ROOT`.
11. `task::init_multitasking()` — spawns `hilo_segador` (Ring 0
    Reaper), then spawns `shell.elf` (Ring 3) into an isolated PML4.
12. `drivers::pit::init()` — PIT Channel 0 at 100 Hz (`divisor
    = 11931`).
13. `loop { x86_64::instructions::interrupts::enable_and_hlt() }`
    — the boot thread sleeps on the next IRQ.

Boot-thread wake-ups land on the `timer_interrupt_handler` (IRQ 0)
which fires `schedule()`. The scheduler updates `tss.rsp0` for
Ring 3 transitions, swaps `Cr3` if the next task's PML4 differs,
installs `KERNEL_RSP` for the syscall trampoline, and calls
`context_switch`. Ring 3 → kernel crossings go through `syscall`
on user code path, or `interrupt` on the IRQ path; both end up
running on whichever kernel stack the scheduler wired into
`tss.privilege_stack_table[0]`.

Ring 3 exceptions (`#DE`, `#GP`, `#UD`, `#PF`) terminate the
faulting task via `TaskManager::exit_current_task` — the Reaper
later calls `destroy_user_address_space` to release its user
half of the PML4 and all data frames.

```
+--------------------------------------------------------------+
|                  Limine / UEFI Bootloader                    |
+--------------------------------------------------------------+
                          |
                          v
+--------------------------------------------------------------+
|                    Kernel (Ring 0)                           |
|                                                              |
|  +-----------------------+       +-------------------------+ |
|  |  core/                |       |  mm/                    | |
|  |   cpu  gdt  idt       |<----->|   bitmap + CoW + heap   | |
|  |   syscall  panic      |       |   per-task PML4         | |
|  |   error  sync         |       +-------------------------+ |
|  +-----------^-----------+                       ^           |
|              |                               |               |
|  +-----------+-----------+      +--------------+--------+   |
|  |   task/                |      |   fs/                    |  |
|  |   scheduler  reaper    |      |   vfs  fd  manager       |  |
|  |   task_manager  elf    |      |   mbr  fat  ext4         |  |
|  |   usermode             |      |   BlockDevice trait      |  |
|  +-----------^-----------+      +--------------^---------+   |
|              |                              |                  |
|  +-----------+------------------------------+--------+         |
|  |   drivers/                                       |         |
|  |   serial  pit  pci  ahci                         |         |
|  |   framebuffer + TTY  keyboard                    |         |
|  +--------------------------------------------------+         |
+--------------------------------------------------------------+
                          |
                          v
+--------------------------------------------------------------+
|                Userspace (Ring 3) - ELF binaries            |
|        shell.elf, test_mem.elf, future programs             |
+--------------------------------------------------------------+
```

---

## Known Limitations

These are gaps that exist in the code today. The roadmap section
below explains how each one is being addressed.

- **TTY scrolling.** `Writer::new_line` wraps to the top of the
  screen and calls `clear_screen` instead of performing a true
  scroll-up copy. A real scroll with double-buffering is on the
  roadmap (Block 2.5).
- **TTY selective erasing.** `K` only implements mode `0` (erase
  to end of line). Modes `1` (to start) and `2` (whole line) are
  not wired.
- **ANSI colour reset.** `SGR 39` / `49` (reset fg / bg to
  default) fall through to the default match arm with no special
  handling — `SGR 0` does the right thing, but the two related
  codes are not honoured explicitly.
- **`sys_fork` (57) and `sys_getpid` (39)** are listed in the
  roadmap but are not yet in the `match` of `handle_syscall_rust`.
- **`PT_DYNAMIC`** is not parsed by the ELF loader. Static ELF
  binaries work; dynamically linked binaries do not.
- **GPT partitions.** The MBR parser detects GPT-protective and
  skips the partition walk with the message `"[VFS] -> Disco GPT
  detectado. Omitiendo MBR."`. There is no full GPT header
  parser yet (roadmap Block 2.4).
- **MBR GPT detection scope.** `Mbr::is_gpt_protective` inspects
  only `partitions[0]`. This matches what UEFI demands for a
  protective MBR, but it is worth flagging.
- **Source-file bug.** `src/fs/partition/mbr.rs` line 11 contains
  two `pub const PART_TYPE_GPT_PROTECTIVE` declarations on the
  same line by mistake. Only the first takes effect because the
  second re-declaration is in the same source line; the constant
  itself is correct, but the file would fail to compile as-is if
  future edits turn the second declaration into a separate
  line.
- **Acpi shutdown coverage.** The `syscall 88` power-off path
  fires both `0x604 0x2000` (ACPI) and `0xf4 0x10` (QEMU
  `isa-debug-exit`). On hardware that neither supports, the
  kernel will loop on `enable_and_hlt` indefinitely. A real
  ACPI parser / FADT is on the roadmap (Block 2.3 and Block 3.7).
- **Single-core only.** The scheduler is single-core. SMP work
  is on the roadmap (Block 2.2).
- **Local APIC disabled.** The legacy 8259 PIC is the sole
  interrupt source. Re-enabling the Local APIC is required
  before SMP.

---

## Roadmap / Future Work

These items are the active engineering targets. Each block lists
what the source currently lacks (referencing the Known Limitations
above) so the project can be tracked empirically.

### Block 1 - Almost Done (>= 80%)

- 1.1 **Native `fork` syscall** (57). Wire it into the
  `handle_syscall_rust` match; clone the parent's PML4 with
  CoW ref-counts copied via `cow_reference_frame`; return `0` to
  the child, the child PID to the parent.
- 1.2 **Robust `free_block` / `free_inode`** in Ext4, including
  multi-group bookkeeping and `s_free_blocks_count_lo` refresh.
- 1.3 **Minimal `PT_DYNAMIC` support** in the ELF loader.
- 1.4 **`futex`** (syscall 202) on top of the existing semaphore
  queue (`src/core/sync.rs`), required for musl/glibc POSIX
  threads.

### Block 2 - In Progress (30 - 70%)

- 2.1 **Scheduler priorities and affinity** (Linux nice, 0-139).
- 2.2 **SMP** — re-enable Local APIC, parse ACPI MADT,
  trampoline for application processors, per-CPU state.
- 2.3 **ACPI parser** — RSDP, RSDT/XSDT, MADT, MCFG, HPET.
- 2.4 **GPT partition parser** alongside the existing MBR path.
- 2.5 **TTY completeness** — save/restore cursor, true scroll
  regions, double buffering, SGR `39`/`49` reset, `K` modes 1/2.
- 2.6 **NVMe driver** — Admin queues, Identify, I/O queues with
  PRP.
- 2.7 **FAT write support** and LFN (Long File Names).
- 2.8 **POSIX signals** — `rt_sigaction`, `kill`,
  `rt_sigreturn`.
- 2.9 **Pipes and `dup`/`dup2`/`fcntl`** for shell plumbing.
- 2.10 **HPET timer** as the primary clock, with PIT fallback.

### Block 3 - To Start (0%)

- 3.1 **LinuxKPI shim** under `compat/lkpi/` compiled via
  `build.rs` + the `cc` crate.
- 3.2 **Formal IPC** — dedicated syscall range for message
  passing.
- 3.3 **LKL server** — Ring 3 host that runs Linux-driver
  blobs.
- 3.4 **Networking** — `e1000` driver, IPv4, UDP, TCP, BSD
  sockets.
- 3.5 **GPU/DRM/KMS** — VGA text mode, Bochs stdvga, Intel
  i915.
- 3.6 **Full dynamic ELF** + `ld-linux` and relocations
  (`R_X86_64_RELATIVE`, `R_X86_64_64`, `R_X86_64_JUMP_SLOT`,
  ...).
- 3.7 **Full ACPI** — FADT (real shutdown), SRAT/SLIT for NUMA,
  WAET.
- 3.8 **USB stack (XHCI)** — host controller, HID, storage,
  hub.
- 3.9 **Functional shell** — builtins (`ls`, `cat`, `cd`, ...),
  pipes, redirects, globbing, history.
- 3.10 **C build integration** — `build.rs` + `cc` crate to
  compile C fragments linked into the kernel.

### Definition of Done

Each block ships with an empirical test:

- `fork`: cargo test using a Ring 3 binary that calls
  `fork + execve` and prints both PIDs.
- `futex`: a two-thread `pthread_mutex_lock` test that does
  not wedge.
- SMP: `nproc`-style introspection reports the physical core
  count parsed from MADT.
- ACPI: `dmesg`-style serial log shows the parsed RSDP /
  RSDT / MADT contents.
- GPT: a GPT disk mounts at `/`.
- TTY: `vim` renders without artefacts.
- NVMe: an NVMe SSD enumerates as `/dev/nvme0n1`.
- LinuxKPI: `e1000.ko` loads; `ifconfig` shows the interface.
- LKL: a vendor driver blob runs without kernel panic.
- Shell: `ls -la /boot` works interactively.

---

## Vision de Arquitectura

El orden de arranque en `_start` (`src/main.rs`) recorre el diseño
por capas de extremo a extremo:

1. `serial_println!(">>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN
   VFS-FAT32) <<<")` sobre COM1.
2. `assert!(BASE_REVISION.is_supported())` — puerta de
   compatibilidad del protocolo Limine.
3. `drivers::display::init()` — framebuffer + `WRITER` global.
4. `core::cpu::init()` — SSE/SSE2 + Local APIC off (PIC legado).
5. `core::gdt::init()` — GDT / TSS / pila IST / selectores de
   usuario.
6. `core::idt::init()` — IDT, PIC en cadena con offsets 32/40,
   cebado temprano del divisor del PIT Canal 0.
7. `core::syscall::init()` — EFER + STAR + LSTAR + SFMask.
8. `mm::init()` — init del bitmap → nueva PML4 preservando el
   HHDM → heap del kernel (`0x_4444_4444_0000`, 1 MiB).
9. `drivers::pci::init() -> Option<u32>` — devuelve el BAR5 de
   AHCI a `_ahci_base`, que lo reenvia a
   `block::ahci::init(bar5)`.
10. `fs::init_filesystem()` — extrae el TAR del initramfs del
    `MODULES_REQUEST` y lo parsea al `VFS_ROOT`.
11. `task::init_multitasking()` — crea `hilo_segador` (Reaper
    Ring 0) y luego `shell.elf` (Ring 3) en una PML4 aislada.
12. `drivers::pit::init()` — PIT Canal 0 a 100 Hz (`divisor =
    11931`).
13. `loop { x86_64::instructions::interrupts::enable_and_hlt() }`
    — el hilo de arranque duerme hasta el siguiente IRQ.

Los despertares del hilo de arranque caen en
`timer_interrupt_handler` (IRQ 0) que dispara `schedule()`. El
planificador actualiza `tss.rsp0` para transiciones a Ring 3,
intercambia `Cr3` si la PML4 de la siguiente tarea es distinta,
instala `KERNEL_RSP` para el trampolin de syscall y llama a
`context_switch`. Los cruces Ring 3 → kernel van por `syscall` en
el camino de codigo de usuario, o por `interrupt` en el camino
IRQ; ambos terminan ejecutando en la pila de kernel que el
planificador cableo en `tss.privilege_stack_table[0]`.

Las excepciones de Ring 3 (`#DE`, `#GP`, `#UD`, `#PF`) terminan la
tarea fallida con `TaskManager::exit_current_task` — el Reaper
invoca despues `destroy_user_address_space` para liberar la mitad
user de la PML4 y todos sus marcos de datos.

```
+--------------------------------------------------------------+
|                    Limine / UEFI Bootloader                  |
+--------------------------------------------------------------+
                          |
                          v
+--------------------------------------------------------------+
|                     Kernel (Ring 0)                          |
|                                                              |
|  +-----------------------+       +-------------------------+ |
|  |  core/                |       |  mm/                    | |
|  |   cpu  gdt  idt       |<----->|   bitmap + CoW + heap   | |
|  |   syscall  panic      |       |   PML4 por tarea        | |
|  |   error  sync         |       +-------------------------+ |
|  +-----------^-----------+                       ^           |
|              |                               |               |
|  +-----------+-----------+      +--------------+--------+   |
|  |   task/                |      |   fs/                    |  |
|  |   scheduler  reaper    |      |   vfs  fd  manager       |  |
|  |   task_manager  elf    |      |   mbr  fat  ext4         |  |
|  |   usermode             |      |   trait BlockDevice      |  |
|  +-----------^-----------+      +--------------^---------+   |
|              |                              |                  |
|  +-----------+------------------------------+--------+         |
|  |   drivers/                                       |         |
|  |   serial  pit  pci  ahci                         |         |
|  |   framebuffer + TTY  keyboard                    |         |
|  +--------------------------------------------------+         |
+--------------------------------------------------------------+
                          |
                          v
+--------------------------------------------------------------+
|               Espacio de usuario (Ring 3) - ELF              |
|          shell.elf, test_mem.elf, programas futuros         |
+--------------------------------------------------------------+
```

---

## Limitaciones Conocidas

Estas son carencias presentes hoy en el codigo. La seccion de
hoja de ruta siguiente explica como se esta abordando cada una.

- **Scroll del TTY.** `Writer::new_line` salta al tope de la
  pantalla y llama a `clear_screen` en lugar de hacer un verdadero
  scroll-up por copia. Un scroll real con doble buffering esta en
  la hoja de ruta (Bloque 2.5).
- **Borrado selectivo del TTY.** `K` solo implementa el modo `0`
  (borrar hasta fin de linea). Los modos `1` (al inicio) y `2`
  (linea completa) no estan cableados.
- **Reset ANSI de color.** `SGR 39` / `49` (reset fg / bg a
  defecto) caen al brazo `_ => {}` sin manejo especial — `SGR 0`
  funciona, pero los dos codigos relacionados no se honran
  explicitamente.
- **`sys_fork` (57) y `sys_getpid` (39)** figuran en la hoja de
  ruta pero aun no estan en el `match` de `handle_syscall_rust`.
- **`PT_DYNAMIC`** no lo parsea el cargador ELF. Los binarios
  ELF estaticos funcionan; los dinamicos no.
- **Particiones GPT.** El parser MBR detecta GPT-protective y se
  salta la iteracion con el mensaje
  `"[VFS] -> Disco GPT detectado. Omitiendo MBR."`. Aun no hay
  parser completo de cabecera GPT (hoja de ruta Bloque 2.4).
- **Alcance de la deteccion GPT en MBR.** `Mbr::is_gpt_protective`
  inspecciona solo `partitions[0]`. Esto coincide con lo que UEFI
  exige para un MBR protectivo, pero conviene senalarlo.
- **Bug en codigo fuente.** `src/fs/partition/mbr.rs` linea 11
  contiene por error dos declaraciones
  `pub const PART_TYPE_GPT_PROTECTIVE` en la misma linea. Solo la
  primera toma efecto porque la segunda esta en la misma linea
  fuente; el valor de la constante es correcto, pero el archivo
  fallara al compilar si ediciones futuras convierten la segunda
  declaracion en una linea aparte.
- **Cobertura de apagado ACPI.** El camino de apagado de
  `syscall 88` dispara tanto `0x604 0x2000` (ACPI) como
  `0xf4 0x10` (`isa-debug-exit` de QEMU). En hardware que no
  soporte ninguno, el kernel quedara en bucle
  `enable_and_hlt` indefinidamente. Un parser ACPI / FADT real
  esta en la hoja de ruta (Bloques 2.3 y 3.7).
- **Solo un nucleo.** El planificador es mono-nucleo. El trabajo
  de SMP esta en la hoja de ruta (Bloque 2.2).
- **Local APIC deshabilitado.** El 8259 PIC legado es la unica
  fuente de interrupciones. Reactivar el Local APIC es requisito
  previo a SMP.

---

## Hoja de Ruta / Trabajo Futuro

Estos son los objetivos activos de ingenieria. Cada bloque lista
lo que al codigo actual le falta (referenciando las Limitaciones
Conocidas de arriba) para que el proyecto se pueda seguir de
forma empirica.

### Bloque 1 - Casi terminado (>= 80%)

- 1.1 **`fork` nativo** (syscall 57). Cablearlo al match de
  `handle_syscall_rust`; clonar la PML4 del padre con ref-counters
  CoW copiados via `cow_reference_frame`; devolver `0` al hijo y
  el PID hijo al padre.
- 1.2 **`free_block` / `free_inode` robustos** en Ext4, incluida
  la contabilidad multi-grupo y el refresco de
  `s_free_blocks_count_lo`.
- 1.3 **Soporte minimo de `PT_DYNAMIC`** en el cargador ELF.
- 1.4 **`futex`** (syscall 202) sobre la cola de semaforos
  existente (`src/core/sync.rs`), requerido para los hilos POSIX
  de musl/glibc.

### Bloque 2 - En curso (30 - 70%)

- 2.1 **Planificador con prioridades y afinidad** (Linux nice,
  0-139).
- 2.2 **SMP** — reactivar Local APIC, parsear ACPI MADT,
  trampolin para application processors, estado por CPU.
- 2.3 **Parser ACPI** — RSDP, RSDT/XSDT, MADT, MCFG, HPET.
- 2.4 **Parser GPT** junto a la ruta MBR existente.
- 2.5 **TTY completo** — save/restore del cursor, regiones de
  scroll reales, doble buffering, reset SGR `39`/`49`, modos
  `K` 1/2.
- 2.6 **Driver NVMe** — colas Admin, Identify, colas I/O con
  PRP.
- 2.7 **Soporte de escritura FAT** y LFN.
- 2.8 **Senales POSIX** — `rt_sigaction`, `kill`,
  `rt_sigreturn`.
- 2.9 **Pipes y `dup`/`dup2`/`fcntl`** para plumbing de shell.
- 2.10 **HPET** como reloj primario, con PIT como fallback.

### Bloque 3 - Por iniciar (0%)

- 3.1 **Shim LinuxKPI** bajo `compat/lkpi/` compilado via
  `build.rs` + crate `cc`.
- 3.2 **IPC formal** — rango propio de syscalls para message
  passing.
- 3.3 **Servidor LKL** — host Ring 3 que corre blobs de drivers
  Linux.
- 3.4 **Networking** — driver `e1000`, IPv4, UDP, TCP, sockets
  BSD.
- 3.5 **GPU/DRM/KMS** — modo texto VGA, Bochs stdvga, Intel
  i915.
- 3.6 **ELF dinamico completo** + `ld-linux` + reubicaciones
  (`R_X86_64_RELATIVE`, `R_X86_64_64`, `R_X86_64_JUMP_SLOT`,
  ...).
- 3.7 **ACPI completo** — FADT (apagado real), SRAT/SLIT para
  NUMA, WAET.
- 3.8 **Stack USB (XHCI)** — host controller, HID, storage, hub.
- 3.9 **Shell funcional** — builtins (`ls`, `cat`, `cd`, ...),
  pipes, redirecciones, globbing, historial.
- 3.10 **Integracion de build en C** — `build.rs` + crate `cc`
  para compilar fragmentos en C linkeados al kernel.

### Criterio de "Terminado"

Cada bloque se entrega con una prueba empirica:

- `fork`: cargo test con un binario Ring 3 que llame
  `fork + execve` e imprima ambos PIDs.
- `futex`: test de `pthread_mutex_lock` entre dos hilos sin
  quedarse colgado.
- SMP: introspeccion estilo `nproc` que reporta el conteo fisico
  de nucleos parseado del MADT.
- ACPI: log estilo `dmesg` que muestra los contenidos parseados
  de RSDP / RSDT / MADT.
- GPT: disco GPT montado en `/`.
- TTY: `vim` renderiza sin artefactos.
- NVMe: SSD NVMe enumerado como `/dev/nvme0n1`.
- LinuxKPI: `e1000.ko` carga; `ifconfig` muestra la interfaz.
- LKL: un blob de driver de un fabricante corre sin Kernel
  Panic.
- Shell: `ls -la /boot` funciona interactivamente.

---

## Repository Layout

```
NWIN-OS/
|-- Cargo.toml                 Crate manifest (bin name "NWIN_OS")
|-- Cargo.lock                 Reproducible build state
|-- LICENSE                    GPLv3 license file
|-- linker.ld                  Custom linker script (W^X PHDRs)
|-- x86_64-kernel.json         Custom Rust target spec
|
|-- scripts/
|   |-- build.bat              cargo build wrapper
|   `-- rung_debug.bat         Launches QEMU with monitor + serial log
|
|-- esp/                       EFI System Partition - boot artifacts
|   |-- EFI/BOOT/BOOTX64.EFI
|   |-- limine.conf
|   |-- startup.nsh            UEFI shell script that boots BOOTX64.EFI
|   |-- NWIN_OS                The kernel ELF (copied from target/)
|   `-- (initramfs.tar, shell.elf, test_mem.elf dropped in by Limine)
|
|-- src/                       Kernel source (no_std Rust)
|   |-- main.rs                Limine requests, _start init sequence
|   |-- core/                  cpu  gdt  idt  syscall  panic  error  sync
|   |-- drivers/
|   |   |-- block/
|   |   |   |-- mod.rs         AhciDisk BlockDevice impl
|   |   |   `-- ahci/         regs  port  mod
|   |   |-- bus/pci.rs         PCI scan + Bar5
|   |   |-- char/serial.rs     COM1 + serial_print! macros
|   |   |-- display/           fb  tty  mod (Writer + WRITER)
|   |   |-- input/keyboard.rs  PS/2 + atomic ring buffer
|   |   `-- timer/pit.rs       PIT @ 100 Hz
|   |-- fs/
|   |   |-- mod.rs             BlockDevice trait, init_filesystem()
|   |   |-- vfs.rs             VNode trait, VFS_ROOT, MOUNT_TABLE
|   |   |-- fd.rs              FileDescriptor + FdTable
|   |   |-- manager.rs         process_disk dispatcher
|   |   |-- partition/
|   |   |   |-- mod.rs
|   |   |   `-- mbr.rs         MBR parser + constants
|   |   |-- fat/
|   |   |   |-- mod.rs         FatNode VNode impl
|   |   |   |-- bpb.rs         Boot sector structures
|   |   |   `-- volume.rs      FatVolume (mount, cluster math)
|   |   `-- ext4/
|   |       |-- mod.rs         Ext4Node VNode impl
|   |       |-- super_block.rs
|   |       |-- inode.rs
|   |       |-- extents.rs
|   |       |-- block_group.rs
|   |       `-- dir_entry.rs
|   |-- mm/
|   |   |-- mod.rs             mm::init entry point
|   |   |-- memory.rs          Bitmap allocator + CoW + PML4 helpers
|   |   `-- allocator.rs       Interrupt-safe heap
|   `-- task/
|       |-- mod.rs             init_multitasking + Reaper daemon
|       |-- task.rs            Task / TaskContext / context_switch
|       |-- task_manager.rs    TaskManager + TASK_MANAGER
|       |-- scheduler.rs       Schedule loop + KERNEL_RSP swap
|       |-- usermode.rs        jump_to_user_mode helper
|       `-- elf.rs             Static ELF64 loader
|
|-- userspace/
|   `-- test_mem.rs            Source of esp/test_mem.elf
|
`-- tests/
    `-- disk-images/
        |-- ext4_test.img      32 MiB ext4 fixture (gitignored)
        `-- README.md          How to regenerate
```

The crate is configured as:

```toml
[[bin]]
name    = "NWIN_OS"
path    = "src/main.rs"
harness = false

[build]
target  = "x86_64-unknown-none"

[dependencies]
limine            = "0.6.3"
x86_64            = "0.14.12"
lazy_static       = { version = "1.4.0", features = ["spin_no_std"] }
spin              = "0.9.8"
font8x8           = { version = "0.3.1", default-features = false, features = ["unicode"] }
linked_list_allocator = "0.10.5"
pic8259           = "0.10.1"
crossbeam-queue   = { version = "0.3.8", default-features = false, features = ["alloc"] }
pc-keyboard       = "0.7.0"
bitflags          = "2.13.0"

[profile.release]
panic      = "abort"
opt-level  = "z"
lto        = true
codegen-units = 1
```

---

## Building and Running

### Requirements

- **Rust nightly** (the project uses `#![feature(abi_x86_interrupt)]`).
- **QEMU** for x86_64 (tested with qemu-system-x86_64, OVMF /
  edk2-x86_64-code.fd on the host).
- A POSIX shell on the host (PowerShell works on Windows).

### Build

From the repository root:

```bash
# Linux / macOS / WSL
cargo build

# Windows
scripts\build.bat
```

The build emits the ELF kernel image at
`target/x86_64-kernel/debug/NWIN_OS` (`scripts\build.bat`
specifically prints `target\debug\NWIN_OS`). Copy it into the
ESP:

```bash
cp target/x86_64-kernel/debug/NWIN_OS esp/NWIN_OS
```

### Boot artifacts in `esp/`

`limine.conf` (full content as shipped):

```ini
timeout: 0
term_margin: 0

/NWIN_OS (Alpha)
    protocol: limine
    path: boot():/NWIN_OS
    module_path: boot():/initramfs.tar
```

`startup.nsh` boots the Limine UEFI loader directly:

```nsh
\EFI\BOOT\BOOTX64.EFI
```

`BOOTX64.EFI` must be the Limine UEFI loader binary (downloaded
from the Limine release that matches the protocol version
requested by `src/main.rs`).

### Run under QEMU

The Windows wrapper is at `scripts/rung_debug.bat`. Its full
contents:

```bat
start "QEMU-MAIN" qemu-system-x86_64 ^
  -drive if=pflash,format=raw,readonly=on,file="C:\Program Files\qemu\share\edk2-x86_64-code.fd" ^
  -drive format=raw,file=fat:rw:esp ^
  -drive format=raw,file=tests/disk-images/ext4_test.img ^
  -m 512 ^
  -monitor stdio ^
  -serial file:serial.log
```

This boots an OVMF firmware, attaches `esp/` as a FAT read-write
drive that Limine can read, attaches the ext4 fixture as a
secondary raw disk, and pipes both QEMU monitor and serial
output to the console (serial output is also written to
`serial.log`).

Two terminals are useful:

1. One running `scripts\rung_debug.bat` (QEMU monitor is on its
   stdin/stdout).
2. One tailing `serial.log` to watch the kernel trace.

On boot you should see the trace:

```
>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<<
=== SISTEMA DE TELEMETRIA EN LINEA ===
[OK] Puerto COM1 Inicializado.
=== NWIN OS Kernel Iniciando ===
[OK] Coprocesador SIMD/SSE habilitado.
[OK] Local APIC apagado. Enrutamiento legado activo.
[OK] GDT, IDT y Syscalls cargadas.
[OK] Gestor de Memoria y Heap listos.
[PCI] Escaneando hardware...
… per-device log lines …
[OK] VFS inicializado. Arbol de directorios montado (… bytes).
[OK] TaskManager y Scheduler asincrono en linea.
[OK] Reloj PIT arrancado a 100Hz.
[DAEMON] Reaper thread online. Sleeping…
[OK] User Shell deployed and queued for Ring 3.
[MAIN] Halting boot thread. Waiting for PIT timer…
```

If the FAT branch fires (depending on the fixture mounted), you
will also see `[VFS] -> ¡Firma MBR confirmada! …` and an ext4
log section with the POSIX round-trip test that creates
`nwin_core.txt`.

### Regenerating the ext4 test image

See `tests/disk-images/README.md`. On Linux:

```bash
dd if=/dev/zero of=tests/disk-images/ext4_test.img bs=1M count=32
mkfs.ext4 -O ^has_journal,^extent,^64bit -b 4096 tests/disk-images/ext4_test.img
```

The image is **not** committed to the repository (the
`tests/disk-images/` directory ships only the README); it is
generated and consumed locally.

---

## Testing

There is no `cargo test` suite yet (`harness = false` and
`#![no_main]` make the standard test harness unusable). Tests
today are:

- **Boot smoke test.** Just `cargo build` and run under QEMU;
  the boot banner reports `>>> [SISTEMA] INICIANDO NWIN OS
  (VERSIÓN VFS-FAT32) <<<` and the kernel prints its init
  trace on serial.
- **Userspace smoke test.** `esp/test_mem.elf` is loaded by
  Limine as an additional Limine module (alongside the TAR) and
  exercises `write`, `brk`, `mmap`, and `exit` in Ring 3 via the
  syscall ABI.
- **POSIX round-trip test.** During boot the kernel itself
  creates `nwin_core.txt` on the ext4 fixture, writes the
  `"¡Hola Mundo!…"` string via the VFS, reads it back through
  the same VFS, and prints the recovered content to serial.
  This is the empirical check that the AHCI → ext4 → VFS path
  is wired end to end.

Block-level unit tests against `no_std` testable extractions of
kernel primitives are an open task. They will be added alongside
the Block 2.1 scheduler work and the Block 3.1 LinuxKPI shim.

---

## Estructura del Repositorio

```
NWIN-OS/
|-- Cargo.toml                 Manifiesto del crate (bin "NWIN_OS")
|-- Cargo.lock                 Estado reproducible de build
|-- LICENSE                    Archivo de licencia GPLv3
|-- linker.ld                  Linker script a medida (PHDRs W^X)
|-- x86_64-kernel.json         Spec de target Rust personalizada
|
|-- scripts/
|   |-- build.bat              Envoltorio de cargo build
|   `-- rung_debug.bat         Lanza QEMU con monitor + serial log
|
|-- esp/                       EFI System Partition - artefactos de boot
|   |-- EFI/BOOT/BOOTX64.EFI
|   |-- limine.conf
|   |-- startup.nsh            Script UEFI que arranca BOOTX64.EFI
|   |-- NWIN_OS                El ELF del kernel (copiado de target/)
|   `-- (initramfs.tar, shell.elf, test_mem.elf dejados por Limine)
|
|-- src/                       Fuente del kernel (Rust no_std)
|   |-- main.rs                Peticiones Limine, secuencia init _start
|   |-- core/                  cpu  gdt  idt  syscall  panic  error  sync
|   |-- drivers/
|   |   |-- block/
|   |   |   |-- mod.rs         Impl BlockDevice de AhciDisk
|   |   |   `-- ahci/         regs  port  mod
|   |   |-- bus/pci.rs         Escaneo PCI + Bar5
|   |   |-- char/serial.rs     COM1 + macros serial_print!
|   |   |-- display/           fb  tty  mod (Writer + WRITER)
|   |   |-- input/keyboard.rs  PS/2 + ring buffer atomico
|   |   `-- timer/pit.rs       PIT @ 100 Hz
|   |-- fs/
|   |   |-- mod.rs             Trait BlockDevice, init_filesystem()
|   |   |-- vfs.rs             Trait VNode, VFS_ROOT, MOUNT_TABLE
|   |   |-- fd.rs              FileDescriptor + FdTable
|   |   |-- manager.rs         Despachador process_disk
|   |   |-- partition/
|   |   |   |-- mod.rs
|   |   |   `-- mbr.rs         Parser MBR + constantes
|   |   |-- fat/
|   |   |   |-- mod.rs         Impl VNode de FatNode
|   |   |   |-- bpb.rs         Estructuras de boot sector
|   |   |   `-- volume.rs      FatVolume (mount, matematica de cluster)
|   |   `-- ext4/
|   |       |-- mod.rs         Impl VNode de Ext4Node
|   |       |-- super_block.rs
|   |       |-- inode.rs
|   |       |-- extents.rs
|   |       |-- block_group.rs
|   |       `-- dir_entry.rs
|   |-- mm/
|   |   |-- mod.rs             Punto de entrada mm::init
|   |   |-- memory.rs          Bitmap allocator + CoW + helpers PML4
|   |   `-- allocator.rs       Heap interrupt-safe
|   `-- task/
|       |-- mod.rs             init_multitasking + daemon Reaper
|       |-- task.rs            Task / TaskContext / context_switch
|       |-- task_manager.rs    TaskManager + TASK_MANAGER
|       |-- scheduler.rs       Bucle schedule + swap KERNEL_RSP
|       |-- usermode.rs        Helper jump_to_user_mode
|       `-- elf.rs             Cargador ELF64 estatico
|
|-- userspace/
|   `-- test_mem.rs            Fuente de esp/test_mem.elf
|
`-- tests/
    `-- disk-images/
        |-- ext4_test.img      Fixture ext4 de 32 MiB (en .gitignore)
        `-- README.md          Como regenerarla
```

El crate esta configurado como:

```toml
[[bin]]
name    = "NWIN_OS"
path    = "src/main.rs"
harness = false

[build]
target  = "x86_64-unknown-none"

[dependencies]
limine            = "0.6.3"
x86_64            = "0.14.12"
lazy_static       = { version = "1.4.0", features = ["spin_no_std"] }
spin              = "0.9.8"
font8x8           = { version = "0.3.1", default-features = false, features = ["unicode"] }
linked_list_allocator = "0.10.5"
pic8259           = "0.10.1"
crossbeam-queue   = { version = "0.3.8", default-features = false, features = ["alloc"] }
pc-keyboard       = "0.7.0"
bitflags          = "2.13.0"

[profile.release]
panic         = "abort"
opt-level     = "z"
lto           = true
codegen-units = 1
```

---

## Compilacion y Ejecucion

### Requisitos

- **Rust nightly** (el proyecto usa
  `#![feature(abi_x86_interrupt)]`).
- **QEMU** para x86_64 (probado con qemu-system-x86_64, OVMF /
  edk2-x86-64-code.fd en el host).
- Un shell POSIX en el host (PowerShell funciona en Windows).

### Build

Desde la raiz del repositorio:

```bash
# Linux / macOS / WSL
cargo build

# Windows
scripts\build.bat
```

El binario ELF queda en
`target/x86_64-kernel/debug/NWIN_OS` (`scripts\build.bat`
imprime especificamente `target\debug\NWIN_OS`). Copialo a la
ESP:

```bash
cp target/x86_64-kernel/debug/NWIN_OS esp/NWIN_OS
```

### Artefactos de boot en `esp/`

`limine.conf` (contenido completo tal como se distribuye):

```ini
timeout: 0
term_margin: 0

/NWIN_OS (Alpha)
    protocol: limine
    path: boot():/NWIN_OS
    module_path: boot():/initramfs.tar
```

`startup.nsh` arranca directamente el loader UEFI de Limine:

```nsh
\EFI\BOOT\BOOTX64.EFI
```

`BOOTX64.EFI` debe ser el binario del loader UEFI de Limine
(descargado de la release de Limine que coincida con la version
de protocolo solicitada por `src/main.rs`).

### Ejecutar bajo QEMU

El envoltorio de Windows esta en `scripts/rung_debug.bat`. Su
contenido completo:

```bat
start "QEMU-MAIN" qemu-system-x86_64 ^
  -drive if=pflash,format=raw,readonly=on,file="C:\Program Files\qemu\share\edk2-x86_64-code.fd" ^
  -drive format=raw,file=fat:rw:esp ^
  -drive format=raw,file=tests/disk-images/ext4_test.img ^
  -m 512 ^
  -monitor stdio ^
  -serial file:serial.log
```

Esto arranca un firmware OVMF, monta `esp/` como una unidad FAT
de lectura/escritura que Limine puede leer, monta la fixture
ext4 como disco raw secundario y redirige la salida del monitor
de QEMU y la salida serie a la consola (la salida serie ademas
se escribe a `serial.log`).

Son utiles dos terminales:

1. Una ejecutando `scripts\rung_debug.bat` (el monitor de QEMU
   esta en su stdin/stdout).
2. Otra haciendo `tail -f serial.log` para ver la traza del
   kernel.

Al arrancar deberias ver la traza:

```
>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<<
=== SISTEMA DE TELEMETRIA EN LINEA ===
[OK] Puerto COM1 Inicializado.
=== NWIN OS Kernel Iniciando ===
[OK] Coprocesador SIMD/SSE habilitado.
[OK] Local APIC apagado. Enrutamiento legado activo.
[OK] GDT, IDT y Syscalls cargadas.
[OK] Gestor de Memoria y Heap listos.
[PCI] Escaneando hardware...
… lineas por dispositivo …
[OK] VFS inicializado. Arbol de directorios montado (… bytes).
[OK] TaskManager y Scheduler asincrono en linea.
[OK] Reloj PIT arrancado a 100Hz.
[DAEMON] Reaper thread online. Sleeping…
[OK] User Shell deployed and queued for Ring 3.
[MAIN] Halting boot thread. Waiting for PIT timer…
```

Si se dispara la rama FAT (dependiendo de la fixture montada)
tambien veras `[VFS] -> ¡Firma MBR confirmada! …` y un log de
ext4 con la prueba round-trip POSIX que crea `nwin_core.txt`.

### Regenerar la imagen de prueba ext4

Ver `tests/disk-images/README.md`. En Linux:

```bash
dd if=/dev/zero of=tests/disk-images/ext4_test.img bs=1M count=32
mkfs.ext4 -O ^has_journal,^extent,^64bit -b 4096 tests/disk-images/ext4_test.img
```

La imagen **no** se commitea al repositorio (el directorio
`tests/disk-images/` solo distribuye el README); se genera y
consume localmente.

---

## Pruebas

No existe aun una suite `cargo test` (`harness = false` y
`#![no_main]` inutilizan el harness estandar). Las pruebas hoy
son:

- **Boot smoke test.** Solo `cargo build` y ejecutar bajo QEMU;
  el banner de arranque reporta
  `>>> [SISTEMA] INICIANDO NWIN OS (VERSIÓN VFS-FAT32) <<<` y
  el kernel imprime su traza init por puerto serie.
- **Smoke test de espacio de usuario.** `esp/test_mem.elf` se
  carga como modulo Limine adicional (junto al TAR) y ejercita
  `write`, `brk`, `mmap` y `exit` en Ring 3 mediante la ABI de
  syscalls.
- **Prueba round-trip POSIX.** Durante el arranque el propio
  kernel crea `nwin_core.txt` en la fixture ext4, escribe la
  cadena `"¡Hola Mundo!…"` a traves del VFS, lo relee por el
  mismo VFS e imprime el contenido recuperado por puerto serie.
  Es la comprobacion empirica de que la cadena AHCI → ext4 → VFS
  esta cableada de extremo a extremo.

Los tests unitarios a nivel de bloque contra extracciones
`no_std` testeables de las primitivas del kernel son una tarea
abierta. Se anadiran junto al trabajo del Bloque 2.1
(planificador) y al Bloque 3.1 (shim LinuxKPI).

---

## Contributing

Contributions are welcome. The flow follows a Linux-kernel-style
patch workflow: plain text patches sent by email, reviewed by the
maintainer, then merged. This keeps the change history readable
and avoids GitHub-only artefacts in the project archive.

### Reporting Issues

- Open an issue at <https://github.com/JoseusZ/NWIN-OS/issues>.
- Include: target commit (`git rev-parse HEAD`), QEMU version,
  Rust toolchain (`rustc --version --verbose`), and a serial log
  excerpt.
- For crashes, attach the backtrace from QEMU's
  `-d guest_errors` flag and the panic message.
- For empirical fact-checks against the README, cite the file
  and line number that disagrees with the description.

### Sending Patches

1. Make your changes on a topic branch:

   ```bash
   git checkout -b topic/short-description
   ```

2. Write a good commit message:

   ```
   subsystem: short imperative summary (<= 72 chars)

   More detailed explanatory text. Wrap at 72 columns. Explain
   the problem being solved, the approach taken, and any
   trade-offs or follow-ups.

   Signed-off-by: Your Name <you@example.com>
   ```

3. Format your series as a single file per patch in
   `git format-patch` style:

   ```bash
   git format-patch -M -C -o outgoing/ master..topic/short-description
   ```

4. Send the patches by email to
   **joseuszoficial@gmail.com** with the subject prefix
   `[NWIN-OS]`. Use `git send-email` if you have it configured,
   or attach the `.patch` files directly.

5. Wait for review. Expect one or more rounds of comments;
   address them with follow-up patches (`git send-email -v2 ...`).
   When the maintainer adds `Acked-by`, the patches will be
   applied to master.

### Coding Style

- **`rustfmt`** with the default settings; run `cargo fmt` before
  sending a patch.
- **`clippy`** clean at the default level; run `cargo clippy`
  before sending a patch.
- One logical change per commit.
- Do **not** mix refactors with feature work.
- Honour the **immutability rules** that the rest of this
  project relies on:
  - `core/gdt.rs`, `core/idt.rs`, `core/cpu.rs` are stable
    targets — changes here affect every Ring 0 / Ring 3
    transition in the system.
  - The CoW machinery and reference counters in
    `mm/memory.rs` are stable — they underpin the planned
    `fork` syscall.
  - The stack-frame layout produced by `task/task.rs::Task::new`
    is stable — it is consumed by `user_mode_trampoline` and
    by `context_switch`.
  - The Linux x86-64 syscall ABI (`core/syscall.rs`) is
    stable — user-space binaries depend on it.

### License and Sign-off

NWIN OS is released under the **GNU General Public License v3**
(GPLv3). See [`LICENSE`](LICENSE) for the full text. By sending
a patch you agree to license your contribution under the same
terms. Add a `Signed-off-by:` line to your commit message to
certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/).

---

## Maintainer

Joseus Z - <joseuszoficial@gmail.com>

Project home: <https://github.com/JoseusZ/NWIN-OS>

> **NWIN is Neither Windows Interface nor Native Linux** —
> an x86_64 Rust kernel built from scratch for real hardware.

---

## Como Contribuir

Las contribuciones son bienvenidas. El flujo sigue un workflow de
parches estilo kernel Linux: parches en texto plano enviados por
correo electronico, revisados por el maintainer y luego
fusionados. Esto mantiene el historial de cambios legible y
evita artefactos exclusivos de GitHub en el archivo del
proyecto.

### Reportar Issues

- Abre un issue en <https://github.com/JoseusZ/NWIN-OS/issues>.
- Incluye: commit objetivo (`git rev-parse HEAD`), version de
  QEMU, toolchain de Rust (`rustc --version --verbose`) y un
  extracto del log serie.
- Para crashes, adjunta el backtrace de QEMU usando
  `-d guest_errors` y el mensaje de panico.
- Para verificaciones empiricas contra el README, cita el
  archivo y la linea que no coincide con la descripcion.

### Enviar Parches

1. Crea una rama de trabajo:

   ```bash
   git checkout -b topic/descripcion-corta
   ```

2. Escribe un buen mensaje de commit:

   ```
   subsistema: resumen imperativo corto (<= 72 chars)

   Texto explicativo mas detallado. Wrap a 72 columnas. Explica
   el problema, la solucion y los trade-offs.

   Signed-off-by: Tu Nombre <tu@ejemplo.com>
   ```

3. Formatea tu serie como un fichero por parche con estilo
   `git format-patch`:

   ```bash
   git format-patch -M -C -o outgoing/ master..topic/descripcion-corta
   ```

4. Envia los parches por correo a **joseuszoficial@gmail.com**
   con el prefijo `[NWIN-OS]` en el subject. Usa
   `git send-email` si lo tienes configurado o adjunta los
   `.patch` directamente.

5. Espera la revision. Es normal una o mas rondas de
   comentarios; respondelas con parches follow-up
   (`git send-email -v2 ...`). Cuando el maintainer anada
   `Acked-by`, los parches se aplican a master.

### Estilo de Codigo

- **`rustfmt`** con la configuracion por defecto; ejecuta
  `cargo fmt` antes de enviar un parche.
- **`clippy`** limpio al nivel por defecto; ejecuta
  `cargo clippy` antes de enviar un parche.
- Un cambio logico por commit.
- **No** mezclar refactors con trabajo de funcionalidades.
- Respeta las **reglas de inmutabilidad** sobre las que se
  sostiene el resto del proyecto:
  - `core/gdt.rs`, `core/idt.rs`, `core/cpu.rs` son objetivos
    estables — un cambio aqui afecta a cada transicion
    Ring 0 / Ring 3 del sistema.
  - La maquinaria CoW y los contadores de referencia en
    `mm/memory.rs` son estables — apuntalan el futuro
    syscall `fork`.
  - El layout del stack-frame producido por
    `task/task.rs::Task::new` es estable — lo consumen
    `user_mode_trampoline` y `context_switch`.
  - La ABI de syscalls Linux x86-64 (`core/syscall.rs`) es
    estable — los binarios de espacio de usuario dependen
    de ella.

### Licencia y Sign-off

NWIN OS se distribuye bajo la **GNU General Public License v3**
(GPLv3). Consulta [`LICENSE`](LICENSE) para ver el texto
completo. Al enviar un parche aceptas licenciarlo bajo los
mismos terminos. Agrega una linea `Signed-off-by:` a tu commit
para certificar el
[Developer Certificate of Origin 1.1](https://developercertificate.org/).

---

## Maintainer / Mantenedor

Joseus Z - <joseuszoficial@gmail.com>

Project home: <https://github.com/JoseusZ/NWIN-OS>

> **NWIN is Neither Windows Interface nor Native Linux** —
> un kernel Rust x86_64 construido desde cero para hardware
> real.

---

*End of document / Fin del documento.*

- [Project Goal](#project-goal) / [Objetivo del Proyecto](#objetivo-del-proyecto)
- [Current Capabilities](#current-capabilities) / [Capacidades Actuales](#capacidades-actuales)
- [Boot and Platform](#boot-and-platform) / [Arranque y Plataforma](#arranque-y-plataforma)
- [CPU and Architecture](#cpu-and-architecture) / [CPU y Arquitectura](#cpu-y-arquitectura)
- [Interrupts and System Calls](#interrupts-and-system-calls) / [Interrupciones y Llamadas al Sistema](#interrupciones-y-llamadas-al-sistema)
- [Memory Management](#memory-management) / [Gestion de Memoria](#gestion-de-memoria)
- [Multitasking and Scheduler](#multitasking-and-scheduler) / [Multitarea y Planificador](#multitarea-y-planificador)
- [ELF Loader](#elf-loader) / [Cargador ELF](#cargador-elf)
- [Drivers](#drivers) / [Controladores](#controladores)
- [Filesystems](#filesystems) / [Sistemas de Archivos](#sistemas-de-archivos)
- [Error Handling](#error-handling) / [Manejo de Errores](#manejo-de-errores)
- [Architecture Overview](#architecture-overview) / [Vision de Arquitectura](#vision-de-arquitectura)
- [Known Limitations](#known-limitations) / [Limitaciones Conocidas](#limitaciones-conocidas)
- [Roadmap / Future Work](#roadmap--future-work) / [Hoja de Ruta / Trabajo Futuro](#hoja-de-ruta--trabajo-futuro)
- [Repository Layout](#repository-layout) / [Estructura del Repositorio](#estructura-del-repositorio)
- [Building and Running](#building-and-running) / [Compilacion y Ejecucion](#compilacion-y-ejecucion)
- [Testing](#testing) / [Pruebas](#pruebas)
- [Contributing](#contributing) / [Como Contribuir](#como-contribuir)
- [Maintainer](#maintainer)