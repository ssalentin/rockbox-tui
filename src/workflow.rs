//! The Rockbox install pipeline, shared by the CLI and the TUI.

use crate::ipod::{self, io::Device};
use crate::{bootloader, firmware, mounts};
use std::path::PathBuf;

/// Progress/log events emitted while a workflow runs.
#[derive(Debug, Clone)]
pub enum Event {
    /// A human-readable status line.
    Log(String),
    /// A named step has started (back up / download / extract / flash).
    Step(String),
    /// Byte progress for the current step.
    Progress { done: u64, total: Option<u64> },
}

/// Parameters for an install.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Whole-disk device path, e.g. `/dev/sdc`.
    pub device: String,
    /// A locally downloaded firmware zip (skips auto-download when set).
    pub firmware: Option<PathBuf>,
    /// A locally provided bootloader (skips the bundled one when set).
    pub bootloader: Option<PathBuf>,
    /// Force a Rockbox build target (e.g. `ipodvideo64mb`) instead of relying
    /// on auto-detection. Useful when RAM size can't be read because the iPod
    /// is booted into Rockbox rather than Apple Disk Mode.
    pub target: Option<String>,
    /// Directory to store the firmware-partition backup in.
    pub backup_dir: Option<PathBuf>,
}

/// Detect and fully describe the iPod on `device`, reopening it read-write.
fn open_device(device: &str, on: &mut dyn FnMut(Event)) -> Result<(Device, ipod::Ipod), String> {
    let mut dev = Device::open_readonly(device)
        .map_err(|e| format!("open {device}: {e}"))?;

    let mut ipod = ipod::Ipod::new(dev.sector_size);
    ipod::read_partinfo(&mut ipod, &mut dev.file())
        .map_err(|e| format!("{device}: {e}"))?;
    ipod::read_directory(&mut ipod, &mut dev.file())
        .map_err(|e| format!("{device}: {e}"))?;

    let version = ipod.images[ipod.osos_image].vers >> 8;
    let model = ipod::get_model(version).ok_or_else(|| format!("{device}: unknown iPod model"))?;
    ipod.model = Some(model);

    if let Some(mb) = ipod::scsi::read_ramsize(&dev) {
        ipod.ramsize_mb = mb;
    }

    let target = ipod.build_target().unwrap_or("unknown");
    on(Event::Log(format!(
        "Detected {} ({}) — build target {target}",
        model.modelstr,
        if ipod.macpod { "macpod" } else { "winpod" }
    )));

    dev.reopen_rw()
        .map_err(|e| format!("reopen {device} read-write (need root?): {e}"))?;

    Ok((dev, ipod))
}

/// Run the full install: back up, fetch/extract firmware, flash bootloader.
pub fn install(opts: &InstallOptions, on: &mut dyn FnMut(Event)) -> Result<(), String> {
    let (mut dev, ipod) = open_device(&opts.device, on)?;

    // 1. Back up the firmware partition.
    on(Event::Step("Backing up firmware partition".into()));
    let backup_dir = opts.backup_dir.clone().unwrap_or_else(default_backup_dir);
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("create {}: {e}", backup_dir.display()))?;
    let backup_path = backup_dir.join(format!(
        "{}-{}-firmware-{}.img",
        basename(&opts.device),
        ipod.model.as_ref().map(|m| m.modelname).unwrap_or("ipod"),
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
    ));
    {
        let mut out = std::fs::File::create(&backup_path)
            .map_err(|e| format!("create backup {}: {e}", backup_path.display()))?;
        ipod::read_partition(&ipod, &mut dev.file(), &mut out)?;
    }
    on(Event::Log(format!("Firmware backup written to {}", backup_path.display())));

    // 2. Obtain the firmware zip.
    let target = opts
        .target
        .clone()
        .unwrap_or_else(|| ipod.build_target().unwrap_or("ipodvideo").to_string());
    let firmware_path = match &opts.firmware {
        Some(p) => p.clone(),
        None => {
            on(Event::Step(format!("Downloading Rockbox {target}")));
            let tmp = backup_dir.join(format!("rockbox-{target}.zip"));
            firmware::download(&target, &tmp, |done, total| {
                on(Event::Progress { done, total });
            })?;
            tmp
        }
    };

    // 3. Locate the mounted data partition and extract .rockbox.
    let mount = mounts::find_data_mount(&opts.device).ok_or_else(|| {
        "the iPod's data partition is not mounted — mount it first".to_string()
    })?;
    on(Event::Step(format!("Extracting .rockbox to {}", mount.display())));
    let rockbox_dir = firmware::extract(&firmware_path, &mount)?;
    on(Event::Log(format!("Extracted {}", rockbox_dir.display())));

    // 4. Flash the bootloader.
    on(Event::Step("Flashing bootloader".into()));
    let bootloader = bootloader::load(opts.bootloader.as_deref())?;
    ipod::add_bootloader(&ipod, &mut dev.file(), &bootloader)?;
    on(Event::Log("Bootloader installed".into()));

    on(Event::Log(
        "Done. Eject the iPod, then hold Menu + Select to reboot into Rockbox.".into(),
    ));
    Ok(())
}

/// Remove a previously installed Rockbox bootloader.
pub fn uninstall(opts: &InstallOptions, on: &mut dyn FnMut(Event)) -> Result<(), String> {
    let (mut dev, ipod) = open_device(&opts.device, on)?;
    on(Event::Step("Removing bootloader".into()));
    ipod::delete_bootloader(&ipod, &mut dev.file())?;
    on(Event::Log("Bootloader removed".into()));
    Ok(())
}

/// Restore a firmware partition from a backup image (the inverse of the
/// backup step performed during install).
pub fn restore(device: &str, backup: &std::path::Path, on: &mut dyn FnMut(Event)) -> Result<(), String> {
    let (mut dev, ipod) = open_device(device, on)?;
    on(Event::Step("Restoring firmware partition".into()));
    let mut src = std::fs::File::open(backup)
        .map_err(|e| format!("open {}: {e}", backup.display()))?;
    ipod::write_partition(&ipod, &mut dev.file(), &mut src)?;
    on(Event::Log("Firmware partition restored".into()));
    Ok(())
}

fn default_backup_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("rockbox-tui").join("backups")
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_works() {
        assert_eq!(basename("/dev/sdc"), "sdc");
        assert_eq!(basename("sdc"), "sdc");
    }
}
