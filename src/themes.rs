//! Bundled theme pack: a small, curated set of themes (with their fonts) that
//! are installed by default, plus the machinery to activate one by merging its
//! settings into Rockbox's `config.cfg`.

use std::fs;
use std::path::{Path, PathBuf};

/// Files of the bundled theme pack, as (`path relative to .rockbox/`, bytes).
const BUNDLED_ASSETS: &[(&str, &[u8])] = &[
    ("themes/musicOS_v2.cfg", include_bytes!("../assets/themes/musicOS/themes/musicOS_v2.cfg")),
    ("wps/musicOS_v2.wps", include_bytes!("../assets/themes/musicOS/wps/musicOS_v2.wps")),
    ("wps/musicOS_v2.sbs", include_bytes!("../assets/themes/musicOS/wps/musicOS_v2.sbs")),
    ("fonts/13-Inter-SemiBold.fnt", include_bytes!("../assets/themes/musicOS/fonts/13-Inter-SemiBold.fnt")),
    ("fonts/15-Inter-Bold.fnt", include_bytes!("../assets/themes/musicOS/fonts/15-Inter-Bold.fnt")),
    ("fonts/20-Inter-SemiBold.fnt", include_bytes!("../assets/themes/musicOS/fonts/20-Inter-SemiBold.fnt")),
];

/// The bundled theme that is activated by default after an install.
const DEFAULT_THEME_CFG: &str = include_str!("../assets/themes/musicOS/themes/musicOS_v2.cfg");

/// Install the bundled theme pack onto the device. Returns the list of files
/// written (relative to `.rockbox/`).
pub fn install_bundled(mount: &Path) -> Result<Vec<PathBuf>, String> {
    let rockbox = mount.join(".rockbox");
    fs::create_dir_all(&rockbox).map_err(|e| format!("create {}: {e}", rockbox.display()))?;

    let mut written = Vec::new();
    for (rel, bytes) in BUNDLED_ASSETS {
        let out = rockbox.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        fs::write(&out, bytes).map_err(|e| format!("write {}: {e}", out.display()))?;
        written.push(PathBuf::from(rel));
    }
    Ok(written)
}

/// Parse a Rockbox `.cfg` file into `(key, value)` pairs, skipping comments
/// and blank lines.
fn parse_cfg(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Merge `settings` into an existing `config.cfg` body: keys that already
/// exist are replaced in place, new keys are appended.
fn merge_config(existing: &str, settings: &[(String, String)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut replaced: Vec<&str> = Vec::new();

    for line in existing.lines() {
        let key = line.trim().split_once(':').map(|(k, _)| k.trim()).unwrap_or("");
        if let Some((_, v)) = settings.iter().find(|(k, _)| k == key) {
            out.push(format!("{key}: {v}"));
            replaced.push(key);
        } else if !key.is_empty() || !line.trim().is_empty() {
            out.push(line.to_string());
        }
    }

    for (k, v) in settings {
        if !replaced.contains(&k.as_str()) {
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
    let settings = parse_cfg(DEFAULT_THEME_CFG);
    let config_path = mount.join(".rockbox").join("config.cfg");

    let existing = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read {}: {e}", config_path.display())),
    };

    let merged = merge_config(&existing, &settings);
    fs::write(&config_path, merged).map_err(|e| format!("write {}: {e}", config_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cfg() {
        let settings = parse_cfg("# comment\nwps: /.rockbox/wps/foo.wps\n\nfont: /.rockbox/fonts/bar.fnt\n");
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0], ("wps".to_string(), "/.rockbox/wps/foo.wps".to_string()));
    }

    #[test]
    fn merges_config_replacing_and_adding() {
        let existing = "volume: -20\nwps: /.rockbox/wps/old.wps\n";
        let settings = vec![
            ("wps".to_string(), "/.rockbox/wps/new.wps".to_string()),
            ("font".to_string(), "/.rockbox/fonts/x.fnt".to_string()),
        ];
        let merged = merge_config(existing, &settings);
        assert!(merged.contains("wps: /.rockbox/wps/new.wps"));
        assert!(merged.contains("volume: -20"));
        assert!(merged.contains("font: /.rockbox/fonts/x.fnt"));
        assert!(!merged.contains("old.wps"));
    }

    #[test]
    fn default_theme_cfg_has_key_settings() {
        let settings = parse_cfg(DEFAULT_THEME_CFG);
        let keys: Vec<&str> = settings.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"wps"));
        assert!(keys.contains(&"sbs"));
        assert!(keys.contains(&"font"));
    }
}
