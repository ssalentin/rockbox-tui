//! Core logic for reading and patching an iPod's hidden firmware partition,
//! ported from Rockbox's `ipodpatcher` (Dave Chapman et al., GPLv2).
//!
//! This module is deliberately independent of any OS-specific I/O: all device
//! access goes through [`Read`], [`Write`] and [`Seek`] so the parsing and
//! patching routines can be unit-tested against an in-memory firmware image.

mod directory;
mod install;
mod partition;
mod transfer;

pub mod io;
pub mod scsi;

use std::io::{Read, Seek, Write};

pub use directory::{read_directory, ImageInfo};
#[allow(unused_imports)] // re-exported so `ImageInfo::ftype` is publicly nameable
pub use directory::Ftype;
pub use install::{add_bootloader, delete_bootloader};
pub use partition::{read_partinfo, PartInfo};
pub use transfer::{read_partition, write_partition};

/// A handle that can only be read and seeked (used for scanning).
pub trait SeekRead: Read + Seek {}
impl<T: Read + Seek + ?Sized> SeekRead for T {}

/// A handle that can be read, written and seeked (used for patching).
pub trait SeekReadWrite: Read + Write + Seek {}
impl<T: Read + Write + Seek + ?Sized> SeekReadWrite for T {}

/// Maximum number of images we expect in a firmware directory.
pub const MAX_IMAGES: usize = 10;

/// Size of the I/O buffer used for whole-partition copies, in bytes.
pub const BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// A fake partition type — DOS partition tables can't express HFS partitions.
pub const PARTTYPE_HFS: u32 = 0xffff;

/// Model identification for an iPod discovered on the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Rockbox internal model number (matches the `.ipod` checksum seed).
    pub modelnum: u32,
    /// Short model tag used in `.ipod`/firmware filenames (e.g. `ipvd`).
    pub modelname: &'static str,
    /// Human readable description.
    pub modelstr: &'static str,
    /// Rockbox build target name (e.g. `ipodvideo`).
    pub targetname: &'static str,
}

/// Maps the high byte of the OSOS `vers` field to a known iPod model.
pub fn get_model(version: u32) -> Option<Model> {
    Some(match version {
        0x01 => Model {
            modelnum: 19,
            modelname: "1g2g",
            modelstr: "1st or 2nd Generation",
            targetname: "ipod1g2g",
        },
        0x02 => Model {
            modelnum: 7,
            modelname: "ip3g",
            modelstr: "3rd Generation",
            targetname: "ipod3g",
        },
        0x40 => Model {
            modelnum: 9,
            modelname: "mini",
            modelstr: "1st Generation Mini",
            targetname: "ipodmini1g",
        },
        0x50 => Model {
            modelnum: 8,
            modelname: "ip4g",
            modelstr: "4th Generation",
            targetname: "ipod4gray",
        },
        0x60 => Model {
            modelnum: 3,
            modelname: "ipco",
            modelstr: "Photo/Color",
            targetname: "ipodcolor",
        },
        0x70 => Model {
            modelnum: 11,
            modelname: "mn2g",
            modelstr: "2nd Generation Mini",
            targetname: "ipodmini2g",
        },
        0xc0 => Model {
            modelnum: 4,
            modelname: "nano",
            modelstr: "1st Generation Nano",
            targetname: "ipodnano1g",
        },
        0xb0 => Model {
            modelnum: 5,
            modelname: "ipvd",
            modelstr: "Video (aka 5th Generation)",
            targetname: "ipodvideo",
        },
        0x100 => Model {
            modelnum: 62,
            modelname: "nn2x",
            modelstr: "2nd Generation Nano",
            targetname: "ipodnano2g",
        },
        _ => return None,
    })
}

/// A detected iPod, fully described after scanning + directory parsing.
#[derive(Debug, Clone)]
pub struct Ipod {
    /// Device sector size in bytes (almost always 512).
    pub sector_size: u64,
    /// Absolute byte offset of the firmware partition within the disk.
    pub start: u64,
    /// Absolute byte offset of the first firmware image.
    pub fwoffset: u64,
    /// Offset (within the firmware partition) of the image directory.
    pub diroffset: u64,
    /// The first four partitions from the MBR / APM.
    pub pinfo: [PartInfo; 4],
    /// Images found in the firmware directory.
    pub images: Vec<ImageInfo>,
    /// Index into `images` of the main firmware (OSOS).
    pub osos_image: usize,
    /// True if the disk uses an Apple Partition Map rather than a DOS MBR.
    pub macpod: bool,
    /// Detected model (unknown until [`crate::ipod::get_model`] is called).
    pub model: Option<Model>,
    /// Amount of RAM in MiB, if it could be read (0 = unknown).
    pub ramsize_mb: u32,
}

