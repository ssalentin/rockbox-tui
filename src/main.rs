mod app;
mod bootloader;
mod firmware;
mod ipod;
mod mounts;
mod theme;
mod themes;
mod ui;
mod version;
mod workflow;

use anyhow::Result;

fn print_help() {
    println!("rockbox-tui — install Rockbox onto iPods from the terminal");
    println!();
    println!("Usage:");
    println!("  rockbox-tui                          launch the interactive TUI");
    println!("  rockbox-tui scan                     list connected iPods");
    println!("  rockbox-tui info --device /dev/sdX    show firmware partition contents");
    println!("  rockbox-tui install --device /dev/sdX [OPTIONS]");
    println!("  rockbox-tui uninstall --device /dev/sdX");
    println!("  rockbox-tui backup  --device /dev/sdX --out FILE");
    println!("  rockbox-tui restore --device /dev/sdX --from FILE");
    println!("  rockbox-tui theme   --device /dev/sdX --zip THEME.zip");
    println!("  rockbox-tui fonts   --device /dev/sdX --zip FONTS.zip");
    println!("  rockbox-tui themes  --device /dev/sdX   (install bundled theme pack)");
    println!();
    println!("Install options:");
    println!("  --device <PATH>       whole-disk device (e.g. /dev/sdc)");
    println!("  --firmware <ZIP>      use a locally downloaded firmware zip (skip download)");
    println!("  --bootloader <FILE>   use a bootloader .ipod/.bin file (skip bundled)");
    println!("  --target <NAME>       force a build target (e.g. ipodvideo64mb)");
    println!("  --no-themes           skip the bundled theme pack + default theme");
    println!("  --backup-dir <DIR>    where to write the firmware backup");
    println!();
    println!("  -V, --version         print version");
    println!("  -h, --help            show this help");
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // No arguments -> TUI.
    if args.is_empty() {
        escalate_if_needed();
        let mut app = app::App::new();
        return app.run();
    }

    match args[0].as_str() {
        "-h" | "--help" => print_help(),
        "-V" | "--version" => println!("rockbox-tui {}", app::VERSION),
        "scan" => cmd_scan(),
        "info" => cmd_info(&args[1..])?,
        "install" => cmd_install(&args[1..])?,
        "uninstall" => cmd_uninstall(&args[1..])?,
        "backup" => cmd_backup(&args[1..])?,
        "restore" => cmd_restore(&args[1..])?,
        "theme" => cmd_assets(&args[1..], "theme")?,
        "fonts" => cmd_assets(&args[1..], "fonts")?,
        "themes" => cmd_themes(&args[1..])?,
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    }
    Ok(())
}

