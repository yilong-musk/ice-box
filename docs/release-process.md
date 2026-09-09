# ice-box Release Process

This document describes the release process as executed for the first public
release (**v0.1.1**, 2026-08-28). It is the reference for subsequent releases;
update it whenever the process changes.

## Overview

A release is a **`vX.Y.Z` tag pushed to `main`**. The tag push triggers
`.github/workflows/release.yml`, which gates the workspace, builds the macOS
arm64 `.dmg` (plus updater `.app.tar.gz` / `.sig`) and the Windows NSIS `.exe`
(plus `.exe.sig`), synthesizes `latest.json`, and publishes a GitHub Release with
the artifacts and compliance notices.

macOS releases are permanently unsigned at the Apple / Gatekeeper layer
(documented product decision): the privileged helper for TUN is installed
through the system authorization dialog at first use (see `docs/tun.md`).
Gatekeeper warnings are expected for published artifacts. In-app updates use a
separate **minisign** key for integrity; that is not Developer ID signing.

## Version sources

The version lives in exactly three places and **must stay in sync**:

| File | Key |
|------|-----|
| `Cargo.toml` | `[workspace.package] version` (all crates inherit via `version.workspace = true`) |
| `apps/desktop/package.json` | `"version"` (also displayed in the app UI and GitHub Pages marketing chrome) |
| `apps/desktop/src-tauri/tauri.conf.json` | `"version"` (installer metadata) |

`Cargo.lock` is refreshed automatically by `cargo check`.

## Updater signing keys (one-time)

In-app updates (`docs/architecture.md` §25) verify artifacts with minisign.
Generate the keypair once (not in the repo):

```bash
cd apps/desktop && npm run tauri signer generate -- -w ~/.tauri/ice-box.key
```

| Material | Where |
|----------|--------|
| Public key | `apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey` (committed) |
| Private key | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` |
| Key password | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |

Losing the private key means already-installed clients cannot verify a new key;
rotating it abandons those clients (they must reinstall by hand). CI **gate +
build** jobs must **not** set `createUpdaterArtifacts` and do not need these
secrets (fork PRs would otherwise fail). Only `release.yml` merges
`src-tauri/tauri.updater.conf.json` and injects the secrets.

## Step-by-step

### 1. Bump the version

```bash
bash scripts/bump-version.sh 0.1.2
```

The script rewrites all three version sources, verifies each pattern was found,
and refreshes `Cargo.lock` (`cargo check --workspace --quiet`).

### 2. Update the changelog

Add a section to `CHANGELOG.md` (Keep a Changelog style):

```markdown
## [0.1.2] - 2026-09-01

### Added
...
```

Release notes for the GitHub Release are extracted from this section by
`scripts/release-notes.sh`. Verify locally before tagging:

```bash
bash scripts/release-notes.sh v0.1.2
```

### 3. Gate and merge to `main`

```bash
bash scripts/gate-local.sh   # fmt, clippy, Rust tests, tsc, vitest; recaptures docs/images/home.png when the version changed
```

Commit the updated `docs/images/home.png` and `docs/images/home.version` with the release.

Open a PR from a `release/vX.Y.Z` branch and merge into `main`. `main` has
branch protection: all CI checks (`gate (linux)`, `gate + build (macOS dmg)`,
`gate + build (Windows nsis)`) must pass before the merge is allowed. To
queue the merge, pass `--auto` to `gh pr merge`.

### 4. Tag and push

Wait for the CI run on the merged `main` to be green, then tag and push:

```bash
git tag -a v0.1.2 -m "ice-box v0.1.2"
git push origin v0.1.2
```

Pushing the tag is the point of no return: it triggers the release pipeline.

### 5. Release pipeline

`.github/workflows/release.yml` (`on: push: tags: ["v*"]`):

| Job | Runner | Work |
|-----|--------|------|
| `build-macos` | macos-latest | gate + headless acceptance + `tauri build --config src-tauri/tauri.updater.conf.json` (signing secrets) → upload DMG + `*.app.tar.gz` + `.sig` |
| `build-windows` | windows-latest | gate + headless acceptance + NSIS (`npm run build:win -- --config src-tauri/tauri.updater.conf.json`, stacked on `tauri.windows.conf.json`) → upload EXE + `.exe.sig` |
| `publish` | ubuntu-latest | `needs` both build jobs; downloads artifacts; extracts the changelog section via `scripts/release-notes.sh`; `scripts/merge-updater-latest.sh` writes `latest.json`; creates the GitHub Release |

Published assets:

- `ice-box_<ver>_aarch64.dmg` (macOS Apple Silicon, first-time install)
- `ice-box.app.tar.gz` and `ice-box.app.tar.gz.sig` (macOS updater payload; exact names follow Tauri)
- `ice-box_<ver>_x64-setup.exe` (Windows NSIS, first-time install)
- `ice-box_<ver>_x64-setup.exe.sig` (Windows updater signature)
- `latest.json` (`version`, `notes`, `pub_date`, `platforms.darwin-aarch64` /
  `platforms.windows-x86_64`; each `signature` is the **full `.sig` file text**,
  each `url` is `https://github.com/yilong-musk/ice-box/releases/download/<tag>/<asset>`)
