//! Firmware image directory parsing.

use crate::ipod::{le16, le32, Ipod, SeekRead, APPLE_STOP_SIGN, MAX_IMAGES};
use std::io::SeekFrom;

/// Types of image stored in the firmware partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ftype {
    Osos,
    Rsrc,
    Aupd,
    Hibe,
    Osbk,
}

/// One entry of the firmware image directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub ftype: Ftype,
    pub id: u32,
    pub dev_offset: u32,
    pub len: u32,
    pub addr: u32,
    pub entry_offset: u32,
    pub chksum: u32,
    pub vers: u32,
    pub load_addr: u32,
}

fn image_type(tag: &[u8]) -> Option<Ftype> {
    match tag {
        b"soso" => Some(Ftype::Osos),
        b"crsr" => Some(Ftype::Rsrc),
        b"dpua" => Some(Ftype::Aupd),
        b"ebih" => Some(Ftype::Hibe),
        b"kbso" => Some(Ftype::Osbk),
        _ => None,
    }
}

/// Read the firmware directory and populate `ipod.images`, `ipod.osos_image`,
/// `ipod.diroffset` and `ipod.fwoffset`.
pub fn read_directory(ipod: &mut Ipod, dev: &mut dyn SeekRead) -> Result<(), String> {
    let sector_size = ipod.sector_size as usize;
    let mut sector = vec![0u8; sector_size];

    dev.seek(SeekFrom::Start(ipod.start))
        .map_err(|e| format!("seek to firmware partition: {e}"))?;
    dev.read_exact(&mut sector)
        .map_err(|e| format!("read firmware header: {e}"))?;

    if &sector[..APPLE_STOP_SIGN.len()] != APPLE_STOP_SIGN {
        return Err("firmware partition doesn't contain Apple copyright".into());
    }

    if &sector[0x100..0x104] != b"]ih[" {
        return Err("bad firmware directory".into());
    }

    let version = le16(&sector, 0x10a);
    if version != 2 && version != 3 {
        return Err(format!("unknown firmware format version {version:04x}"));
    }

    ipod.diroffset = le32(&sector, 0x104) as u64 + 0x200;

    // The directory may not be sector-aligned.
    let x = (ipod.diroffset % ipod.sector_size) as usize;

    dev.seek(SeekFrom::Start(ipod.start + ipod.diroffset - x as u64))
        .map_err(|e| format!("seek to directory: {e}"))?;
    dev.read_exact(&mut sector)
        .map_err(|e| format!("read directory: {e}"))?;

    let mut base = x;

    // 2nd-gen Nano stores the directory one sector later than the offset
    // suggests (p[0] == 0 signals the hack).
    if sector[base] == 0 {
        ipod.diroffset += ipod.sector_size - x as u64;
        dev.seek(SeekFrom::Start(ipod.start + ipod.diroffset))
            .map_err(|e| format!("seek to directory (nano2g): {e}"))?;
        dev.read_exact(&mut sector)
            .map_err(|e| format!("read directory (nano2g): {e}"))?;
        base = 0;
    }

    ipod.images.clear();
    ipod.osos_image = 0;

    let mut p = base;
    while ipod.images.len() < MAX_IMAGES && p < base + 400 {
        let marker = &sector[p..p + 4];
        if marker != b"!ATA" && marker != b"DNAN" {
            break;
        }
        p += 4;

        let ftype = match image_type(&sector[p..p + 4]) {
            Some(t) => t,
            None => return Err(format!("unknown image type {:?}", &sector[p..p + 4])),
        };
        p += 4;

        let info = ImageInfo {
            ftype,
            id: le32(&sector, p),
            dev_offset: le32(&sector, p + 4),
            len: le32(&sector, p + 8),
            addr: le32(&sector, p + 12),
            entry_offset: le32(&sector, p + 16),
            chksum: le32(&sector, p + 20),
            vers: le32(&sector, p + 24),
            load_addr: le32(&sector, p + 28),
        };

        if ftype == Ftype::Osos {
            ipod.osos_image = ipod.images.len();
        }

        ipod.images.push(info);
        p += 32;
    }

    if ipod.images.is_empty() {
        return Err("no images found in firmware directory".into());
    }

    if ipod.images[ipod.osos_image].ftype != Ftype::Osos {
        return Err("no OSOS image found".into());
    }

    if ipod.images.len() > 1 && version == 2 {
        // 3g firmware has no version field; make one up (never written back).
        ipod.fwoffset = ipod.start;
    } else {
        ipod.fwoffset = ipod.start + ipod.sector_size;
    }

    Ok(())
}