impl Ipod {
    /// A blank detector, with only the sector size filled in.
    pub fn new(sector_size: u64) -> Ipod {
        Ipod {
            sector_size,
            start: 0,
            fwoffset: 0,
            diroffset: 0,
            pinfo: [PartInfo::default(); 4],
            images: Vec::new(),
            osos_image: 0,
            macpod: false,
            model: None,
            ramsize_mb: 0,
        }
    }

    /// The Rockbox build target name to use, taking the 64 MiB RAM
    /// `ipodvideo64mb` variant into account for the 5th-gen Video.
    pub fn build_target(&self) -> Option<&'static str> {
        let model = self.model?;
        if model.modelnum == 5 && self.ramsize_mb >= 64 {
            Some("ipodvideo64mb")
        } else {
            Some(model.targetname)
        }
    }
}

#[inline]
pub(crate) fn le16(buf: &[u8], pos: usize) -> u16 {
    (buf[pos] as u16) | ((buf[pos + 1] as u16) << 8)
}

#[inline]
pub(crate) fn le32(buf: &[u8], pos: usize) -> u32 {
    (buf[pos] as u32)
        | ((buf[pos + 1] as u32) << 8)
        | ((buf[pos + 2] as u32) << 16)
        | ((buf[pos + 3] as u32) << 24)
}

#[inline]
pub(crate) fn be32(buf: &[u8], pos: usize) -> u32 {
    ((buf[pos] as u32) << 24)
        | ((buf[pos + 1] as u32) << 16)
        | ((buf[pos + 2] as u32) << 8)
        | (buf[pos + 3] as u32)
}

#[inline]
#[allow(dead_code)] // used by tests and future image-writers
pub(crate) fn put_le16(val: u16, buf: &mut [u8], pos: usize) {
    buf[pos] = (val & 0xff) as u8;
    buf[pos + 1] = (val >> 8) as u8;
}

#[inline]
pub(crate) fn put_le32(val: u32, buf: &mut [u8], pos: usize) {
    buf[pos] = (val & 0xff) as u8;
    buf[pos + 1] = ((val >> 8) & 0xff) as u8;
    buf[pos + 2] = ((val >> 16) & 0xff) as u8;
    buf[pos + 3] = ((val >> 24) & 0xff) as u8;
}

#[inline]
#[allow(dead_code)] // used by tests and future image-writers
pub(crate) fn put_be32(val: u32, buf: &mut [u8], pos: usize) {
    buf[pos] = ((val >> 24) & 0xff) as u8;
    buf[pos + 1] = ((val >> 16) & 0xff) as u8;
    buf[pos + 2] = ((val >> 8) & 0xff) as u8;
    buf[pos + 3] = (val & 0xff) as u8;
}

/// Round `n` up to a multiple of `align`.
#[inline]
pub(crate) fn round_up(n: u64, align: u64) -> u64 {
    (n + align - 1) & !(align - 1)
}

/// The ASCII "stop sign" that appears at the start of the Apple firmware
/// partition. Reproduced byte-for-byte from `ipodpatcher.c` (255 bytes,
/// null-terminated on disk).
pub(crate) const APPLE_STOP_SIGN: &[u8] = br#"{{~~  /-----\   {{~~ /       \  {{~~|         | {{~~| S T O P | {{~~|         | {{~~ \       /  {{~~  \-----/   Copyright(C) 2001 Apple Computer, Inc.---------------------------------------------------------------------------------------------------------"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_sign_is_255_bytes() {
        assert_eq!(APPLE_STOP_SIGN.len(), 255);
        assert_eq!(&APPLE_STOP_SIGN[..4], b"{{~~");
        assert_eq!(&APPLE_STOP_SIGN[0x70..0x74], b"Copy");
    }

    #[test]
    fn model_mapping() {
        let m = get_model(0xb0).unwrap();
        assert_eq!(m.modelnum, 5);
        assert_eq!(m.modelname, "ipvd");
        assert_eq!(m.targetname, "ipodvideo");

        let m = get_model(0x100).unwrap();
        assert_eq!(m.modelnum, 62);

        assert!(get_model(0xff).is_none());
    }

    #[test]
    fn byte_helpers() {
        let mut b = [0u8; 8];
        put_le32(0xdead_beef, &mut b, 0);
        assert_eq!(le32(&b, 0), 0xdead_beef);
        put_be32(0x0102_0304, &mut b, 4);
        assert_eq!(be32(&b, 4), 0x0102_0304);
        put_le16(0xaabb, &mut b, 0);
        assert_eq!(le16(&b, 0), 0xaabb);
    }
}
