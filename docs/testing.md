# Testing

Run commands from the repository root. Development prerequisites are in the
[README](../README.md#development); release requirements are in
[release-process.md](release-process.md).

## Automated gates

| Command | Coverage |
|---------|----------|
| `bash scripts/gate-local.sh` | Scoped to uncommitted changes vs `HEAD` (or the whole branch vs `main` when the working tree is clean; pass `--branch` to force full-branch scope, `--serial` for sequential live output, or `--full` for every check). Formatting, the Rust toolchain pin check, and the documentation version check (`scripts/check-doc-versions.sh`: the sing-box version in both READMEs and `tun.md` against `third_party/sing-box/VERSION`) always run; independent buckets run in parallel. For Rust changes, runs one combined `clippy` and unit tests (`--lib --bins`) for the changed crates and their transitive non-shell workspace dependents (plus `ice-tun-sys` integration tests when `ice-tun-sys` is affected, and changed Tauri shells when GTK/WebKit are available); for frontend changes, splits into parallel desktop/website/mobile typechecks, desktop Vitest, fast architecture/version Vitest for cross-app boundary inputs, website installer-link tests, and website build + mobile Live Demo smoke (skipping `playwright install` when Chromium is already cached); for script changes, runs `bash -n` on changed scripts and all fixture suites in parallel |
| `bash scripts/gate.sh` | The same toolchain pin and documentation version checks; all-crate clippy; non-desktop Rust library/integration/doc tests plus desktop library tests; frontend checks, fixtures, the website production build and mobile Live Demo smoke, and the desktop production build |
| `bash scripts/capture-docs.sh` | Not a gate: refreshes the desktop and Android Home captures and recomposes the README hero |

When the working tree has staged, unstaged, or untracked changes, the local gate
compares those uncommitted files against `HEAD` so follow-up commits on a large
branch do not re-run unrelated buckets. When the working tree is clean — or when
`--branch` or `GATE_LOCAL_BASE` is passed — it compares against the merge base
with `origin/main` (or `main`). Each changed path selects fine-grained checks:

| Changed paths | Checks |
|---------------|--------|
| `crates/` | Rust `clippy` + `--lib --bins` for the changed crates and their transitive non-shell workspace dependents (plus `ice-tun-sys` integration tests when affected) |
| `apps/*/src-tauri/`, `apps/mobile/plugins/` | Changed Tauri shell (`ice-box` / `ice-box-mobile`, when GTK/WebKit are available) |
| `configs/`, `third_party/`, `spikes/`, `Cargo.*`, `rust-toolchain.toml`, `deny.toml`, `.cargo/` | Full workspace Rust (non-shell) + `ice-tun-sys` integration tests |
| `apps/desktop/src/**/*.test.{ts,tsx}`, `apps/desktop/src/test/` | Parallel typecheck (`desktop`, `website`, `mobile`) + desktop Vitest (skips website build and mobile demo smoke) |
| Other `apps/desktop/` files | Parallel typecheck + desktop Vitest + website installer-link tests + website build & mobile demo smoke (for shared app sources) |
| `apps/website/` | Parallel typecheck + website installer-link tests + website build & mobile demo smoke (skips full desktop Vitest) |
| `apps/mobile/` (non-Rust) | Parallel typecheck + website build & mobile demo smoke |
| Root `package*.json` | All frontend checks |
| `scripts/` | `bash -n` on changed scripts + all script fixture suites in parallel |
| `*.md`, `docs/`, `.github/`, `.vscode/`, `LICENSE`, `NOTICE`, `.gitignore` | Formatting, toolchain pins, and documentation versions only |

Some files also feed another bucket's checks and select it as well:

| Changed paths | Also runs |
|---------------|-----------|
| `apps/desktop/src-tauri/src/application/`, `crates/ice-app/Cargo.toml`, `apps/website/vite.config.ts`, `apps/website/src/browser-shell.ts`, `apps/website/mobile-demo.html`, `apps/website/src/main.js`, `apps/mobile/src/shell.ts` | Fast architecture & version Vitest (`architecture.test.ts`, `appVersion.test.ts`) unless full Vitest is already selected |
| `CHANGELOG.md`, root `Cargo.toml`, `apps/{desktop,mobile}/package.json`, `apps/{desktop,mobile}/src-tauri/tauri.conf.json` | Script fixtures (release preflight checks the version sources and the changelog section) |

Renames count both the old and the new path. Any other path, a change to
`scripts/gate-local.sh` itself, or a missing merge base (in branch mode) runs
every check. `--dry-run` prints the selected diff mode and scope without running
anything, and `--serial` (`GATE_LOCAL_SERIAL=1`) runs tasks sequentially with
live stdout. Scope selection is covered by `scripts/test-gate-local.sh`.

The local gate builds the desktop and mobile Tauri shells only when their
sources change, and skips them with a warning when the GTK/WebKit development
packages are missing on Linux; CI covers them either way. Build helper scripts
such as `fetch-singbox.sh` and `prepare-singbox-resource.sh` run only inside
`tauri build`, so locally they get just the syntax check. Neither gate touches
tracked files: `scripts/capture-docs.sh` refreshes the desktop and Android Home
captures when the version or relevant UI files change, or when any capture is
missing, and recomposes the README hero on every run. Run it when the Live Demo
UI changes and before a release.

The mobile Live Demo smoke test builds the website, serves `dist` with
`vite preview`, and checks the phone iframe on the landing page: the node-list
warning, the phone shell, all five tabs, the Apps fixtures, per-app routing, the
VPN power toggle, and the language and theme bridges from the site into the
iframe. The Linux CI gate installs Chromium and runs it; `GATE_SCOPE=rust` jobs
skip it.

CI runs the `.github/actions/gate-linux` action (full gate) and
`GATE_SCOPE=rust` on macOS/Windows; packaging jobs build separately. The release
workflow does not re-run the gate: its preflight requires those CI checks to
have passed on the release commit (`scripts/verify-ci-status.sh`), runs
`scripts/release-preflight.sh` before any build, and runs
`scripts/verify-release-assets.sh` after publication. `GATE_SCOPE=frontend` runs
only the frontend half. The [gate scripts](../scripts/gate.sh) and
[CI workflow](../.github/workflows/ci.yml) define the exact commands.

## Android builds

A debug APK needs JDK 17, Go, and an Android SDK. Set `ANDROID_HOME`, then
install the SDK packages and build:

```bash
bash scripts/ci-install-android-sdk.sh
bash scripts/build-android-apk.sh debug arm64   # device
bash scripts/build-android-apk.sh debug amd64   # emulator
```

`arm64` is the published ABI. `amd64` is the emulator artifact and is not
released. The APK script downloads the pinned GeoIP rule-sets and packages
them; the phone copies those files into its files directory before generating
`config.json`. Daily flows were checked on an Android 14 emulator. A signed
release build is described in [release-process.md](release-process.md#android-signing-one-time).

`cargo test --lib` does not run `crates/*/tests/`. The local gate explicitly
includes `ice-tun-sys` integration tests; CI covers all non-desktop integration
and doc tests. Gates exclude all `#[ignore]` tests.

## Live tests

Run relevant ignored tests when changing the corresponding subsystem. TUN,
core, and system-proxy tests can modify host networking; use a disposable test
host and stop the app first. Core/TUN tests require the bundled binary
(`bash scripts/fetch-singbox.sh`, or pass `win`, `linux-x64`, or `linux-arm64`).

| Test | Command | Prerequisite |
|------|---------|--------------|
| macOS host probes | `cargo test -p ice-tun-sys --test macos_backend -- --ignored` | Real Mac |
| Windows host probes | `cargo test -p ice-tun-sys --test windows_backend -- --ignored` | Real Windows host with elevation |
| Linux host probes | `cargo test -p ice-tun-sys --test linux_backend -- --ignored` | Real Linux host with `iproute2` |
| macOS TUN (G9.12) | `bash scripts/run-acceptance-macos-tun.sh` | Cache sudo credentials with `sudo -v` first |
| macOS helper (G9.13) | `bash scripts/run-acceptance-macos-tun.sh --helper` | Authorization for helper install/uninstall |
| macOS restart recovery (G9.15) | `cargo test -p ice-box --lib g9_15 -- --ignored --nocapture` | Installed helper; unset `ICE_BOX_TUN_DEV_SUDO`; uses the installed app data directory |
| Windows TUN (G9.14) | `bash scripts/run-acceptance-windows-tun.sh` | Elevated Bash shell and MSVC toolchain |
| Linux core & system proxy | `bash scripts/run-acceptance-linux.sh` | Linux host (`gsettings` or KDE `kwriteconfig` for system proxy cases) |
| Linux TUN (G9.12) | `bash scripts/run-acceptance-linux-tun.sh` | Cache sudo credentials with `sudo -v` first |
| Linux helper (G9.13) | `bash scripts/run-acceptance-linux-tun.sh --helper` | systemd host and Polkit/sudo authorization for helper install/uninstall |
| Pinned core config check | `ICE_BOX_SINGBOX_BIN=third_party/sing-box/<target>/sing-box cargo test -p ice-config generated_configs_pass_pinned_core_check -- --ignored` | Bundled binary for the host (`bash scripts/fetch-singbox.sh`; e.g., `darwin-aarch64` or `linux-x86_64`) |
| System proxy | `cargo test -p ice-proxy-sys --lib -- --ignored --test-threads=1` | Native macOS, Windows, or Linux (GNOME/KDE) host |
| HTTPS fetch | `cargo test -p ice-subscription --lib https_fetch_succeeds -- --ignored` | Network access |
| HTTPS fetch through the app proxy | `ICE_BOX_TEST_PROXY=http://127.0.0.1:17890 cargo test -p ice-subscription --lib proxied_https_fetch_succeeds -- --ignored` | App running with the proxy on |
| MCP bridge | manual: enable MCP, start the copied stdio configuration, and call `initialize`, `tools/list`, `get_status`, then `get_settings`; repeat after a service restart | Desktop app running; no proxy/TUN privilege needed |
| macOS tray delay test | manual: click 「延迟测试」 / "Test Delay" on the top page and on a group page, then dismiss the menu mid-run | Real Mac with the app running; the menu must stay open with results streaming into the rows, and a dismissal must cancel the run and clear the results |

The helper script covers G9.13 and uninstalls the helper afterwards; G9.15 needs
a separately installed helper. Its data directory defaults to the installed
app's directory and can be set with `ICE_BOX_TUN_LIVE_DATA_DIR`. The tray row is
manual: the menu's AppKit tracking has no headless coverage, so run it by hand
when touching `tray.rs` / `tray_delay*`, and check the close that a start click
brings still leaves the menu open while Esc or a click away cancels (Windows and
Linux keep the plain tray menu).

For ordinary live core acceptance, use a serial run with the privileged TUN
cases excluded:

```bash
cargo test -p ice-box --lib acceptance::live:: -- --ignored --nocapture \
  --test-threads=1 --skip g9_12 --skip g9_13 --skip g9_14 --skip g9_15
```

This covers core lifecycle, reload, system-proxy restoration/port changes,
logs, and Clash profile/group/mode behavior. Test definitions and individual
ignore reasons live in [acceptance.rs](../apps/desktop/src-tauri/src/acceptance.rs).
