//! Bootloader installation/removal — the core of what Rockbox Utility does.

use crate::ipod::{le32, put_le32, round_up, Ipod, SeekReadWrite, BUFFER_SIZE};
use std::io::SeekFrom;

/// Append `bootloader` (the raw Rockbox bootloader binary) to the main OSOS
/// firmware image and update the firmware directory, mirroring
/// `ipodpatcher`'s `add_bootloader`.
pub fn add_bootloader(ipod: &Ipod, dev: &mut dyn SeekReadWrite, bootloader: &[u8]) -> Result<(), String> {
    // The 2nd-gen Nano (and the S5L87xx family generally) use a different
    // scheme; not implemented here.
    if ipod.model.map(|m| m.modelnum) == Some(62) {
        return Err("the 2nd-gen Nano uses a different bootloader scheme and is not supported yet".into());
    }

    let osos = ipod.images.get(ipod.osos_image).ok_or("no OSOS image")?;

    let entry_offset: u64 = if osos.entry_offset > 0 {
        osos.entry_offset as u64
    } else {
        round_up(osos.len as u64, ipod.sector_size)
    };

    let length = bootloader.len() as u64;
    let padded_length = round_up(length, ipod.sector_size);

    if entry_offset + padded_length > BUFFER_SIZE as u64 {
        return Err("bootloader too big for buffer".into());
    }

    // If the expanded OSOS would overlap the next image, shift images 2..n
    // forward to make room.
    let mut delta = 0u64;
    if ipod.images.len() > 1 {
        let image1 = ipod.images[1];
        if osos.dev_offset as u64 + entry_offset + padded_length > image1.dev_offset as u64 {
            delta = osos.dev_offset as u64 + entry_offset + padded_length
                - image1.dev_offset as u64
                + ipod.sector_size;
            diskmove(ipod, dev, delta)?;
        }
    }

    let fw_start = ipod.fwoffset + osos.dev_offset as u64;

    // Read the original OSOS firmware, then splice the bootloader on the end.
    let mut buf = vec![0u8; (entry_offset + padded_length) as usize];
    dev.seek(SeekFrom::Start(fw_start))
        .map_err(|e| format!("seek to firmware: {e}"))?;
    dev.read_exact(&mut buf[..entry_offset as usize])
        .map_err(|e| format!("read original firmware: {e}"))?;

    buf[entry_offset as usize..entry_offset as usize + length as usize]
        .copy_from_slice(bootloader);

    // 32-bit sum-of-bytes checksum over the combined image.
    let chksum = buf[..entry_offset as usize + length as usize]
        .iter()
        .map(|&b| b as u32)
        .fold(0u32, |a, b| a.wrapping_add(b));

    dev.seek(SeekFrom::Start(fw_start))
        .map_err(|e| format!("seek to firmware: {e}"))?;
    dev.write_all(&buf)
        .map_err(|e| format!("write firmware: {e}"))?;

    // Update the directory entry for the OSOS image.
    let (mut dir, base) = read_dir_sector(ipod, dev)?;
    let e = base + ipod.osos_image * 40;
    put_le32((entry_offset + length) as u32, &mut dir, e + 16); // len
    put_le32(entry_offset as u32, &mut dir, e + 24); // entryOffset
    put_le32(chksum, &mut dir, e + 28); // chksum
    put_le32(0xffff_ffff, &mut dir, e + 36); // loadAddr

    if delta > 0 {
        for i in 1..ipod.images.len() {
            let p = base + i * 40 + 12;
            let new = le32(&dir, p).wrapping_add(delta as u32);
            put_le32(new, &mut dir, p);
        }
    }

    write_dir_sector(ipod, dev, &dir)?;
    Ok(())
}

/// Remove a previously installed bootloader by restoring the OSOS `len`,
/// `entryOffset` and `chksum` fields (the appended bytes are simply no longer
/// referenced).
pub fn delete_bootloader(ipod: &Ipod, dev: &mut dyn SeekReadWrite) -> Result<(), String> {
    if ipod.model.map(|m| m.modelnum) == Some(62) {
        return Err("the 2nd-gen Nano uses a different bootloader scheme and is not supported yet".into());
    }

    let osos = ipod.images.get(ipod.osos_image).ok_or("no OSOS image")?;

    if osos.entry_offset == 0 {
        return Err("no bootloader found".into());
    }

    let length = osos.entry_offset as usize;

    // Re-read the original (pre-bootloader) firmware to recompute the checksum.
    let read_len = round_up(length as u64, ipod.sector_size) as usize;
    let mut buf = vec![0u8; read_len];
    let fw_start = ipod.fwoffset + osos.dev_offset as u64;
    dev.seek(SeekFrom::Start(fw_start))
        .map_err(|e| format!("seek to firmware: {e}"))?;
    dev.read_exact(&mut buf)
        .map_err(|e| format!("read firmware: {e}"))?;

    let chksum = buf[..length].iter().map(|&b| b as u32).fold(0u32, |a, b| a.wrapping_add(b));

    let (mut dir, base) = read_dir_sector(ipod, dev)?;
    let e = base + ipod.osos_image * 40;
    put_le32(length as u32, &mut dir, e + 16); // len
    put_le32(0, &mut dir, e + 24); // entryOffset
    put_le32(chksum, &mut dir, e + 28); // chksum

    write_dir_sector(ipod, dev, &dir)?;
    Ok(())
}

