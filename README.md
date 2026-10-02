# rockbox-tui

> **⚠️ Experimental** — work in progress. The firmware-partition logic is
> unit-tested against synthetic images and has been exercised read-only on a
> real iPod Video, but the write (flash) path has **not** been validated on
> hardware yet. Use with care and keep a firmware backup.

Install [Rockbox](https://www.rockbox.org/) onto iPods from the comfort of a
terminal — no Windows machine, no Rockbox Utility.

It automates the manual install that the Rockbox project documents: detect the
iPod, back up its Apple firmware partition, flash the Rockbox bootloader into
the hidden firmware partition, and copy the Rockbox build (`.rockbox`) onto the
mounted data partition.

The low-level logic is a faithful Rust port of Rockbox's `ipodpatcher`
(`rbutil/ipodpatcher`, GPLv2, by Dave Chapman et al.) and is covered by unit
tests against a synthetic iPod disk image.

## Why

Rockbox Utility (`rbutil`) has a Linux build, but it is an aging Qt GUI that
breaks on modern distros — which is why many people keep a Windows machine
around just to install Rockbox. This is a small, self-contained TUI that does
the same job.

## Install

```bash
cargo install --path .
rockbox-tui            # interactive TUI
```

## Usage

Launch the TUI (run with `sudo` — raw-device access is required):

```bash
sudo rockbox-tui
```

Or use the CLI:

```bash
rockbox-tui scan                       # list connected iPods
sudo rockbox-tui install --device /dev/sdX
sudo rockbox-tui install --device /dev/sdX --firmware rockbox-ipodvideo.zip
sudo rockbox-tui uninstall --device /dev/sdX
sudo rockbox-tui backup   --device /dev/sdX --out ipod-firmware.img
sudo rockbox-tui restore  --device /dev/sdX --from ipod-firmware.img
```

### TUI keys

| Key        | Action                                    |
| ---------- | ----------------------------------------- |
| `s`/Enter  | install Rockbox on the selected iPod      |
| `j`/`k`/↑/↓| move selection                           |
| `r`        | rescan for iPods                          |
| `f`/`u`/`d`| log follow / scroll                       |
| `x`        | clear logs                                |
| `?`/`h`    | help                                      |
| `q`/Esc    | quit                                      |

## What install does

1. **Detect** — scans `/dev/sd[a-z]`, parses the MBR/Apple Partition Map and
   the firmware directory, and identifies the model (and RAM size, to pick
   `ipodvideo` vs `ipodvideo64mb`).
2. **Back up** — dumps the whole firmware partition to
   `~/.local/share/rockbox-tui/backups/` before touching anything.
3. **Fetch** — downloads the nightly Rockbox build, or uses a zip you pass
   with `--firmware`.
4. **Extract** — copies `.rockbox` onto the mounted data partition.
5. **Flash** — appends the bootloader to the OSOS firmware image and rewrites
   the firmware directory (the `entryOffset`/`len`/`chksum` dance from
   `ipodpatcher`).

## Getting the firmware and bootloader

Two external artifacts are involved at install time:

- **Bootloader** — bundled in the binary (no download needed). It ships
  [ipodloader2](https://github.com/crozone/ipodloader2) (GPLv2), a dual-boot
  loader for the classic iPod line (1g–5.5g, Mini 1g, Nano 1g). Override it
  with `--bootloader bootloader-ipodvideo.ipod` if you want the minimal
  official Rockbox bootloader instead.
- **Firmware** (the `.rockbox` build): downloaded automatically from
  `download.rockbox.org`, *or* supplied via `--firmware`. Note that
  `download.rockbox.org` sits behind a proof-of-work bot wall (Anubis), so the
  automatic download may fail from a headless tool; in that case download
  `rockbox-<target>.zip` manually from <https://www.rockbox.org/download/> and
  pass it with `--firmware`.

## Supported targets

Currently implemented and tested against synthetic images: the generic
`ipodpatcher` path, i.e. all 1g–5.5g iPods and the 1st-gen Mini/Nano. The
2nd-gen Nano (S5L87xx) uses a different bootloader scheme and is **not**
implemented yet.

## Development

```bash
cargo test      # core logic tests (partition/directory/bootloader parsing)
cargo build --release
```

## Prior art

- [Rockbox `ipodpatcher`](https://github.com/Rockbox/rockbox) — the C tool this
  ports (GPLv2).
- [robrohan/ipodpatcher](https://github.com/robrohan/ipodpatcher) — a 2024
  write-up of doing this exact install by hand on Linux.
