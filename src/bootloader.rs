//! Rockbox bootloader handling.
//!
//! The bootloader is a small ARM binary that the Apple bootloader jumps to
//! (via the OSOS `entryOffset`). Rockbox distributes it as a `.ipod` file:
//! an 8-byte header (4-byte big-endian checksum + 4-byte model tag) followed
//! by the raw binary. [`add_bootloader`](crate::ipod::add_bootloader) expects
//! the raw binary, so the header is stripped here.

use std::fs;
use std::path::Path;

/// The bundled default bootloader: [ipodloader2](https://github.com/crozone/ipodloader2)
/// (GPLv2), a dual-boot loader for the classic iPod line (1g–5.5g, Mini 1g,
/// Nano 1g). A user-provided `--bootloader` file overrides this.
const BUNDLED_BOOTLOADER: &[u8] = include_bytes!("bootloader_ipod.bin");

/// Load a bootloader, returning the raw binary (header stripped).
///
/// If `path` is `None`, the bundled bootloader is used. The `.ipod` header is
/// detected and stripped automatically; a bare `.bin`/`loader.bin` is passed
/// through unchanged.
pub fn load(path: Option<&Path>) -> Result<Vec<u8>, String> {
    let bytes = match path {
        Some(p) => fs::read(p).map_err(|e| format!("read bootloader {}: {e}", p.display()))?,
        None => BUNDLED_BOOTLOADER.to_vec(),
    };

    // Strip an 8-byte ".ipod" header if present: the header is 4 bytes of
    // big-endian checksum followed by a 4-byte model tag, and the rest of the
    // file is not a header (so it's "ipvd"/"ipco"/... ASCII in bytes 4..8).
    if bytes.len() > 8 {
        let tag = &bytes[4..8];
        let looks_like_ipod_header = tag.iter().all(u8::is_ascii_lowercase);
        if looks_like_ipod_header {
            return Ok(bytes[8..].to_vec());
        }
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn bundled_bootloader_is_present() {
        assert!(BUNDLED_BOOTLOADER.len() > 1024, "bundled bootloader missing or too small");
    }

    #[test]
    fn strips_ipod_header() {
        let dir = std::env::temp_dir();
        let p = dir.join("rockbox-tui-test-bootloader.ipod");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(&[0x00, 0x01, 0x02, 0x03]).unwrap(); // checksum
        f.write_all(b"ipvd").unwrap(); // model tag
        f.write_all(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap(); // raw binary
        drop(f);

        let bytes = load(Some(&p)).unwrap();
        assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn passes_bare_bin_through() {
        let dir = std::env::temp_dir();
        let p = dir.join("rockbox-tui-test-loader.bin");
        fs::write(&p, [0xAA, 0xBB, 0xCC]).unwrap();
        let bytes = load(Some(&p)).unwrap();
        assert_eq!(bytes, vec![0xAA, 0xBB, 0xCC]);
        let _ = fs::remove_file(&p);
    }
}
