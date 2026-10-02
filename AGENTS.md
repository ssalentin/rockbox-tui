# rockbox-tui — agent notes

- Run `cargo test` after any change (partition/directory/bootloader logic is
  unit-tested against synthetic iPod disk images).
- Reinstall the binary after changes so `rockbox-tui` on PATH picks up the new
  build:
  ```bash
  cargo install --path . --force
  sha256sum ~/.cargo/bin/rockbox-tui target/release/rockbox-tui   # hashes must match
  ```
- The low-level firmware-partition logic is a Rust port of Rockbox's
  `ipodpatcher` (GPLv2) — keep the license header/attribution when vendoring
  more of it.
- `download.rockbox.org` is behind Anubis; the auto-download path may return an
  HTML challenge. Prefer the `--firmware`/`--bootloader` file paths for tests.
- Commit and push to the Gitea remote (`origin`) when done.

See `../AGENTS.md` for the global rules (sudo/pkexec, Gitea push, etc.).
