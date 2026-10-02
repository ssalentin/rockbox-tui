//! Whole-firmware-partition backup and restore.

use crate::ipod::{round_up, Ipod, SeekRead, SeekReadWrite, BUFFER_SIZE};
use std::io::{Read, SeekFrom, Write};

/// Dump the entire firmware partition (partition 0) to `out`.
pub fn read_partition(ipod: &Ipod, dev: &mut dyn SeekRead, out: &mut dyn Write) -> Result<u64, String> {
    let total = ipod.pinfo[0].size as u64 * ipod.sector_size;
    let mut buf = vec![0u8; BUFFER_SIZE];
    let mut remaining = total;
    let mut pos: u64 = 0;

    dev.seek(SeekFrom::Start(ipod.start))
        .map_err(|e| format!("seek to partition: {e}"))?;

    while remaining > 0 {
        let chunk = remaining.min(BUFFER_SIZE as u64) as usize;
        dev.read_exact(&mut buf[..chunk])
            .map_err(|e| format!("read partition: {e}"))?;
        out.write_all(&buf[..chunk])
            .map_err(|e| format!("write output: {e}"))?;
        remaining -= chunk as u64;
        pos += chunk as u64;
    }

    Ok(pos)
}

/// Restore a firmware partition image (as produced by [`read_partition`]) from
/// `src` onto the device, padding the final sector if needed.
pub fn write_partition(ipod: &Ipod, dev: &mut dyn SeekReadWrite, src: &mut dyn Read) -> Result<u64, String> {
    let mut buf = vec![0u8; BUFFER_SIZE];
    let mut written: u64 = 0;

    dev.seek(SeekFrom::Start(ipod.start))
        .map_err(|e| format!("seek to partition: {e}"))?;

    loop {
        let n = src.read(&mut buf).map_err(|e| format!("read input: {e}"))?;
        if n == 0 {
            break;
        }

        let mut chunk = n;
        if n < BUFFER_SIZE {
            // EOF: pad the final chunk up to a whole sector.
            let padded = round_up(n as u64, ipod.sector_size) as usize;
            buf[n..padded].fill(0);
            chunk = padded;
        }

        dev.write_all(&buf[..chunk])
            .map_err(|e| format!("write partition: {e}"))?;
        written += chunk as u64;

        if n < BUFFER_SIZE {
            break;
        }
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipod::PartInfo;
    use std::io::Cursor;

    fn test_ipod() -> Ipod {
        let mut ipod = Ipod::new(512);
        ipod.start = 512;
        ipod.fwoffset = 1024;
        ipod.diroffset = 0x200;
        ipod.pinfo = [
            PartInfo { start: 1, size: 4, ptype: 0 },
            PartInfo { start: 5, size: 100, ptype: 0xb },
            PartInfo::default(),
            PartInfo::default(),
        ];
        ipod
    }

    #[test]
    fn round_trips_partition() {
        let ipod = test_ipod();
        let total = (ipod.pinfo[0].size as u64 * ipod.sector_size) as usize;

        // "device" = 512-byte MBR + 4-sector firmware partition
        let mut device = vec![0u8; 512 + total];
        device[512..512 + total].fill(0xAB);
        let mut dev = Cursor::new(device);

        let mut backup = Vec::new();
        let n = read_partition(&ipod, &mut dev, &mut backup).unwrap();
        assert_eq!(n, total as u64);
        assert!(backup.iter().all(|&b| b == 0xAB));

        // Restore onto a zeroed device (with a fresh MBR region untouched)
        let mut device2 = vec![0u8; 512 + total];
        let mut dev2 = Cursor::new(device2);
        let n = write_partition(&ipod, &mut dev2, &mut Cursor::new(backup.clone())).unwrap();
        assert_eq!(n, total as u64);
    }
}