/// Copy images 2..n forward by `delta` bytes (working backwards to avoid
/// clobbering the source region). Offsets are relative to the start of the
/// firmware partition, exactly as in `ipodpatcher.c`.
fn diskmove(ipod: &Ipod, dev: &mut dyn SeekReadWrite, delta: u64) -> Result<(), String> {
    let src_start = ipod.images[1].dev_offset as u64;
    let last = ipod.images[ipod.images.len() - 1];
    let src_end = round_up(
        last.dev_offset as u64 + ipod.sector_size + last.len as u64,
        ipod.sector_size,
    );
    let mut bytesleft = src_end - src_start;
    let mut end = src_end;

    let mut buf = vec![0u8; BUFFER_SIZE];

    while bytesleft > 0 {
        let chunk = bytesleft.min(BUFFER_SIZE as u64) as usize;
        let read_pos = ipod.start + end - chunk as u64;

        dev.seek(SeekFrom::Start(read_pos))
            .map_err(|e| format!("diskmove seek (read): {e}"))?;
        dev.read_exact(&mut buf[..chunk])
            .map_err(|e| format!("diskmove read: {e}"))?;

        dev.seek(SeekFrom::Start(read_pos + delta))
            .map_err(|e| format!("diskmove seek (write): {e}"))?;
        dev.write_all(&buf[..chunk])
            .map_err(|e| format!("diskmove write: {e}"))?;

        end -= chunk as u64;
        bytesleft -= chunk as u64;
    }

    Ok(())
}

/// Read the firmware directory sector, returning the sector contents plus the
/// byte offset (`base`) of the first directory entry within it.
fn read_dir_sector(ipod: &Ipod, dev: &mut dyn SeekReadWrite) -> Result<(Vec<u8>, usize), String> {
    let base = (ipod.diroffset % ipod.sector_size) as usize;
    let mut sector = vec![0u8; ipod.sector_size as usize];

    dev.seek(SeekFrom::Start(ipod.start + ipod.diroffset - base as u64))
        .map_err(|e| format!("seek to directory: {e}"))?;
    dev.read_exact(&mut sector)
        .map_err(|e| format!("read directory: {e}"))?;

    // The 2nd-gen Nano stores the directory one sector later.
    if sector[base] == 0 {
        dev.seek(SeekFrom::Start(ipod.start + ipod.diroffset - base as u64 + ipod.sector_size))
            .map_err(|e| format!("seek to directory (nano2g): {e}"))?;
        dev.read_exact(&mut sector)
            .map_err(|e| format!("read directory (nano2g): {e}"))?;
        return Ok((sector, 0));
    }

    Ok((sector, base))
}

