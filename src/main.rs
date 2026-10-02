mod app;
mod bootloader;
mod firmware;
mod ipod;
mod mounts;
mod ui;
mod workflow;

use anyhow::Result;

fn print_help() {
    println!("rockbox-tui — install Rockbox onto iPods from the terminal");
    println!();
    println!("Usage:");
    println!("  rockbox-tui                          launch the interactive TUI");
    println!("  rockbox-tui scan                     list connected iPods");
    println!("  rockbox-tui install --device /dev/sdX [OPTIONS]");
    println!("  rockbox-tui uninstall --device /dev/sdX");
    println!("  rockbox-tui backup  --device /dev/sdX --out FILE");
    println!("  rockbox-tui restore --device /dev/sdX --from FILE");
    println!();
    println!("Install options:");
    println!("  --device <PATH>       whole-disk device (e.g. /dev/sdc)");
    println!("  --firmware <ZIP>      use a locally downloaded firmware zip (skip download)");
    println!("  --bootloader <FILE>   use a bootloader .ipod/.bin file (skip bundled)");
    println!("  --backup-dir <DIR>    where to write the firmware backup");
    println!();
    println!("  -V, --version         print version");
    println!("  -h, --help            show this help");
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // No arguments -> TUI.
    if args.is_empty() {
        let mut app = app::App::new();
        return app.run();
    }

    match args[0].as_str() {
        "-h" | "--help" => print_help(),
        "-V" | "--version" => println!("rockbox-tui {}", app::VERSION),
        "scan" => cmd_scan(),
        "install" => cmd_install(&args[1..])?,
        "uninstall" => cmd_uninstall(&args[1..])?,
        "backup" => cmd_backup(&args[1..])?,
        "restore" => cmd_restore(&args[1..])?,
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    }
    Ok(())
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

fn parse_install_options(args: &[String]) -> Result<workflow::InstallOptions> {
    let mut opts = workflow::InstallOptions {
        device: String::new(),
        firmware: None,
        bootloader: None,
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
