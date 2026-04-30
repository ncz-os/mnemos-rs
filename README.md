# mnemos-rs

Rust client tooling for [MNEMOS](https://github.com/mnemos-os/mnemos).

## Why Rust

The MNEMOS server stays Python (PyInstaller for desktop tier, Podman for
production fleet). The **clients** that ship per-user / per-session are
where Rust pays off:

| Component | Old (Python) | New (Rust) |
|---|---|---|
| Desktop CLI | 3 ad-hoc Python scripts in `~/mnemos-desktop/` | One `mnemosctl` binary, ~10 MB, statically linked |
| MCP stdio | SSH→PYTHIA→`/opt/mnemos/venv/bin/python /opt/mnemos/mcp_server.py` | One `mnemosctl mcp-stdio` invocation, instant startup, zero install dependencies |

See [the charter memory] for the Python/Rust scope decision rule:
**per-user / per-session / per-spawn → Rust; per-host / per-server /
per-cutover → Python.**

[the charter memory]: https://github.com/mnemos-os/mnemos/blob/master/docs/RUST_CLIENT_SCOPE.md

## Binaries

- `mnemosctl` — desktop CLI. Status, search, create, sync, peer mgmt,
  MCP stdio bridge.

## Build matrix

- macos-aarch64 (Apple Silicon)
- macos-x86_64 (Intel Macs)
- linux-x86_64 (Ubuntu, Debian, RHEL family — covers WSL2)
- linux-aarch64 (Pi 4/5, Jetson, ARM workstations)
- windows-x86_64 (cargo cross-build from Linux/macOS host)

## Status

Slice 1 (scaffold): clap CLI surface + subcommand stubs.
Subsequent slices fill in subcommands one at a time.

## License

Apache-2.0.
