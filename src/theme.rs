//! Rockbox theme (and associated assets) installation.
//!
//! Themes are distributed as zip archives from themes.rockbox.org. An archive
//! contains the `.cfg` theme file plus whatever it needs (fonts, backdrops,
//! icons, WPS/SBS files). Files are placed under the device's `.rockbox/`
//! directory in the correct subfolder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Subfolders of `.rockbox/` that a theme archive may already reference by
/// path (in which case the archive's own path is preserved).
const KNOWN_DIRS: &[&str] = &[
    "themes", "fonts", "wps", "sbs", "backdrops", "icons", "langs", "eqs", "rocks", "codecs",
    "recpresets", "fms", "playlist_catalog", "sounds",
];

/// Map a single archive entry to its destination inside `.rockbox/`.
/// Returns `None` for entries that should be skipped (directories, etc.).
pub fn map_entry(rel: &Path) -> Option<PathBuf> {
    let mut components = rel.components();
    let first = components.next()?.as_os_str().to_str()?;

    // If the archive is already laid out relative to `.rockbox/` (e.g.
    // `themes/foo.cfg`, `fonts/bar.fnt`), keep the path as-is.
    if KNOWN_DIRS.contains(&first) {
        return Some(rel.to_path_buf());
    }

    let ext = rel.extension().and_then(|e| e.to_str())?;
    let dir = match ext {
        "cfg" => "themes",
        "fnt" => "fonts",
        "wps" => "wps",
        "sbs" => "sbs",
        "bmp" | "jpg" | "jpeg" | "png" => "backdrops",
        "icons" => "icons",
        "voice" => "langs",
        _ => return Some(rel.to_path_buf()),
    };
    let name = rel.file_name()?;
    Some(PathBuf::from(dir).join(name))
}

/// Install a theme archive onto the device, returning the list of files
/// written (relative to `.rockbox/`). Accepts any readable+seekable source
/// (a zip file, or an in-memory buffer).
pub fn install<R: io::Read + io::Seek>(reader: R, mount: &Path) -> Result<Vec<PathBuf>, String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("open zip: {e}"))?;

    let rockbox = mount.join(".rockbox");
    fs::create_dir_all(&rockbox).map_err(|e| format!("create {}: {e}", rockbox.display()))?;

    let mut installed = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if entry.is_dir() {
            continue;
        }

        let Some(dest) = map_entry(&rel) else { continue };
        let out = rockbox.join(&dest);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let mut outfile = fs::File::create(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
        io::copy(&mut entry, &mut outfile).map_err(|e| format!("extract {}: {e}", out.display()))?;
        installed.push(dest);
    }

    Ok(installed)
}

/// Install a theme archive from a zip file on disk.
pub fn install_zip(zip_path: &Path, mount: &Path) -> Result<Vec<PathBuf>, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("open {}: {e}", zip_path.display()))?;
    install(file, mount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn maps_known_dir_paths_through() {
        assert_eq!(map_entry(Path::new("themes/foo.cfg")), Some(PathBuf::from("themes/foo.cfg")));
        assert_eq!(map_entry(Path::new("fonts/12-Adobe-Helvetica.fnt")), Some(PathBuf::from("fonts/12-Adobe-Helvetica.fnt")));
    }

    #[test]
    fn maps_by_extension() {
        assert_eq!(map_entry(Path::new("foo.cfg")), Some(PathBuf::from("themes/foo.cfg")));
        assert_eq!(map_entry(Path::new("font.fnt")), Some(PathBuf::from("fonts/font.fnt")));
        assert_eq!(map_entry(Path::new("pic.bmp")), Some(PathBuf::from("backdrops/pic.bmp")));
    }

    #[test]
    fn builds_and_extracts_theme_zip() {
        let dir = std::env::temp_dir().join("rockbox-tui-theme-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("theme.zip");
        let mut zw = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("themes/MyTheme.cfg", opts).unwrap();
        zw.write_all(b"# Name: MyTheme\n").unwrap();
        zw.start_file("fonts/12-Test.fnt", opts).unwrap();
        zw.write_all(&[0x11, 0x22, 0x33]).unwrap();
        zw.start_file("backdrop.bmp", opts).unwrap();
        zw.write_all(&[0x42, 0x4d]).unwrap();
        zw.finish().unwrap();

        let mount = dir.join("mount");
        fs::create_dir_all(&mount).unwrap();

        let installed = install_zip(&zip_path, &mount).unwrap();
        assert_eq!(installed.len(), 3);
        assert!(mount.join(".rockbox/themes/MyTheme.cfg").exists());
        assert!(mount.join(".rockbox/fonts/12-Test.fnt").exists());
        assert!(mount.join(".rockbox/backdrops/backdrop.bmp").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
