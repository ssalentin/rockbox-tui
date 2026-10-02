//! Mount-point resolution for an iPod's data partition.

use std::fs;
use std::path::PathBuf;

/// Given a whole-disk device like `/dev/sdc`, find the mount point of its
/// data partition (`/dev/sdc2` for a FAT32 winpod, `/dev/sdc3` for an HFS
/// macpod). Returns `None` if no data partition is currently mounted.
pub fn find_data_mount(dev: &str) -> Option<PathBuf> {
    let content = fs::read_to_string("/proc/mounts").ok()?;
    let mut fallback = None;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let src = parts.next()?;
        let mnt = parts.next()?;

        if let Some(suffix) = src.strip_prefix(dev) {
            if !suffix.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let n: u32 = suffix.parse().ok()?;
            if n >= 2 {
                if n == 2 {
                    return Some(PathBuf::from(mnt));
                }
                if fallback.is_none() {
                    fallback = Some(PathBuf::from(mnt));
                }
            }
        }
    }

    fallback
}
