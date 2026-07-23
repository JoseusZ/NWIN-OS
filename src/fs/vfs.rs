// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Virtual File System core: the [`VNode`] trait, the in-memory TAR
//! reader used for the initramfs, the POSIX-style path resolver and
//! the mount table.
//!
//! Drivers for FAT and ext4 attach themselves by mounting a root
//! [`VNode`] into [`MOUNT_TABLE`]; from that point on
//! [`resolve_path`] transparently crosses mount points.

use core::str;
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::collections::BTreeMap;
use alloc::string::String;

// ====================================================================
// CONCURRENCY SAFETY PRIMITIVES
// ====================================================================

// Raw pointers (*const u8) do not implement Send or Sync by default,
// which prevents storing them in a `lazy_static!` global. Wrapping
// the pointer in `TarContext` and explicitly promising the compiler
// that access is synchronised through a Mutex removes the E0277
// error without compromising safety.

/// A virtual filesystem node: the common interface every filesystem
/// driver (TAR, FAT, ext4, …) implements.
pub trait VNode: Send + Sync {
    /// Reads up to `buf.len()` bytes starting at `offset`.
    ///
    /// Returns the number of bytes actually copied; `0` signals EOF
    /// or an out-of-bounds offset.
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }

    /// Total size of the underlying data in bytes.
    fn get_size(&self) -> usize { 0 }

    /// Whether this node represents a directory.
    fn is_dir(&self) -> bool { false }

    /// Looks up a child node by name within this directory.
    fn lookup(&self, _name: &str) -> Option<alloc::sync::Arc<dyn VNode>> { None }
}


/// Raw pointer + length bundle pointing at the initramfs TAR image
/// supplied by Limine.
#[derive(Clone, Copy)]
pub struct TarContext {
    pub ptr: *const u8,
    pub size: usize,
}

// Safety: every read of the initramfs goes through a Mutex in
// `INITRAMFS`, so concurrent access is synchronised.
unsafe impl Send for TarContext {}
unsafe impl Sync for TarContext {}

// ====================================================================
// GLOBAL VFS VAULT
// ====================================================================

lazy_static! {
    /// Address and size of the initramfs TAR, or `None` until
    /// [`init`] is called from `main.rs`.
    pub static ref INITRAMFS: Mutex<Option<TarContext>> = Mutex::new(None);
    /// Maps absolute paths (`"/"`, `"/mnt"`, …) to the root [`VNode`]
    /// of every filesystem mounted into the VFS.
    pub static ref MOUNT_TABLE: MountTable = MountTable::new();
    /// In-memory tree built by [`build_vfs_tree_from_tar`] before any
    /// real disk driver attaches.
    pub static ref VFS_ROOT: alloc::sync::Arc<DirVNode> = alloc::sync::Arc::new(DirVNode::new());
}

/// Stores the initramfs address for the rest of the boot sequence.
///
/// Must be called exactly once from `main.rs` before any
/// [`find_file`] / [`build_vfs_tree_from_tar`] invocation.
pub fn init(tar_address: *const u8, tar_size: usize) {
    *INITRAMFS.lock() = Some(TarContext {
        ptr: tar_address,
        size: tar_size,
    });
}

// ====================================================================
// LOOKUP AND READ LOGIC
// ====================================================================

/// Searches the initramfs TAR for `filename` and returns the raw
/// bytes of its payload, or `None` when the file is missing or the
/// VFS has not been initialised yet.
pub fn find_file(filename: &str) -> Option<&'static [u8]> {
    let initramfs = INITRAMFS.lock();

    if let Some(tar_ctx) = *initramfs {
        let mut offset = 0;

        while offset + 512 <= tar_ctx.size {
            // TAR entries always start with a 512-byte header.
            let header_ptr = unsafe { tar_ctx.ptr.add(offset) };
            let header = unsafe { core::slice::from_raw_parts(header_ptr, 512) };

            // A zeroed first byte marks the end of the TAR stream.
            if header[0] == 0 {
                break;
            }

            // 1. Read the file name (first 100 bytes, NUL-terminated).
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let current_filename = core::str::from_utf8(&header[0..name_len]).unwrap_or("");

            // 2. Read the size (bytes 124..135, octal ASCII).
            let size_bytes = &header[124..135]; // 11 bytes + NUL terminator
            let size_str = core::str::from_utf8(size_bytes).unwrap_or("0").trim();
            let file_size = usize::from_str_radix(size_str, 8).unwrap_or(0);

            // 3. Compute the data offset.
            let data_start = offset + 512;

            // Match?
            if current_filename == filename {
                let data_ptr = unsafe { tar_ctx.ptr.add(data_start) };
                let data_slice = unsafe { core::slice::from_raw_parts(data_ptr, file_size) };
                return Some(data_slice);
            }

            // 4. Advance to the next entry (payload rounded up to 512).
            let data_blocks = (file_size + 511) / 512;
            offset += 512 + (data_blocks * 512);
        }
    }

    None
}

/// Read-only [`VNode`] backed by a slice of initramfs memory.
pub struct TarVNode {
    data: &'static [u8],
}

impl TarVNode {
    /// Builds a TAR-backed node from the byte slice returned by
    /// [`find_file`].
    pub fn new(data: &'static [u8]) -> Self {
        Self { data }
    }
}