- `LICENSE`, `NOTICE` (bundled sing-box is GPL-3.0-or-later; the `NOTICE` file
  satisfies the redistribution requirements, the upstream license text is
  attached as `third_party/sing-box/LICENSE`)

### 6. Verify

```bash
gh run list --workflow release.yml --limit 1   # conclusion: success
gh release view v0.1.2 --json assets           # dmg, exe, tar.gz, sigs, latest.json
```

`latest.json` must list both `darwin-aarch64` and `windows-x86_64` with non-empty
signatures. Fixture coverage: `bash scripts/test-merge-updater-latest.sh`
(also run from `scripts/gate.sh` / `scripts/gate-local.sh`).

## Local vs CI gates

`scripts/gate-local.sh` is the pre-commit gate. It is intentionally lighter
than CI:

- Excludes the `ice-box` desktop crate (`cargo clippy` / `cargo test --lib
  --exclude ice-box`) because GTK/webkit is often missing on developer
  machines. Desktop-crate tests (`apps/desktop/src-tauri`, including the
  `g9_*` acceptance filters) run only in CI.
- Runs `cargo test --workspace --lib` plus `cargo test -p ice-tun-sys
  --tests` (the host-free TUN integration tests). It does not run the full
  `cargo test --workspace` (doc tests and other crates' integration tests).
- Skips the desktop Vite production build (`npm run build`); CI
  `scripts/gate.sh` runs it.

The Rust toolchain is pinned in `rust-toolchain.toml` (CI-3). GeoIP rule-set
fetches (`scripts/fetch-geoip.sh`) pin `third_party/sing-geoip/REF` and
verify SHA-256 against `third_party/sing-geoip/CHECKSUMS.sha256` (CI-8);
the committed checksum file is the trust root, as with
`third_party/sing-box/CHECKSUMS.sha256`.

CI `scripts/gate.sh` runs `cargo test --workspace --exclude ice-box` (lib
+ integration + doc) and `cargo test -p ice-box --lib`. Linux runs the
full gate (`GATE_SCOPE=all`). macOS and Windows each have a rust-only
test job (`GATE_SCOPE=rust`) in parallel with a packaging job; `g9_*`
and `ice-proxy-sys` are covered by that test job, not by extra steps.
Ignored live tests and how to run them are listed in
[`testing.md`](testing.md).

## Known issues and workarounds

### Network instability to github.com

Direct connections to `github.com` from some networks time out intermittently
(HTTP/2 framing errors, port 443 connect failures). Use the local proxy
(`127.0.0.1:17890` on the dev machine) per command:

```bash
git -c http.proxy=http://127.0.0.1:17890 push origin v0.1.2
```

or set it once for the repository:

```bash
git config http.proxy http://127.0.0.1:17890
```

`gh` (the GitHub CLI) is not affected and can be used as a fallback check
during outages. Retry with a short sleep between attempts; connections usually
recover within a minute or two.

### Node.js 20 deprecation warning in CI

`actions/download-artifact@v6` still runs on Node 20 and is forced to Node 24
on GitHub-hosted runners (warning only). Fixed by `@v7`; do not downgrade.
`actions/upload-artifact@v6` already runs on Node 24.

## Distribution constraints

The macOS release is intentionally unsigned: Developer ID signing,
notarization, and stapling are not part of the product. TUN elevation uses
the system authorization dialog (`AuthorizationServices`, deprecated but
functional) to install the privileged helper on first use; the manual
equivalent is `scripts/install-helper-macos.sh`. G9.12 and G9.13 are the
completed macOS TUN live gates; the clean-machine install/uninstall gate is
intentionally waived. Published `.app`/`.dmg` artifacts are unsigned and may
trigger Gatekeeper warnings; users must right-click → Open (or use
`xattr -dr com.apple.quarantine`) on first launch.

In-app updates after 0.1.5 do not change that decision: they only check minisign
signatures. Windows NSIS installers remain without Authenticode.

## Future milestones

- Universal macOS build (`arm64 + x64`) once Intel demand justifies the build time
- Draft releases with manual publish for release-candidate verification
