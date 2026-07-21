// src/fs/fd.rs
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use crate::fs::vfs::VNode;

/// Represents any type of Input/Output resource in the system.
/// Using Rust's enums avoids null pointers and C-style inheritance.
#[derive(Clone)]
pub enum FileDescriptor {
    Stdin,
    Stdout,
    Stderr,
    RegularFile {
        /// Abstract reference to the file node (RAM, SATA, Network, etc.)
        vnode: Arc<dyn VNode>,
        /// The cursor: which byte the program is currently reading/writing
        offset: usize,
    },
}

/// The File Descriptor Table for a Process.
/// Encapsulates logic to keep the PCB (Task) clean.
#[derive(Clone)]
pub struct FdTable {
    descriptors: BTreeMap<usize, FileDescriptor>,
    next_fd: usize,
}

impl FdTable {
    /// Creates a new table with the standard UNIX configuration (POSIX Trinity)
    pub fn new() -> Self {
        let mut table = FdTable {
            descriptors: BTreeMap::new(),
            next_fd: 3, // FDs 0, 1, and 2 are reserved
        };
        
        table.descriptors.insert(0, FileDescriptor::Stdin);
        table.descriptors.insert(1, FileDescriptor::Stdout);
        table.descriptors.insert(2, FileDescriptor::Stderr);
        
        table
    }

    /// Gets a mutable reference to an open file
    pub fn get_mut(&mut self, fd: usize) -> Option<&mut FileDescriptor> {
        self.descriptors.get_mut(&fd)
    }

    /// Opens a new file and returns its Descriptor number (FD)
    pub fn insert(&mut self, file: FileDescriptor) -> usize {
        let fd = self.next_fd;
        self.descriptors.insert(fd, file);
        self.next_fd += 1;
        fd
    }

    /// Closes a file
    pub fn close(&mut self, fd: usize) -> bool {
        if fd < 3 { return false; } // Prevent closing standard streams for now
        self.descriptors.remove(&fd).is_some()
    }
}