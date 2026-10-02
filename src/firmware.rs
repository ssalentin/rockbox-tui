//! Rockbox firmware (the `.rockbox` build) download and extraction.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Where the nightly/daily builds live for a given target.
pub fn firmware_url(target: &str) -> String {
    format!("https://download.rockbox.org/daily/{target}/rockbox-{target}.zip")
}

/// Best-effort download of the firmware zip to `dest`, streaming through
/// `on_progress(downloaded, total)`. Returns a clear error if the server
/// returned something that isn't a zip (e.g. the Anubis bot-wall HTML).
pub fn download<F: FnMut(u64, Option<u64>)>(
    target: &str,
    dest: &Path,
    mut on_progress: F,
) -> Result<(), String> {
    let url = firmware_url(target);
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("download {target}: {e}"))?;

    let total = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    let mut reader = resp.into_reader();
    let mut file = fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded = 0u64;

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", dest.display()))?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }

    // The Anubis challenge is served as HTML, not a zip; detect it early.
    if !looks_like_zip(dest) {
        let _ = fs::remove_file(dest);
        return Err(format!(
            "the server did not return a firmware zip (likely a bot challenge). \
             Download rockbox-{target}.zip manually from https://www.rockbox.org/download/ \
             and pass it with --firmware."
        ));
    }

    Ok(())
}

fn looks_like_zip(path: &Path) -> bool {
    fs::File::open(path)
        .map(|mut f| {
            let mut magic = [0u8; 4];
            f.read_exact(&mut magic).is_ok() && magic == [0x50, 0x4b, 0x03, 0x04]
        })
        .unwrap_or(false)
}

/// Extract the `.rockbox` directory from a firmware zip onto the device.
///
/// `dest_dir` is the root of the mounted iPod data partition. An existing
/// `.rockbox` is removed first (matching a clean Rockbox update).
pub fn extract(zip_path: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("open {}: {e}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("open zip {}: {e}", zip_path.display()))?;

    // Remove any previous installation so stale files don't linger.
    let rockbox_dir = dest_dir.join(".rockbox");
    if rockbox_dir.exists() {
        fs::remove_dir_all(&rockbox_dir)
            .map_err(|e| format!("remove {}: {e}", rockbox_dir.display()))?;
    }

    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("create {}: {e}", dest_dir.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;

        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        // Only extract the `.rockbox/` tree.
        let rel = match rel.strip_prefix(".rockbox") {
            Ok(r) => r,
            Err(_) => continue,
        };

        let out = rockbox_dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let mut outfile =
                fs::File::create(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("extract {}: {e}", out.display()))?;
        }
    }

    Ok(rockbox_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_well_formed() {
        assert_eq!(
            firmware_url("ipodvideo"),
            "https://download.rockbox.org/daily/ipodvideo/rockbox-ipodvideo.zip"
        );
    }
}
