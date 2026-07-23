// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Partition probing: the [`mbr`] submodule owns the legacy MBR parser
//! used by the disk manager to discover FAT and ext4 partitions.

pub mod mbr;
