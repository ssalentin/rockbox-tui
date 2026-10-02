//! SCSI INQUIRY via `SG_IO` — used to read the iPod's XML device info and
//! determine RAM size (needed to pick `ipodvideo64mb` vs `ipodvideo`).

use crate::ipod::io::Device;

const SG_IO: libc::c_ulong = 0x2285;
const SG_DXFER_FROM_DEV: libc::c_int = -3;
const SG_FLAG_LUN_INHIBIT: libc::c_uint = 2;

/// Mirrors `struct sg_io_hdr` from `<scsi/sg.h>`.
#[repr(C)]
#[derive(Default)]
struct SgIoHdr {
    interface_id: libc::c_int,
    dxfer_direction: libc::c_int,
    cmd_len: libc::c_uchar,
    mx_sb_len: libc::c_uchar,
    iovec_count: libc::c_ushort,
    dxfer_len: libc::c_uint,
    dxferp: *mut libc::c_void,
    cmdp: *mut libc::c_uchar,
    sbp: *mut libc::c_uchar,
    timeout: libc::c_uint,
    flags: libc::c_uint,
    pack_id: libc::c_int,
    usr_ptr: *mut libc::c_void,
    status: libc::c_uchar,
    masked_status: libc::c_uchar,
    msg_status: libc::c_uchar,
    sb_len_wr: libc::c_uchar,
    host_status: libc::c_ushort,
    driver_status: libc::c_ushort,
    resid: libc::c_int,
    duration: libc::c_uint,
    info: libc::c_uint,
}

/// Issue an INQUIRY (EVPD) command for `page_code` and return the result.
fn inquiry(dev: &Device, page_code: u8, buf: &mut [u8]) -> Result<(), String> {
    let mut cdb = [0u8; 6];
    cdb[0] = 0x12; // INQUIRY
    cdb[1] = 1; // EVPD
    cdb[2] = page_code;
    cdb[3] = 0;
    cdb[4] = 0xff;
    cdb[5] = 0;

    let mut sense = [0u8; 255];
    let mut hdr = SgIoHdr {
        interface_id: 'S' as libc::c_int,
        flags: SG_FLAG_LUN_INHIBIT,
        dxferp: buf.as_mut_ptr() as *mut libc::c_void,
        dxfer_len: buf.len() as libc::c_uint,
        sbp: sense.as_mut_ptr() as *mut libc::c_uchar,
        mx_sb_len: sense.len() as libc::c_uchar,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmdp: cdb.as_mut_ptr() as *mut libc::c_uchar,
        cmd_len: 6,
        timeout: 10_000,
        ..Default::default()
    };

    // SAFETY: the kernel reads `hdr` and writes up to `mx_sb_len` bytes into
    // `sense`, and up to `dxfer_len` bytes into `buf`. All pointers stay valid
    // for the duration of the call.
    let rc = unsafe { libc::ioctl(dev.raw_fd(), SG_IO, &mut hdr) };
    if rc < 0 {
        return Err(format!("SG_IO failed: {}", std::io::Error::last_os_error()));
    }
    if hdr.status != 0 || hdr.host_status != 0 || hdr.driver_status != 0 {
        return Err(format!(
            "SG_IO error: status={} host={} driver={}",
            hdr.status, hdr.host_status, hdr.driver_status
        ));
    }
    Ok(())
}

/// Read the device-information XML and extract the RAM size in MiB.
/// Returns `None` if the information is unavailable (e.g. no raw-io
/// permission).
pub fn read_ramsize(dev: &Device) -> Option<u32> {
    let xml = read_xmlinfo(dev).ok()?;
    parse_ramsize(&xml)
}

fn read_xmlinfo(dev: &Device) -> Result<String, String> {
    // Page 0xC0 returns the list of available information pages.
    let mut hdr = [0u8; 255];
    inquiry(dev, 0xc0, &mut hdr)?;
    let npages = hdr[3] as usize;

    let mut xml = Vec::new();
    let mut buf = [0u8; 255];
    for i in 0..npages {
        let page = hdr[i + 4];
        inquiry(dev, page, &mut buf)?;
        let len = buf[3] as usize;
        if len > buf.len() - 4 {
            return Err("truncated xml info page".into());
        }
        xml.extend_from_slice(&buf[4..4 + len]);
    }
    String::from_utf8(xml).map_err(|_| "xml info was not valid utf-8".into())
}

fn parse_ramsize(xml: &str) -> Option<u32> {
    let needle = "<key>RAM</key>\n<integer>";
    let p = xml.find(needle)?;
    let rest = &xml[p + needle.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ramsize_from_xml() {
        let xml = r#"<dict>
<key>RAM</key>
<integer>64</integer>
<key>Other</key>
<integer>1</integer>
</dict>"#;
        assert_eq!(parse_ramsize(xml), Some(64));
    }

    #[test]
    fn handles_missing_ram() {
        assert_eq!(parse_ramsize("<dict/>"), None);
    }
}