/// Re-exec the program with elevated privileges (via `pkexec`) when raw
/// device access is required but unavailable. Only used for the TUI, where a
/// silent "no iPods found" would otherwise be confusing.
fn escalate_if_needed() {
    // Already root?
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    // We can already read the devices (e.g. user is in the `disk` group)?
    let (devices, denied) = ipod::io::scan();
    if !devices.is_empty() || denied == 0 {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    eprintln!("raw disk access is required — requesting elevation…");
    let _ = std::process::Command::new("pkexec").arg(exe).status();
    std::process::exit(0);
}

fn cmd_scan() {
    let (devices, denied) = ipod::io::scan();
    if devices.is_empty() {
        println!("No iPods found.");
        if denied > 0 {
            eprintln!("Note: {denied} device(s) required root access. Run with sudo to scan them.");
        }
        return;
    }
    for (path, ipod) in &devices {
        let model = ipod.model.as_ref().map(|m| m.modelstr).unwrap_or("unknown");
        let target = ipod.build_target().unwrap_or("unknown");
        let mount = mounts::find_data_mount(path)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not mounted".into());
        let ram = if ipod.ramsize_mb > 0 {
            format!("{} MiB RAM", ipod.ramsize_mb)
        } else {
            "RAM unknown".into()
        };
        println!(
            "{path}  {model}  ({})  target={target}  {ram}  {mount}",
            if ipod.macpod { "macpod" } else { "winpod" }
        );
    }
}

fn cmd_info(args: &[String]) -> Result<()> {
    let mut device = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--device" {
            device = args.get(i + 1).cloned();
        }
        i += 1;
    }
    let Some(device) = device else {
        eprintln!("info requires --device /dev/sdX");
        std::process::exit(2);
    };

    let mut dev = ipod::io::Device::open_readonly(&device)?;
    let mut ipod = ipod::Ipod::new(dev.sector_size);
    ipod::read_partinfo(&mut ipod, &mut dev.file())
        .map_err(|e| anyhow::anyhow!("{device}: {e}"))?;
    ipod::read_directory(&mut ipod, &mut dev.file())
        .map_err(|e| anyhow::anyhow!("{device}: {e}"))?;

    let model = ipod
        .images
        .get(ipod.osos_image)
        .map(|o| ipod::get_model(o.vers >> 8))
        .flatten()
        .map(|m| m.modelstr.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("{device}  {model}  ({})", if ipod.macpod { "macpod" } else { "winpod" });
    println!("  firmware partition: LBA {} + {} sectors", ipod.pinfo[0].start, ipod.pinfo[0].size);

    for (n, img) in ipod.images.iter().enumerate() {
        let ftype = match img.ftype {
            ipod::Ftype::Osos => "OSOS",
            ipod::Ftype::Rsrc => "RSRC",
            ipod::Ftype::Aupd => "AUPD",
            ipod::Ftype::Hibe => "HIBE",
            ipod::Ftype::Osbk => "OSBK",
        };
        let star = if n == ipod.osos_image { " (main firmware)" } else { "" };
        if img.ftype == ipod::Ftype::Osos && img.entry_offset > 0 {
            println!(
                "  [{n}] {ftype}{star}: {size} bytes, bootloader present ({bl} bytes at +{eo})",
                size = img.len,
                bl = img.len - img.entry_offset,
                eo = img.entry_offset
            );
        } else {
            println!("  [{n}] {ftype}{star}: {} bytes", img.len);
        }
    }

    let bootloader = ipod.images[ipod.osos_image].entry_offset > 0;
    println!(
        "  Rockbox bootloader: {}",
        if bootloader { "installed" } else { "not installed" }
    );
    Ok(())
}

