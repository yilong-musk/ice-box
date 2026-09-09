# Testing

Run commands from the repository root. Development prerequisites are in the
[README](../README.md#development); release requirements are in
[release-process.md](release-process.md).

## Automated gates

| Command | Coverage |
|---------|----------|
| `bash scripts/gate-local.sh` | Formatting, clippy and Rust library tests excluding the desktop crate; TUN integration tests; desktop/website typechecks; Vitest; updater fixtures; demo screenshot |
| `bash scripts/gate.sh` | All-crate clippy; non-desktop Rust library/integration/doc tests plus desktop library tests; frontend checks, fixtures, and production build |

The local gate omits desktop Rust tests because GTK/WebKit dependencies may be
unavailable. Its screenshot step refreshes the documentation image when the
version or relevant UI files change. CI skips screenshot capture.

CI runs the full gate on Linux and `GATE_SCOPE=rust` on macOS/Windows; packaging
jobs build separately. `GATE_SCOPE=frontend` runs only the frontend half.
The [gate scripts](../scripts/gate.sh) and [CI workflow](../.github/workflows/ci.yml)
define the exact commands.

`cargo test --lib` does not run `crates/*/tests/`. The local gate explicitly
includes `ice-tun-sys` integration tests; CI covers all non-desktop integration
and doc tests. Gates exclude all `#[ignore]` tests.

## Live tests

Run relevant ignored tests when changing the corresponding subsystem. TUN,
core, and system-proxy tests can modify host networking; use a disposable test
host and stop the app first. Core/TUN tests require the bundled binary
(`bash scripts/fetch-singbox.sh`, or add `win` for Windows).

| Test | Command | Prerequisite |
|------|---------|--------------|
| macOS host probes | `cargo test -p ice-tun-sys --test macos_backend -- --ignored` | Real Mac |
| Windows host probes | `cargo test -p ice-tun-sys --test windows_backend -- --ignored` | Real Windows host with elevation |
| macOS TUN (G9.12) | `bash scripts/run-acceptance-macos-tun.sh` | Cache sudo credentials with `sudo -v` first |
| macOS helper (G9.13) | `bash scripts/run-acceptance-macos-tun.sh --helper` | Authorization for helper install/uninstall |
| macOS restart recovery (G9.15) | `cargo test -p ice-box --lib g9_15 -- --ignored --nocapture` | Installed helper; unset `ICE_BOX_TUN_DEV_SUDO`; uses the installed app data directory |
| Windows TUN (G9.14) | `bash scripts/run-acceptance-windows-tun.sh` | Elevated Bash shell and MSVC toolchain |
| System proxy | `cargo test -p ice-proxy-sys --lib -- --ignored --test-threads=1` | Native macOS/Windows host |
| HTTPS fetch | `cargo test -p ice-subscription --lib https_fetch_succeeds -- --ignored` | Network access |

The helper script covers G9.13 and uninstalls the helper afterwards; G9.15 needs
a separately installed helper. Its data directory defaults to the installed
app's directory and can be set with `ICE_BOX_TUN_LIVE_DATA_DIR`.

For ordinary live core acceptance, use a serial run with the privileged TUN
cases excluded:

```bash
cargo test -p ice-box --lib acceptance::live:: -- --ignored --nocapture \
  --test-threads=1 --skip g9_12 --skip g9_13 --skip g9_14 --skip g9_15
```

This covers core lifecycle, reload, system-proxy restoration/port changes,
logs, and Clash profile/group/mode behavior. Test definitions and individual
ignore reasons live in [acceptance.rs](../apps/desktop/src-tauri/src/acceptance.rs).
