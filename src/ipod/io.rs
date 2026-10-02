//! Linux block-device access (the `/dev/sdX` layer of `ipodio-posix.c`).

use crate::ipod::{directory::read_directory, get_model, partition::read_partinfo, Ipod};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;

/// `BLKSSZGET` — retrieve the logical sector size of a block device.
const BLKSSZGET: libc::c_ulong = 0x1268;

/// An open block device, plus its detected sector size.
pub struct Device {
    pub path: String,
    file: File,
    pub sector_size: u64,
}

impl Device {
    /// Open a block device read-only (for scanning).
    pub fn open_readonly(path: &str) -> io::Result<Device> {
        let file = OpenOptions::new().read(true).open(path)?;
        let sector_size = get_sector_size(file.as_raw_fd()).unwrap_or(512);
        Ok(Device {
            path: path.to_string(),
            file,
            sector_size,
        })
    }

    /// Reopen the device read-write (required for patching). Returns
    /// `PermissionDenied` if we lack raw-device access.
    pub fn reopen_rw(&mut self) -> io::Result<()> {
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        self.sector_size = get_sector_size(file.as_raw_fd()).unwrap_or(self.sector_size);
        self.file = file;
        Ok(())
    }

    pub fn file(&mut self) -> &mut File {
        &mut self.file
    }

    pub fn raw_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
}

fn get_sector_size(fd: i32) -> Option<u64> {
    let mut size: libc::c_int = 0;
    // SAFETY: BLKSSZGET writes a single c_int into `size`.
    let rc = unsafe { libc::ioctl(fd, BLKSSZGET, &mut size) };
    if rc == 0 && size > 0 {
        Some(size as u64)
    } else {
        None
    }
}

/// Scan `/dev/sda` … `/dev/sdz` for iPods. Returns `(device path, iPod)` for
/// every iPod found.
///
/// `denied` reports how many candidate disks could not be opened due to
/// permissions (a hint that the caller needs root/raw-device access).
pub fn scan() -> (Vec<(String, Ipod)>, usize) {
    let mut found = Vec::new();
    let mut denied = 0usize;

    for i in 0..26 {
        let path = format!("/dev/sd{}", (b'a' + i) as char);

        let mut dev = match Device::open_readonly(&path) {
            Ok(d) => d,
            Err(e) => {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
                continue;
            }
        };

        let mut ipod = Ipod::new(dev.sector_size);

        if read_partinfo(&mut ipod, &mut dev.file).is_err() {
            continue;
        }
        if ipod.pinfo[0].start == 0 || ipod.pinfo[0].ptype != 0 {
            continue;
        }
        if read_directory(&mut ipod, &mut dev.file).is_err() {
            continue;
        }

        let version = ipod.images[ipod.osos_image].vers >> 8;
        let Some(model) = get_model(version) else {
            continue;
        };
        ipod.model = Some(model);

        // RAM size is best-effort; SCSI INQUIRY needs raw-io privileges that
        // we may not have. It only matters for picking `ipodvideo64mb`.
        if let Some(mb) = crate::ipod::scsi::read_ramsize(&dev) {
            ipod.ramsize_mb = mb;
        }

        found.push((path, ipod));
    }

    (found, denied)
}
