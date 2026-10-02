//! Partition-table parsing (DOS MBR and Apple Partition Map).

use crate::ipod::{be32, le32, Ipod, SeekRead, PARTTYPE_HFS};
use std::io::SeekFrom;

/// One entry of a DOS/APM partition table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartInfo {
    pub start: u32,
    pub size: u32,
    pub ptype: u32,
}

/// Parse the partition table at the start of the device and populate
/// `ipod.pinfo` / `ipod.start`.
pub fn read_partinfo(ipod: &mut Ipod, dev: &mut dyn SeekRead) -> Result<(), String> {
    let mut sector = vec![0u8; ipod.sector_size as usize];

    dev.seek(SeekFrom::Start(0)).map_err(|e| format!("seek: {e}"))?;
    dev.read_exact(&mut sector)
        .map_err(|e| format!("read partition table: {e}"))?;

    ipod.pinfo = [PartInfo::default(); 4];

    if sector[510] == 0x55 && sector[511] == 0xaa {
        ipod.macpod = false;
        for (i, slot) in ipod.pinfo.iter_mut().enumerate() {
            let p = 0x1be + 16 * i;
            slot.ptype = sector[p + 4] as u32;
            slot.start = le32(&sector, p + 8);
            slot.size = le32(&sector, p + 12);
        }
    } else if sector[0] == b'E' && sector[1] == b'R' {
        ipod.macpod = true;
        parse_apm(ipod, dev, &sector)?;
    } else {
        return Err("bad boot sector signature".into());
    }

    // A real iPod: partition 1 is type 0 (the firmware partition) but has a
    // non-zero size, and partition 2 is a data partition (winpod 0xb/0xc or
    // macpod HFS).
    if ipod.pinfo[0].ptype != 0
        || ipod.pinfo[0].size == 0
        || (ipod.pinfo[1].ptype != 0xb
            && ipod.pinfo[1].ptype != 0xc
            && ipod.pinfo[1].ptype != PARTTYPE_HFS)
    {
        return Err("partition layout is not an ipod".into());
    }

    ipod.start = ipod.pinfo[0].start as u64 * ipod.sector_size;
    Ok(())
}

fn parse_apm(ipod: &mut Ipod, dev: &mut dyn SeekRead, ddm: &[u8]) -> Result<(), String> {
    // `ddm[2]` is the high byte of the 16-bit block size (BE). For 512-byte
    // blocks this is 2, so `part_blk_siz_mul` is 1 (i.e. one 512-byte block
    // per map entry). For 2048-byte blocks it is 8 -> 4 (i.e. 4*512 bytes).
    let part_blk_siz_mul = (ddm[2] / 2) as u64;

    let mut blk_no: u64 = 1;
    let mut part_blk_count: u64 = 1;
    let mut i = 0usize;

    let mut sector = vec![0u8; ipod.sector_size as usize];

    while blk_no <= part_blk_count && i < 4 {
        let pos = blk_no * part_blk_siz_mul * 512;
        dev.seek(SeekFrom::Start(pos))
            .map_err(|e| format!("seek to APM entry: {e}"))?;
        dev.read_exact(&mut sector)
            .map_err(|e| format!("read APM entry: {e}"))?;

        if sector[0] != b'P' || sector[1] != b'M' {
            break;
        }

        let pm_map_blk_cnt = be32(&sector, 4);
        let pm_py_part_start = be32(&sector, 8);
        let pm_part_blk_cnt = be32(&sector, 12);

        part_blk_count = pm_map_blk_cnt as u64;

        let name = &sector[48..48 + 32];
        if name.starts_with(b"Apple_MDFW") {
            ipod.pinfo[i] = PartInfo {
                start: pm_py_part_start,
                size: pm_part_blk_cnt,
                ptype: 0,
            };
            i += 1;
        } else if name.starts_with(b"Apple_HFS") {
            ipod.pinfo[i] = PartInfo {
                start: pm_py_part_start,
                size: pm_part_blk_cnt,
                ptype: PARTTYPE_HFS,
            };
            i += 1;
        }

        blk_no += 1;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn ipod_with_sector_size(ss: u64) -> Ipod {
        Ipod::new(ss)
    }

    #[test]
    fn parses_dos_mbr_winpod() {
        let mut ipod = ipod_with_sector_size(512);
        let mut img = vec![0u8; 512];
        // partition 1: type 0, start 1, size 100
        crate::ipod::put_le32(0x0000_0000, &mut img, 0x1be + 4);
        crate::ipod::put_le32(1, &mut img, 0x1be + 8);
        crate::ipod::put_le32(100, &mut img, 0x1be + 12);
        // partition 2: type 0xb
        crate::ipod::put_le32(0xb, &mut img, 0x1be + 16 + 4);
        crate::ipod::put_le32(101, &mut img, 0x1be + 16 + 8);
        crate::ipod::put_le32(900000, &mut img, 0x1be + 16 + 12);
        img[510] = 0x55;
        img[511] = 0xaa;

        let mut dev = Cursor::new(img);
        read_partinfo(&mut ipod, &mut dev).unwrap();
        assert!(!ipod.macpod);
        assert_eq!(ipod.pinfo[0].start, 1);
        assert_eq!(ipod.start, 512);
        assert_eq!(ipod.pinfo[1].ptype, 0xb);
    }

    #[test]
    fn rejects_non_ipod_layout() {
        let mut ipod = ipod_with_sector_size(512);
        let mut img = vec![0u8; 512];
        img[510] = 0x55;
        img[511] = 0xaa;
        let mut dev = Cursor::new(img);
        assert!(read_partinfo(&mut ipod, &mut dev).is_err());
    }

    #[test]
    fn parses_apple_partition_map() {
        let mut ipod = ipod_with_sector_size(512);

        // Disk layout: block 0 = driver descriptor, blocks 1..2 = partition map.
        let mut img = vec![0u8; 3 * 512];
        // Driver descriptor map.
        img[0] = b'E';
        img[1] = b'R';
        img[2] = 2; // high byte of 512 -> block size 512
        img[3] = 0;

        // Helper to write a PM entry at block `blk`.
        let mut pm = |blk: usize, name: &[u8], start: u32, size: u32| {
            let p = blk * 512;
            img[p] = b'P';
            img[p + 1] = b'M';
            crate::ipod::put_be32(2, &mut img, p + 4); // map block count
            crate::ipod::put_be32(start, &mut img, p + 8);
            crate::ipod::put_be32(size, &mut img, p + 12);
            img[p + 48..p + 48 + 32].fill(b'A');
            img[p + 48..p + 48 + name.len()].copy_from_slice(name);
        };
        pm(1, b"Apple_MDFW", 2, 100);
        pm(2, b"Apple_HFS", 102, 900000);

        let mut dev = Cursor::new(img);
        read_partinfo(&mut ipod, &mut dev).unwrap();
        assert!(ipod.macpod);
        assert_eq!(ipod.pinfo[0].start, 2);
        assert_eq!(ipod.pinfo[0].ptype, 0);
        assert_eq!(ipod.pinfo[1].ptype, PARTTYPE_HFS);
        assert_eq!(ipod.start, 1024);
    }
}
