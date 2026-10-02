//! Bundled theme pack: a curated starter theme (with its fonts and images)
//! that is installed by default, plus the machinery to activate it by merging
//! its settings into Rockbox's `config.cfg`.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// The bundled themes, as zips laid out relative to `.rockbox/`.
///
/// musicOS is the default (activated after install); the others are installed
/// alongside it so they can be selected from Rockbox's Theme menu.
const BUNDLED_THEMES: &[&[u8]] = &[
    include_bytes!("../assets/themes/musicOS.zip"),
    include_bytes!("../assets/themes/chroma.zip"),
];

/// Settings that activate the bundled theme, parsed from its `.cfg`.
const DEFAULT_THEME_SETTINGS: &[(&str, &str)] = &[
    ("wps", "/.rockbox/wps/musicOS_v2.wps"),
    ("sbs", "/.rockbox/wps/musicOS_v2.sbs"),
    ("font", "/.rockbox/fonts/20-Inter-SemiBold.fnt"),
    ("foreground color", "000000"),
    ("background color", "FFFFFF"),
    ("selector type", "bar (color)"),
    ("line selector start color", "F24E61"),
    ("line selector end color", "F24E61"),
    ("line selector text color", "FFFFFF"),
    ("show icons", "off"),
    ("scrollbar", "right"),
    ("scrollbar width", "6"),
    ("statusbar", "top"),
];

/// Install the bundled theme pack onto the device. Returns the list of files
/// written (relative to `.rockbox/`).
pub fn install_bundled(mount: &Path) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    for zip in BUNDLED_THEMES {
        written.extend(crate::theme::install(Cursor::new(*zip), mount)?);
    }
    Ok(written)
}

/// Merge `settings` into an existing `config.cfg` body: keys that already
/// exist are replaced in place, new keys are appended.
fn merge_config(existing: &str, settings: &[(&str, &str)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut replaced: Vec<&str> = Vec::new();

    for line in existing.lines() {
        let key = line.trim().split_once(':').map(|(k, _)| k.trim()).unwrap_or("");
        if let Some((_, v)) = settings.iter().find(|(k, _)| *k == key) {
            out.push(format!("{key}: {v}"));
            replaced.push(key);
        } else if !line.trim().is_empty() {
            out.push(line.to_string());
        }
    }

    for (k, v) in settings {
        if !replaced.contains(k) {
            out.push(format!("{k}: {v}"));
        }
    }

    let mut s = out.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Activate the bundled default theme by merging its settings into the
/// device's `.rockbox/config.cfg`.
pub fn activate_default(mount: &Path) -> Result<(), String> {
    let config_path = mount.join(".rockbox").join("config.cfg");

    let existing = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read {}: {e}", config_path.display())),
    };

    let merged = merge_config(&existing, DEFAULT_THEME_SETTINGS);
    fs::write(&config_path, merged).map_err(|e| format!("write {}: {e}", config_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_zip_is_complete() {
        for (idx, zip) in BUNDLED_THEMES.iter().enumerate() {
            let mut archive = zip::ZipArchive::new(Cursor::new(*zip)).unwrap();
            let names: Vec<String> = (0..archive.len())
                .map(|i| archive.by_index(i).unwrap().name().to_string())
                .collect();
            assert!(names.iter().any(|n| n.ends_with(".cfg")), "theme {idx} has no .cfg");
            assert!(names.iter().any(|n| n.ends_with(".fnt")), "theme {idx} has no fonts");
        }
    }

    #[test]
    fn merges_config_replacing_and_adding() {
        let existing = "volume: -20\nwps: /.rockbox/wps/old.wps\n";
        let settings = &[("wps", "/.rockbox/wps/new.wps"), ("font", "/.rockbox/fonts/x.fnt")];
        let merged = merge_config(existing, settings);
        assert!(merged.contains("wps: /.rockbox/wps/new.wps"));
        assert!(merged.contains("volume: -20"));
        assert!(merged.contains("font: /.rockbox/fonts/x.fnt"));
        assert!(!merged.contains("old.wps"));
    }

    #[test]
    fn default_settings_have_key_entries() {
        let keys: Vec<&str> = DEFAULT_THEME_SETTINGS.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"wps"));
        assert!(keys.contains(&"sbs"));
        assert!(keys.contains(&"font"));
    }
}
