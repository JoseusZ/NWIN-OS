# Disk Images for Testing

This directory holds raw disk images used by the NWIN_OS kernel for I/O and
filesystem driver validation.

## ext4_test.img

A 32 MiB raw disk image containing a freshly formatted ext4 filesystem used by
the AHCI driver and the ext4 VFS module for integration tests.

### How to regenerate

The image is **not** committed to the repository (see `.gitignore`). To
recreate it from scratch you need a Linux environment with `mkfs.ext4`
(e.g. WSL, a container, or a real Linux box):

```bash
# Create a 32 MiB blank raw image
dd if=/dev/zero of=ext4_test.img bs=1M count=32

# Format it as ext4 (no journal, fewer features = easier for our parser)
mkfs.ext4 -O ^has_journal,^extent,^64bit -b 4096 ext4_test.img

# Optionally populate it with test data
mkdir mnt
sudo mount -o loop ext4_test.img mnt
echo "hello from NWIN_OS" | sudo tee mnt/README.txt
sudo umount mnt
```

### How the kernel uses it

QEMU attaches the image as a secondary raw disk to the AHCI bus:

```bat
-drive format=raw,file=tests/disk-images/ext4_test.img
```

The kernel's AHCI driver detects the device, the partition manager parses the
MBR, and the ext4 module mounts the filesystem and exposes it through the VFS.

## Adding more test images

Drop new `.img` files here and reference them from `scripts/rung_debug.bat`
using a path relative to the repository root (the script does `cd /d
"%~dp0.."` so all relative paths resolve correctly).

Suggested filenames:

- `fat32_test.img` -- for FAT32 driver validation
- `empty.img`      -- blank disk for partition-table edge cases
