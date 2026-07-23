// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-process file-descriptor table.
//!
//! [`FileDescriptor`] models every I/O resource the kernel exposes
//! through the POSIX ABI, and [`FdTable`] stores the open descriptors
//! of a single task (PCB).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use crate::fs::vfs::VNode;

/// Any Input/Output resource visible to user code.
///
/// Modelled as a Rust enum to avoid null pointers and the
/// C-style inheritance trap; the variants are kept flat on purpose
/// because each one carries a different set of fields.
#[derive(Clone)]
pub enum FileDescriptor {
    Stdin,
    Stdout,
    Stderr,
    RegularFile {
        /// Abstract reference to the file node (RAM, SATA, network, …).
        vnode: Arc<dyn VNode>,
        /// Cursor: which byte the program is currently reading or writing.
        offset: usize,
    },
}

/// Per-process file-descriptor table.
///
/// Kept separate from the PCB so the task structure only stores a
/// cheap handle (clone) without dragging the descriptor map around.
#[derive(Clone)]
pub struct FdTable {
    descriptors: BTreeMap<usize, FileDescriptor>,
    next_fd: usize,
}

impl FdTable {
    /// Creates a fresh table with the standard UNIX "POSIX trinity"
    /// (`stdin`, `stdout`, `stderr`) pre-registered at FDs 0/1/2.
    pub fn new() -> Self {
        let mut table = FdTable {
            descriptors: BTreeMap::new(),
            next_fd: 3, // FDs 0, 1 and 2 are reserved for the POSIX trinity
        };

        table.descriptors.insert(0, FileDescriptor::Stdin);
        table.descriptors.insert(1, FileDescriptor::Stdout);
        table.descriptors.insert(2, FileDescriptor::Stderr);

        table
    }

    /// Returns a mutable reference to the descriptor at slot `fd`,
    /// or `None` if no such descriptor exists.
    pub fn get_mut(&mut self, fd: usize) -> Option<&mut FileDescriptor> {
        self.descriptors.get_mut(&fd)
    }

    /// Registers `file` in the next free slot and returns the
    /// descriptor number assigned to it.
    pub fn insert(&mut self, file: FileDescriptor) -> usize {
        let fd = self.next_fd;
        self.descriptors.insert(fd, file);
        self.next_fd += 1;
        fd
    }

    /// Closes `fd` and returns whether anything was actually removed.
    ///
    /// FDs 0/1/2 are currently protected so a misbehaving user
    /// program cannot detach its own standard streams.
    pub fn close(&mut self, fd: usize) -> bool {
        if fd < 3 { return false; } // prevent closing the POSIX trinity
        self.descriptors.remove(&fd).is_some()
    }
}