impl VNode for TarVNode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        // Refuse out-of-bounds reads.
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

/// Opens a file and returns it as a standardised Virtual Node.
///
/// Thin wrapper around [`resolve_path`] used by the syscall layer so
/// the rest of the kernel does not need to know how paths are
/// resolved.
pub fn open_vnode(path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
    resolve_path(path)
}

/// In-memory directory node used by the TAR initramfs and as the
/// default root when no disk is mounted at `"/"`.
pub struct DirVNode {
    children: Mutex<BTreeMap<String, alloc::sync::Arc<dyn VNode>>>,
}

impl DirVNode {
    /// Creates an empty directory node.
    pub fn new() -> Self {
        Self {
            children: Mutex::new(BTreeMap::new()),
        }
    }

    /// Inserts (or replaces) a child node by name.
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
// PATH RESOLUTION ENGINE (POSIX-STYLE)
// ====================================================================

/// Resolves an absolute POSIX path against the live mount table and
/// the in-memory tree.
///
/// Returns `None` if any component of the path is missing or if a
/// non-directory is traversed as a directory.
pub fn resolve_path(path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
    // 1. Start at the mounted root or fall back to the TAR tree.
    let mut current: alloc::sync::Arc<dyn VNode> = MOUNT_TABLE.get_mount("/")
        .unwrap_or_else(|| VFS_ROOT.clone());

    let parts = path.split('/').filter(|p| !p.is_empty());
    let mut current_path = alloc::string::String::from("");

    for part in parts {
        if !current.is_dir() {
            return None; // tried to descend into a file
        }

        // Rebuild the path incrementally so mount points can be
        // intercepted at every level.
        current_path.push('/');
        current_path.push_str(part);

        // 2. POSIX interception: if a disk is mounted on this exact
        //    path, cross the boundary into it.
        if let Some(mounted_node) = MOUNT_TABLE.get_mount(&current_path) {
            current = mounted_node;
            continue;
        }

        // 3. No mount boundary: walk the in-memory tree.
        match current.lookup(part) {
            Some(next_node) => current = next_node,
            None => return None, // path does not resolve
        }
    }

    Some(current)
}

// ====================================================================
// MOUNT TABLE
// ====================================================================

/// Thread-safe map of absolute paths to mounted filesystem roots.
pub struct MountTable {
    mounts: spin::Mutex<alloc::collections::BTreeMap<alloc::string::String, alloc::sync::Arc<dyn VNode>>>,
}

impl MountTable {
    /// Creates an empty mount table.
    pub fn new() -> Self {
        Self {
            mounts: spin::Mutex::new(alloc::collections::BTreeMap::new()),
        }
    }

    /// Anchors a [`VNode`] (a whole filesystem tree) at `path`,
    /// making it visible to [`resolve_path`].
    pub fn mount(&self, path: &str, node: alloc::sync::Arc<dyn VNode>) {
        self.mounts.lock().insert(alloc::string::String::from(path), node);
    }

    /// Returns the [`VNode`] mounted at `path`, if any.
    pub fn get_mount(&self, path: &str) -> Option<alloc::sync::Arc<dyn VNode>> {
        self.mounts.lock().get(path).cloned()
    }

    /// Removes the mount at `path`, returning `true` when something
    /// was actually unmounted. Required for `pivot_root`.
    pub fn unmount(&self, path: &str) -> bool {
        self.mounts.lock().remove(path).is_some()
    }
}

/// Walks the initramfs TAR image and registers every non-empty file
/// as a [`TarVNode`] under [`VFS_ROOT`].
pub fn build_vfs_tree_from_tar() {
    let initramfs = INITRAMFS.lock();

    if let Some(tar_ctx) = *initramfs {
        let mut offset = 0;

        while offset + 512 <= tar_ctx.size {
            let header_ptr = unsafe { tar_ctx.ptr.add(offset) };
            let header = unsafe { core::slice::from_raw_parts(header_ptr, 512) };

            // End-of-archive marker.
            if header[0] == 0 { break; }

            // Read name.
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let current_filename = core::str::from_utf8(&header[0..name_len]).unwrap_or("");

            // Read size.
            let size_bytes = &header[124..135];
            let size_str = core::str::from_utf8(size_bytes).unwrap_or("0").trim();
            let file_size = usize::from_str_radix(size_str, 8).unwrap_or(0);

            let data_start = offset + 512;

            // Register every valid, non-empty file at the TAR root.
            if file_size > 0 && !current_filename.is_empty() {
                let data_ptr = unsafe { tar_ctx.ptr.add(data_start) };
                let data_slice = unsafe { core::slice::from_raw_parts(data_ptr, file_size) };

                let vnode = alloc::sync::Arc::new(TarVNode::new(data_slice));

                // Strip a leading slash so entries like "/shell.elf"
                // become "shell.elf" and fit at the root.
                let clean_name = current_filename.trim_start_matches('/');
                VFS_ROOT.add(clean_name, vnode);
            }

            // Advance to the next entry.
            let data_blocks = (file_size + 511) / 512;
            offset += 512 + (data_blocks * 512);
        }
    }
}