fn write_dir_sector(ipod: &Ipod, dev: &mut dyn SeekReadWrite, sector: &[u8]) -> Result<(), String> {
    let base = (ipod.diroffset % ipod.sector_size) as usize;
    dev.seek(SeekFrom::Start(ipod.start + ipod.diroffset - base as u64))
        .map_err(|e| format!("seek to directory: {e}"))?;
    dev.write_all(sector)
        .map_err(|e| format!("write directory: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipod::{get_model, le16, le32, put_le16, put_le32, read_directory, read_partinfo, Ipod, PartInfo, APPLE_STOP_SIGN};
    use std::io::Cursor;

    fn put_dir_entry(img: &mut [u8], off: usize, ftype: &[u8; 4], dev_offset: u32, len: u32, vers: u32) {
        img[off..off + 4].copy_from_slice(b"!ATA");
        img[off + 4..off + 8].copy_from_slice(ftype);
        put_le32(0, img, off + 8);
        put_le32(dev_offset, img, off + 12);
        put_le32(len, img, off + 16);
        put_le32(0, img, off + 20);
        put_le32(0, img, off + 24);
        put_le32(0, img, off + 28);
        put_le32(vers, img, off + 32);
        put_le32(0, img, off + 36);
    }

    /// Build a synthetic winpod disk image: an MBR, a firmware partition with
    /// a header + directory (OSOS + RSRC), and a data partition.
    fn synthetic_image() -> (Vec<u8>, Ipod) {
        const SECTOR: usize = 512;
        let mut img = vec![0u8; 8192];

        // --- MBR (sector 0) ---
        put_le32(0, &mut img, 0x1be + 4); // partition 1: type 0 (firmware)
        put_le32(1, &mut img, 0x1be + 8);
        put_le32(8, &mut img, 0x1be + 12);
        put_le32(0xb, &mut img, 0x1be + 16 + 4); // partition 2: FAT32 data
        put_le32(9, &mut img, 0x1be + 16 + 8);
        put_le32(7, &mut img, 0x1be + 16 + 12);
        img[510] = 0x55;
        img[511] = 0xaa;

        // --- firmware partition (LBA 1) ---
        let start = SECTOR;
        img[start..start + 256].copy_from_slice(APPLE_STOP_SIGN);
        img[start + 0x100..start + 0x104].copy_from_slice(b"]ih[");
        put_le32(0, &mut img, start + 0x104); // diroffset = 0 + 0x200
        put_le16(3, &mut img, start + 0x10a); // version 3

        // directory at start + 0x200
        let dir = start + 0x200;
        put_dir_entry(&mut img, dir, b"soso", 0x400, 256, 0x0000_b000); // OSOS
        put_dir_entry(&mut img, dir + 40, b"crsr", 0x800, 256, 0); // RSRC

        // OSOS data at fwoffset(1024) + 0x400 = 2048
        img[2048..2048 + 256].fill(0x11);
        // RSRC data at fwoffset(1024) + 0x800 = 3072
        img[3072..3072 + 256].fill(0x22);

        // Parse it.
        let mut ipod = Ipod::new(SECTOR as u64);
        let mut dev = Cursor::new(img.clone());
        read_partinfo(&mut ipod, &mut dev).unwrap();
        read_directory(&mut ipod, &mut dev).unwrap();
        let version = ipod.images[ipod.osos_image].vers >> 8;
        ipod.model = Some(get_model(version).unwrap());

        (img, ipod)
    }

    #[test]
    fn parses_synthetic_firmware() {
        let (_img, ipod) = synthetic_image();
        assert_eq!(ipod.model.unwrap().modelnum, 5);
        assert_eq!(ipod.build_target(), Some("ipodvideo"));
        assert_eq!(ipod.images.len(), 2);
        assert_eq!(ipod.osos_image, 0);
        assert_eq!(ipod.fwoffset, 1024);
    }

    #[test]
    fn installs_bootloader_round_trip() {
        let (mut img, ipod) = synthetic_image();
        let bootloader = [0xAAu8, 0xBB, 0xCC];

        {
            let mut dev = Cursor::new(&mut img);
            add_bootloader(&ipod, &mut dev, &bootloader).unwrap();
        }

        // The bootloader must be appended at fwoffset + entry_offset.
        let fw_start = ipod.fwoffset + ipod.images[0].dev_offset as u64;
        let entry_offset = 512; // round_up(256, 512)
        assert_eq!(&img[fw_start as usize..fw_start as usize + 256], &[0x11; 256]);
        assert_eq!(&img[fw_start as usize + entry_offset..fw_start as usize + entry_offset + 3], &bootloader);

        // Directory entry 0 (len, entryOffset, chksum, loadAddr) is updated.
        let base = (ipod.start + ipod.diroffset) as usize;
        let e = base;
        assert_eq!(le32(&img, e + 16), 512 + 3); // len
        assert_eq!(le32(&img, e + 24), 512); // entryOffset
        assert_eq!(le32(&img, e + 36), 0xffff_ffff); // loadAddr
        // checksum = sum of original 256 bytes (0x11) + bootloader
        let expect = 256u32 * 0x11 + (0xAA + 0xBB + 0xCC) as u32;
        assert_eq!(le32(&img, e + 28), expect);
    }

    #[test]
    fn delete_bootloader_restores_entry() {
        let (mut img, ipod) = synthetic_image();
        {
            let mut dev = Cursor::new(&mut img);
            add_bootloader(&ipod, &mut dev, &[0xAA, 0xBB, 0xCC]).unwrap();
        }
        // Refresh the in-memory view of entryOffset so delete sees it.
        let mut ipod2 = ipod.clone();
        ipod2.images[ipod2.osos_image].entry_offset = 512;
        ipod2.images[ipod2.osos_image].len = 512 + 3;

        {
            let mut dev = Cursor::new(&mut img);
            delete_bootloader(&ipod2, &mut dev).unwrap();
        }

        let base = (ipod.start + ipod.diroffset) as usize;
        assert_eq!(le32(&img, base + 24), 0); // entryOffset cleared
        assert_eq!(le32(&img, base + 16), 512); // len = entryOffset (original)
    }
}