fn cmd_install(args: &[String]) -> Result<()> {
    use std::io::Write;
    let opts = parse_install_options(args)?;
    let mut on = |ev: workflow::Event| match ev {
        workflow::Event::Log(s) => println!("  {s}"),
        workflow::Event::Step(s) => println!("== {s}"),
        workflow::Event::Progress { done, total } => {
            match total {
                Some(t) if t > 0 => print!("\r  {:.1}% ({done}/{t})", done as f64 * 100.0 / t as f64),
                _ => print!("\r  {done} bytes"),
            }
            let _ = std::io::stdout().flush();
        }
    };
    match workflow::install(&opts, &mut on) {
        Ok(()) => {
            println!();
            println!("Installation complete.");
            Ok(())
        }
        Err(e) => {
            println!();
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_uninstall(args: &[String]) -> Result<()> {
    let opts = parse_install_options(args)?;
    let mut on = |ev: workflow::Event| match ev {
        workflow::Event::Log(s) => println!("  {s}"),
        workflow::Event::Step(s) => println!("== {s}"),
        workflow::Event::Progress { .. } => {}
    };
    match workflow::uninstall(&opts, &mut on) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_backup(args: &[String]) -> Result<()> {
    let mut device = None;
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--device" => device = args.get(i + 1).cloned(),
            "--out" => out = args.get(i + 1).cloned(),
            _ => {}
        }
        i += 1;
    }
    let (device, out) = match (device, out) {
        (Some(d), Some(o)) => (d, o),
        _ => {
            eprintln!("backup requires --device and --out");
            std::process::exit(2);
        }
    };

    let mut dev = ipod::io::Device::open_readonly(&device)?;
    let mut ipod = ipod::Ipod::new(dev.sector_size);
    ipod::read_partinfo(&mut ipod, &mut dev.file())
        .map_err(|e| anyhow::anyhow!("{device}: {e}"))?;
    ipod::read_directory(&mut ipod, &mut dev.file())
        .map_err(|e| anyhow::anyhow!("{device}: {e}"))?;

    let mut file = std::fs::File::create(&out)?;
    ipod::read_partition(&ipod, &mut dev.file(), &mut file)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Wrote firmware partition to {out}");
    Ok(())
}

fn cmd_restore(args: &[String]) -> Result<()> {
    let mut device = None;
    let mut from = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--device" => device = args.get(i + 1).cloned(),
            "--from" => from = args.get(i + 1).cloned(),
            _ => {}
        }
        i += 1;
    }
    let (device, from) = match (device, from) {
        (Some(d), Some(f)) => (d, f),
        _ => {
            eprintln!("restore requires --device and --from");
            std::process::exit(2);
        }
    };
    let mut on = |ev: workflow::Event| match ev {
        workflow::Event::Log(s) => println!("  {s}"),
        workflow::Event::Step(s) => println!("== {s}"),
        workflow::Event::Progress { .. } => {}
    };
    match workflow::restore(&device, std::path::Path::new(&from), &mut on) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

/// Install the bundled theme pack and activate the default theme.
fn cmd_themes(args: &[String]) -> Result<()> {
    let mut device = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--device" {
            device = args.get(i + 1).cloned();
        }
        i += 1;
    }
    let Some(device) = device else {
        eprintln!("themes requires --device /dev/sdX");
        std::process::exit(2);
    };

    let mount = mounts::find_data_mount(&device)
        .ok_or_else(|| anyhow::anyhow!("data partition is not mounted — mount it first"))?;
    let written = themes::install_bundled(&mount).map_err(|e| anyhow::anyhow!("{e}"))?;
    themes::activate_default(&mount).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Installed {} bundled theme file(s) and activated the default theme.", written.len());
    Ok(())
}

fn cmd_assets(args: &[String], kind: &str) -> Result<()> {
    let mut device = None;
    let mut zip = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--device" => device = args.get(i + 1).cloned(),
            "--zip" => zip = args.get(i + 1).cloned(),
            _ => {}
        }
        i += 1;
    }
    let (device, zip) = match (device, zip) {
        (Some(d), Some(z)) => (d, z),
        _ => {
            eprintln!("{kind} requires --device and --zip");
            std::process::exit(2);
        }
    };

    let mount = mounts::find_data_mount(&device)
        .ok_or_else(|| anyhow::anyhow!("data partition is not mounted — mount it first"))?;
    let installed = theme::install_zip(std::path::Path::new(&zip), &mount)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Installed {} file(s) to {}/.rockbox:", installed.len(), mount.display());
    for f in installed {
        println!("  {}", f.display());
    }
    Ok(())
}

fn parse_install_options(args: &[String]) -> Result<workflow::InstallOptions> {
    let mut opts = workflow::InstallOptions {
        device: String::new(),
        firmware: None,
        bootloader: None,
        target: None,
        no_themes: false,
        backup_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--device" => {
                if let Some(v) = args.get(i + 1) {
                    opts.device = v.clone();
                }
            }
            "--firmware" => {
                if let Some(v) = args.get(i + 1) {
                    opts.firmware = Some(v.into());
                }
            }
            "--bootloader" => {
                if let Some(v) = args.get(i + 1) {
                    opts.bootloader = Some(v.into());
                }
            }
            "--target" => {
                if let Some(v) = args.get(i + 1) {
                    opts.target = Some(v.clone());
                }
            }
            "--no-themes" => opts.no_themes = true,
            "--backup-dir" => {
                if let Some(v) = args.get(i + 1) {
                    opts.backup_dir = Some(v.into());
                }
            }
            _ => {}
        }
        i += 1;
    }
    if opts.device.is_empty() {
        anyhow::bail!("install requires --device /dev/sdX");
    }
    Ok(opts)
}
