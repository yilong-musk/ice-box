# Testing

How automated tests are split, which ones CI actually runs, and which
`#[ignore]` tests need a real host.

Companion: `scripts/gate.sh` (CI), `scripts/gate-local.sh` (pre-commit),
`docs/release-process.md` (local vs CI gates).

## What runs where

| Command | Scope |
|---|---|
| `scripts/gate-local.sh` | `cargo fmt --check`, clippy (excluding `ice-box`), `cargo test --workspace --lib --exclude ice-box`, `cargo test -p ice-tun-sys --tests`, desktop + website `tsc`, vitest, updater-fixture script, Live Demo screenshot |
| `scripts/gate.sh` (CI) | The above plus clippy for **all** crates, `cargo test --workspace --exclude ice-box` (lib + integration + doc), `cargo test -p ice-box --lib`, Vite production build |
| CI macOS / Windows extra steps | `cargo test -p ice-box --lib 'g9_'` (headless acceptance), `cargo test -p ice-proxy-sys` |

`cargo test --lib` never compiles or runs `crates/*/tests/*.rs`. Those
integration binaries (TUN recovery, macOS/Windows backends, helper e2e)
are the riskiest tests in the tree; CI now executes them via
`cargo test --workspace --exclude ice-box`. The desktop crate is tested
with `cargo test -p ice-box --lib`. On Windows that harness needs the
Common Controls v6 manifest (`apps/desktop/src-tauri/build.rs`); Tauri
only embeds it on the app exe, and without it the test process dies at
load (`STATUS_ENTRYPOINT_NOT_FOUND`).

## Ignored tests (need hardware, privileges, or the network)

Each `#[ignore]` has a reason string in source. They are **not** run by
the gates. Run them by hand when changing the matching subsystem.

### Live TUN / helper (destructive on the host)

| Test | File | Manual command |
|---|---|---|
| macOS live host reads | `crates/ice-tun-sys/tests/macos_backend.rs` | `cargo test -p ice-tun-sys --test macos_backend -- --ignored` on a real Mac |
| Windows live host + elevation | `crates/ice-tun-sys/tests/windows_backend.rs` | `scripts/run-acceptance-windows-tun.sh` |
| macOS TUN gate (sudo) | `apps/desktop/src-tauri/src/acceptance.rs` (`g9_12…`) | `scripts/run-acceptance-macos-tun.sh` |
| macOS helper install/enable | `acceptance.rs` (`g9_13…`) | `scripts/run-acceptance-macos-tun.sh --helper` |
| Windows TUN gate (elevated) | `acceptance.rs` (`g9_14…`) | `scripts/run-acceptance-windows-tun.sh` |

### Live sing-box (bundled binary, mutates ports / proxy)

All in `apps/desktop/src-tauri/src/acceptance.rs`, marked
`live: real sing-box…`. Run with a fetched binary:

```bash
./scripts/fetch-singbox.sh
cargo test -p ice-box --lib -- --ignored --nocapture
```

These cover: start/stop, system proxy, reload, port change, logs, a full
Clash profile, Clash API group state, and Clash API mode switch.

### Live system proxy (mutates OS settings)

| Test | File |
|---|---|
| WinInet apply/restore | `crates/ice-proxy-sys/src/lib.rs` (`proxy_sys: mutates real WinInet Internet Settings`) |
| macOS `networksetup` apply/restore | `crates/ice-proxy-sys/src/lib.rs` (`proxy_sys: mutates real macOS network settings`) |

Run only on a disposable host: `cargo test -p ice-proxy-sys -- --ignored`.

### Network

| Test | File |
|---|---|
| Live HTTPS fetch after SSRF validation | `crates/ice-subscription/src/fetch.rs` |

```bash
cargo test -p ice-subscription --lib -- --ignored
```
