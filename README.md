# mnemos-rs

Rust client tooling for [MNEMOS](https://github.com/mnemos-os/mnemos).

## Why Rust

The MNEMOS server stays Python (PyInstaller for desktop tier, Podman for
production fleet). The **clients** that ship per-user / per-session are
where Rust pays off:

| Component | Old (Python) | New (Rust) |
|---|---|---|
| Desktop utilities | Ad-hoc Python scripts in `~/mnemos-desktop/` | One `mnemos-desktop` binary, ~10 MB, statically linked |

See [the charter memory] for the Python/Rust scope decision rule:
**per-user / per-session / per-spawn → Rust; per-host / per-server /
per-cutover → Python.**

[the charter memory]: https://github.com/mnemos-os/mnemos/blob/master/docs/RUST_CLIENT_SCOPE.md

## Binaries

- `mnemos-desktop` — desktop CLI. Status, search, create, and peer management.

The separately maintained [`mnemosctl`](https://github.com/mnemos-os/mnemosctl)
project owns the `mnemosctl` package and executable names.

## Build matrix

- macos-aarch64 (Apple Silicon)
- macos-x86_64 (Intel Macs)
- linux-x86_64 (Ubuntu, Debian, RHEL family — covers WSL2)
- linux-aarch64 (Pi 4/5, Jetson, ARM workstations)
- windows-x86_64 (cargo cross-build from Linux/macOS host)

## Status

Implemented commands are exposed in `mnemos-desktop --help`.

## License

Apache-2.0.
