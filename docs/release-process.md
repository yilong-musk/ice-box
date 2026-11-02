# ice-box Release Process

This document describes the release process as executed for the first public
release (**v0.1.1**, 2026-08-28). It is the reference for subsequent releases;
update it whenever the process changes.

## Overview

A release is a **`vX.Y.Z` tag pushed to `main`**. The tag push triggers
`.github/workflows/release.yml`, which first validates the tag in a fast
preflight job, runs the full gate once on Linux, builds the macOS arm64 `.dmg`
(plus updater `.app.tar.gz` / `.sig`), the Windows NSIS `.exe` (plus
`.exe.sig`), and one signed Android `arm64-v8a` APK, then stages everything as
a **draft** GitHub Release. The publish job verifies the assets, writes
`SHA256SUMS`, checks the updater signatures against the committed pubkey, and
only then publishes; publishing makes the release the updater feed target.
Pre-release tags (`vX.Y.Z-rc.N`) are published as GitHub pre-releases and do
not contain `latest.json`.

macOS releases are ad-hoc signed at the bundle level (documented product
decision: no Developer ID certificate, no notarization). TUN elevation is
documented in [`tun.md`](tun.md#macos-elevation).
First launch still shows an "unidentified developer" Gatekeeper prompt, which
users can override (System Settings → Privacy & Security → Open Anyway on
macOS 15+; right-click → Open on older versions).
CI (`ci.yml` / `release.yml`) asserts the bundle signature with
`scripts/verify-macos-code-sign.sh`, so a signing regression cannot ship.
In-app updates use a separate **minisign** key for integrity; that is not
Developer ID signing.

## Version sources

The version lives in these places and **must stay in sync**:

| File | Key |
|------|-----|
| `Cargo.toml` | `[workspace.package] version` (all crates inherit via `version.workspace = true`) |
| `apps/desktop/package.json` | `"version"` (also displayed in the app UI and GitHub Pages marketing chrome) |
| `apps/desktop/src-tauri/tauri.conf.json` | `"version"` (installer metadata) |
| `apps/mobile/package.json` | `"version"` |
| `apps/mobile/src-tauri/tauri.conf.json` | `"version"` and `bundle.android.versionCode` |
| `apps/mobile/src-tauri/gen/android/app/tauri.properties` | `tauri.android.versionName` and `tauri.android.versionCode` (rewritten by the bump script; `tauri android build` regenerates the same values) |

`Cargo.lock` is refreshed automatically by `cargo check`.
`scripts/release-preflight.sh` checks every source above (including
`apps/desktop/package.json` and `tauri.properties`) before the release builds
start; a mismatch fails the preflight instead of the publish job.

The Android version code is `major * 1000000 + minor * 1000 + patch` (the same
integer Tauri uses when `versionCode` is unset). Minor and patch must stay below
1000, otherwise they carry into the next component and collide with a different
release. A pre-release suffix does not change it, so do not publish both a
pre-release and a final build of the same `X.Y.Z` (or two pre-releases of the
same `X.Y.Z`): Android will not treat the later build as an upgrade. `0.0.0` is
rejected.

## Updater signing keys (one-time)

In-app updates verify artifacts with minisign; see the
[architecture overview](architecture.md#trust-boundaries) for the trust boundary.
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
rotating it abandons those clients (they must reinstall by hand). Back the key
up offline before the first release, together with the password: keep an
encrypted copy in a password manager or on offline media, and verify the
backup by signing a throwaway artifact with it. CI **gate + build** jobs must
**not** set `createUpdaterArtifacts` and do not need these secrets (fork PRs
would otherwise fail). Only `release.yml` merges
`src-tauri/tauri.updater.conf.json` and injects the secrets. The publish job
checks that every updater `.sig` was produced by the committed public key
before it publishes; a key mismatch fails the release instead of breaking every
installed client.

To rotate the key without abandoning installed clients, first publish a regular
release signed with the **old** key whose binary embeds the **new** public key
in `plugins.updater.pubkey`; clients that install it can then verify releases
signed with the new key. Only after that release reaches users should the CI
secrets switch to the new private key.

## Step-by-step

### 1. Bump the version

```bash
bash scripts/bump-version.sh 0.1.2
```

The script rewrites every version source above, verifies each pattern was found,
and refreshes `Cargo.lock` (`cargo check --workspace --quiet`).

### 2. Update the changelog

Add a section to `CHANGELOG.md` (Keep a Changelog style):

```markdown
## [0.1.2] - 2026-08-31

### Added
...
```

The GitHub Release body is deliberately short: `scripts/release-notes.sh` prints
the fixed summary line `Fixes and improvements` and a link to this section in
`CHANGELOG.md`. The full per-release notes stay in the changelog and are never
pasted into the release body. Verify locally before tagging:

```bash
bash scripts/release-notes.sh v0.1.2   # fixed summary + changelog link
```

The script fails when the version's changelog section is missing or empty, so
run it before tagging.

### 3. Gate and merge to `main`

```bash
bash scripts/gate-local.sh
```

The gate recaptures the documentation screenshots when the version or the UI
changed, and recomposes the README product shot from them on every run. To
refresh them explicitly:

```bash
bash scripts/capture-demo-home.sh
bash scripts/capture-demo-mobile.sh
(cd apps/website && npm run capture-hero)
```

The capture scripts write the desktop and phone screenshots plus their version
files to `docs/images/`. Those intermediates are ignored by git; commit only the
composed `docs/images/hero.png` and `docs/images/hero.zh-CN.png`.

Open a PR from a `release/vX.Y.Z` branch and merge into `main`. `main` has
branch protection: all CI checks (`gate (linux)`, `gate + build (macOS dmg)`,
`gate + build (Windows nsis)`) must pass before the merge is allowed. To
queue the merge, pass `--auto` to `gh pr merge`.

The release workflow repeats the full gate once on Linux and runs
`GATE_SCOPE=rust` on macOS and Windows, so a release is never built from a tree
that only passed the local gate.

### 4. Tag and push

Wait for the CI run on the merged `main` to be green, then tag and push:

```bash
git tag -a v0.1.2 -m "ice-box v0.1.2"
git push origin v0.1.2
```

The tag preflight requires the tag commit to be reachable from `main` and the
version to be newer than the latest published release. It is the last cheap
check: pushing the tag starts the release pipeline, and the tag is public even
when the build later fails. Never move or reuse a tag after it has been pushed;
cut a new patch version instead.

A pre-release uses the same flow with a suffix, for example
`git tag -a v0.1.2-rc.1 -m "ice-box v0.1.2-rc.1"`. Pre-release tags are
published as GitHub pre-releases, are excluded from `/releases/latest`, and do
not publish `latest.json`, so installed clients never receive them
automatically. Android `versionCode` is unchanged by the suffix; do not publish
both `X.Y.Z-rc.N` and `X.Y.Z` (or multiple pre-releases of the same core
version), or Android will not treat the later build as an upgrade.

### 5. Release pipeline

`.github/workflows/release.yml` (`on: push: tags: ["v*"]`):

| Job | Runner | Work |
|-----|--------|------|
| `preflight` | ubuntu-latest | tag is on `main`, every version source matches, changelog section exists, signing secrets are present, version is newer than the latest release |
| `gate-linux` | ubuntu-latest | full workspace gate (formatting, clippy, tests, frontend, website, mobile demo) |
| `build-macos` | macos-latest | `GATE_SCOPE=rust` gate + `tauri build --config src-tauri/tauri.updater.conf.json` (signing secrets); asserts the binary is arm64, the DMG is `ice-box_<ver>_aarch64.dmg`, and the bundle signature; uploads DMG + `*.app.tar.gz` + `.sig` |
| `build-windows` | windows-latest | `GATE_SCOPE=rust` gate + NSIS (`npm run build:win -- --config src-tauri/tauri.updater.conf.json`, stacked on `tauri.windows.conf.json`); asserts the `ice-box_<ver>_x64-setup.exe` name and payload; uploads EXE + `.exe.sig` |
| `build-android` | ubuntu-latest | signed `arm64-v8a` APK (`scripts/build-android-apk.sh release arm64`) → `scripts/verify-android-apk.sh --release --version <ver>` → upload `ice-box_<ver>_android_arm64.apk` |
| `publish` | ubuntu-latest | `needs` the gate and all three build jobs; downloads artifacts; builds the short release body; verifies updater signatures against the committed pubkey; writes `SHA256SUMS`; creates a **draft** release, uploads, compares the draft asset list, then publishes (pre-release when the tag has a suffix) and runs `scripts/verify-release-assets.sh` |

The `publish` job uses the `release` environment. Configure required reviewers
on that environment to make publication wait for a human; see
[Repository settings](#repository-settings-one-time).

Published assets:

- `ice-box_<ver>_aarch64.dmg` (macOS Apple Silicon, first-time install)
- `ice-box.app.tar.gz` and `ice-box.app.tar.gz.sig` (macOS updater payload; exact names follow Tauri)
- `ice-box_<ver>_x64-setup.exe` (Windows NSIS, first-time install)
- `ice-box_<ver>_x64-setup.exe.sig` (Windows updater signature)
- `ice-box_<ver>_android_arm64.apk` (Android arm64-v8a, signed with the project keystore)
- `SHA256SUMS` (checksums for every asset in the release)
- `latest.json` (`version`, `notes`, `pub_date`, `platforms.darwin-aarch64` /
  `platforms.windows-x86_64`; each `signature` is the **full `.sig` file text**,
  each `url` is `https://github.com/yilong-musk/ice-box/releases/download/<tag>/<asset>`).
  Final releases only; pre-releases do not include it.
- `LICENSE`, `NOTICE`, `sing-box-LICENSE` (root GPLv3 plus bundled sing-box
  license; the upstream text is copied from `third_party/sing-box/LICENSE`
  because two GitHub Release assets cannot share the name `LICENSE`)

The release is created as a draft first, so a failed asset upload never becomes
`/releases/latest`. Rerunning a failed `publish` job resumes the draft; if the
release was already published, it verifies the existing assets and does not
overwrite them.

### 6. Verify

```bash
gh run list --workflow release.yml --limit 1   # conclusion: success
gh release view v0.1.2 --json assets           # dmg, exe, apk, tar.gz, sigs, SHA256SUMS, latest.json
gh release view v0.1.2 --json body             # "Fixes and improvements" + CHANGELOG.md link
bash scripts/verify-release-assets.sh v0.1.2   # asset URLs, checksum manifest, latest.json
```

`verify-release-assets.sh` checks releases produced by this pipeline. Releases
published before `SHA256SUMS` was added do not have that asset and will fail the
check.

The release body is the fixed summary plus a changelog link from
`scripts/release-notes.sh`; the link is pinned to the tag, not `main`, so it
keeps pointing at the notes that shipped. The full notes stay one click away in
`CHANGELOG.md`.

`latest.json` must list both `darwin-aarch64` and `windows-x86_64` with non-empty
signatures. Fixture coverage: `bash scripts/test-merge-updater-latest.sh`
(also run from `scripts/gate.sh` / `scripts/gate-local.sh`).

The website deploys only after a final release for the current
`apps/desktop/package.json` version is published. A failed release leaves the
site on the previous version instead of exposing download links that 404.

## Release recovery

| Situation | Action |
|-----------|--------|
| Preflight or a build job fails | Fix the cause and re-run the workflow if the tag can stay. If the tag itself is wrong, cut a new patch version; once a release has been published, the version is never reused. |
| `publish` fails before the release is published | Rerun the failed `publish` job. It finds the draft, refreshes the notes, uploads the assets again, and publishes only after the asset list matches. |
| `publish` fails after the release is published | Rerun the job; it detects the published release, verifies the existing assets, and does not overwrite them. Fix the cause and cut a new patch if the assets themselves are wrong. |
| A published release is broken | Cut a higher patch with the fix. Deleting the bad release only makes `/releases/latest` point at the previous version for clients that have not updated yet; Tauri never downgrades, so clients already on the bad version stay there until a higher version exists. |
| A pre-release is broken | Publish another pre-release (`rc.2`, ...). Pre-releases are not in `/releases/latest`, so installed clients are unaffected. |

Do not edit or replace assets on a published release: `latest.json` embeds the
signature and URL of each updater payload, and clients may already be
downloading it. Forward-fix instead.

## Repository settings (one-time)

These settings are not stored in the repository; configure them once in
GitHub:

- **Release environment**: `Settings → Environments → release`. Add required
  reviewers so `publish` waits for a human after the builds pass. The preflight
  and build jobs read repository secrets; if you move the signing secrets into
  this environment instead, add `environment: release` to those jobs too (each
  referencing job is approved separately).
- **Tag protection**: create a tag ruleset for `v*` that restricts updates and
  deletions to maintainers and, if desired, requires signed tags. Without it,
  anyone with write access can push a tag that starts the release pipeline.
- **Branch protection**: keep the required checks (`gate (linux)`,
  `gate + build (macOS dmg)`, `gate + build (Windows nsis)`) on `main`.
- **Secrets**: set `TAURI_SIGNING_PRIVATE_KEY`,
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `ANDROID_KEYSTORE_BASE64`,
  `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, and
  `ANDROID_KEY_PASSWORD`. The preflight checks the updater key and all four
  Android secrets before any build; the updater-key password may be empty for
  an unencrypted key. Back up the updater private key offline; see
  [Updater signing keys](#updater-signing-keys-one-time).

The workflow has read-only permissions by default; only the `publish` job can
write to the repository.

## Local vs CI gates

Run `scripts/gate-local.sh` before every commit and require all CI checks to
pass before merging or tagging, as described above. The maintained reference
for gate coverage, platform splits, and manual live tests is
[`testing.md`](testing.md).

The Rust toolchain is pinned in `rust-toolchain.toml` (CI-3). GeoIP rule-set
fetches (`scripts/fetch-geoip.sh`) pin `third_party/sing-geoip/REF` and
verify SHA-256 against `third_party/sing-geoip/CHECKSUMS.sha256` (CI-8);
the committed checksum file is the trust root, as with
`third_party/sing-box/CHECKSUMS.sha256`.

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

## Distribution constraints

The macOS release is ad-hoc signed: Developer ID signing, notarization, and
stapling are not part of the product (documented product decision). TUN
elevation and the helper installation gate exception are maintained in
[`tun.md`](tun.md#macos-elevation). The `.app` inside the published `.dmg`
carries the complete ad-hoc signature, asserted by CI through
`scripts/verify-macos-code-sign.sh`; the DMG container itself is not signed,
which is expected for the ad-hoc identity. First launch still shows an
"unidentified developer" Gatekeeper prompt; users allow it via System
Settings → Privacy & Security → Open Anyway (macOS 15+), right-click → Open
(older versions), or `xattr -dr com.apple.quarantine`.

In-app updates after 0.1.5 do not change that decision: they only check minisign
signatures. Windows NSIS installers remain without Authenticode. Because the
DMG and EXE are not platform-signed, every release also publishes `SHA256SUMS`
so first-time downloaders can verify the files they saved; the updater `.sig`
files cover the in-app update payloads only.

## Android signing (one-time)

Android APKs are signed with one project release keystore. It is not Developer
ID, Play App Signing, or the minisign updater key. The Tauri updater does not
support Android, so the APK is a normal GitHub Release asset and is not listed
in `latest.json`. CI debug APKs are signed with the Android debug key only.

Generate the keystore once, on a machine you control, and keep a copy offline.
Losing it means installed clients must uninstall before they can take a newer
build.

```bash
export ICE_BOX_ANDROID_KEYSTORE_PASSWORD='...'
export ICE_BOX_ANDROID_KEY_PASSWORD='...'
bash scripts/generate-android-keystore.sh "$HOME/.local/share/ice-box/ice-box-release.jks"
```

The script refuses to write the file inside the repository. It uses a JKS
keystore so the store password and the key password stay independent.
`keytool` warns that JKS is a proprietary format; that warning is expected.
Set the GitHub Actions secrets it prints:

| Secret | Value |
|--------|--------|
| `ANDROID_KEYSTORE_BASE64` | base64 of the `.jks` file, one line |
| `ANDROID_KEYSTORE_PASSWORD` | store password |
| `ANDROID_KEY_ALIAS` | `ice-box`, unless you overrode `ICE_BOX_ANDROID_KEY_ALIAS` |
| `ANDROID_KEY_PASSWORD` | key password |

`release.yml` fails the preflight when any of those secrets is empty, before it
spends a build. A local signed build uses the file path instead of the base64
secret:

```bash
export ICE_BOX_ANDROID_KEYSTORE="$HOME/.local/share/ice-box/ice-box-release.jks"
export ICE_BOX_ANDROID_KEYSTORE_PASSWORD='...'
export ICE_BOX_ANDROID_KEY_ALIAS=ice-box
export ICE_BOX_ANDROID_KEY_PASSWORD='...'
bash scripts/build-android-apk.sh release arm64
```

`scripts/verify-android-apk.sh --release --version <ver>` rejects an APK signed
with the Android debug key, an APK that contains any ABI other than
`arm64-v8a`, and an APK whose manifest `versionName` does not match the tag.
The `x86_64` APK is a local emulator artifact (`debug amd64`) and is not
published.

## Future milestones

- Universal macOS build (`arm64 + x64`) once Intel demand justifies the build time
- Staged rollout and automatic rollback of the updater feed
