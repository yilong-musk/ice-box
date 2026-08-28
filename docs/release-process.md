# ice-box Release Process

This document describes the release process as executed for the first public
release (**v0.1.1**, 2026-08-28). It is the reference for subsequent releases;
update it whenever the process changes.

## Overview

A release is a **`vX.Y.Z` tag pushed to `main`**. The tag push triggers
`.github/workflows/release.yml`, which gates the workspace, builds the macOS
arm64 `.dmg` and the Windows NSIS `.exe`, and publishes a GitHub Release with
the artifacts and compliance notices.

No signing is configured yet (see [Signing](#signing)).

## Version sources

The version lives in exactly three places and **must stay in sync**:

| File | Key |
|------|-----|
| `Cargo.toml` | `[workspace.package] version` (all crates inherit via `version.workspace = true`) |
| `apps/desktop/package.json` | `"version"` (also displayed in the app UI) |
| `apps/desktop/src-tauri/tauri.conf.json` | `"version"` (installer metadata) |

`Cargo.lock` is refreshed automatically by `cargo check`.

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
bash scripts/gate-local.sh   # fmt, clippy, Rust tests, tsc, vitest
```

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
| `build-macos` | macos-latest | gate + headless acceptance + `tauri build` → upload DMG |
| `build-windows` | windows-latest | gate + headless acceptance + NSIS build (`npm run build:win`) → upload EXE |
| `publish` | ubuntu-latest | `needs` both build jobs; downloads artifacts, extracts the changelog section via `scripts/release-notes.sh`, creates the GitHub Release |

Published assets:

- `ice-box_<ver>_aarch64.dmg` (macOS Apple Silicon)
- `ice-box_<ver>_x64-setup.exe` (Windows NSIS)
- `LICENSE`, `NOTICE` (bundled sing-box is GPL-3.0-or-later; the `NOTICE` file
  satisfies the redistribution requirements, the upstream license text is
  attached as `third_party/sing-box/LICENSE`)

### 6. Verify

```bash
gh run list --workflow release.yml --limit 1   # conclusion: success
gh release view v0.1.2 --json assets           # expected assets present
```

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

## Signing

Signed releases (macOS notarization with an Apple Developer ID, Windows code
signing certificate) are **not configured yet**. First-run users see
Gatekeeper ("right-click to open") or SmartScreen warnings, which is expected.
Track signing as a separate milestone before any wider distribution.

## Future milestones

- Universal macOS build (`arm64 + x64`) once Intel demand justifies the build time
- Draft releases with manual publish for release-candidate verification