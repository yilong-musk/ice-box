#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install or uninstall the cross-platform headless ice-box CLI (`ice-box-cli`)
# on macOS (arm64) and Linux (x64, arm64) from GitHub Releases.
#
# Quick install:
#   curl -fsSL https://raw.githubusercontent.com/yilong-musk/ice-box/main/scripts/install-cli.sh | bash
#
# Options (or equivalent ICE_BOX_* environment variables):
#   --version <ver>       Release version (e.g. 0.4.2 or v0.4.2; default: latest)
#   --install-dir <dir>   Install root (default: ~/.local/share/ice-box-cli)
#   --bin-dir <dir>       Symlink directory (default: ~/.local/bin)
#   --platform <alias>    Override target (mac-arm64|linux-x64|linux-arm64)
#   --repo <owner/name>   GitHub repository (default: yilong-musk/ice-box)
#   --no-verify           Skip SHA256SUMS verification (not recommended)
#   --skip-smoke          Skip running `ice-box-cli --version` after install
#   --uninstall           Remove the installed CLI and its symlink
#   -h, --help            Show this help message
set -euo pipefail

_ICE_BOX_CLI_WORK=""

cleanup() {
  if [[ -n "${_ICE_BOX_CLI_WORK:-}" ]]; then
    rm -rf "$_ICE_BOX_CLI_WORK"
  fi
}

main() {
  local repo="${ICE_BOX_REPO:-yilong-musk/ice-box}"
  local version="${ICE_BOX_VERSION:-}"
  local install_dir="${ICE_BOX_INSTALL_DIR:-$HOME/.local/share/ice-box-cli}"
  local bin_dir="${ICE_BOX_BIN_DIR:-$HOME/.local/bin}"
  local platform_override="${ICE_BOX_PLATFORM:-}"
  local no_verify="${ICE_BOX_NO_VERIFY:-0}"
  local skip_smoke="${ICE_BOX_SKIP_SMOKE:-0}"
  local uninstall="${ICE_BOX_UNINSTALL:-0}"

  usage() {
    cat <<'EOF'
Install or uninstall ice-box-cli (macOS arm64, Linux x64 / arm64).

Usage:
  install-cli.sh [options]

Options:
  --version <ver>       Release version (e.g. 0.4.2 or v0.4.2; default: latest)
  --install-dir <dir>   Install root (default: ~/.local/share/ice-box-cli)
  --bin-dir <dir>       Symlink directory (default: ~/.local/bin)
  --platform <alias>    Override target (mac-arm64|linux-x64|linux-arm64)
  --repo <owner/name>   GitHub repository (default: yilong-musk/ice-box)
  --no-verify           Skip SHA256SUMS verification
  --skip-smoke          Skip post-install `--version` verification
  --uninstall           Remove installed files and symlink
  -h, --help            Show this help
EOF
  }

  die() {
    echo "install-cli: $*" >&2
    exit 1
  }

  while (($# > 0)); do
    case "$1" in
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      version="$2"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || die "--install-dir requires a value"
      install_dir="$2"
      shift 2
      ;;
    --bin-dir)
      [[ $# -ge 2 ]] || die "--bin-dir requires a value"
      bin_dir="$2"
      shift 2
      ;;
    --platform)
      [[ $# -ge 2 ]] || die "--platform requires a value"
      platform_override="$2"
      shift 2
      ;;
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a value"
      repo="$2"
      shift 2
      ;;
    --no-verify)
      no_verify=1
      shift
      ;;
    --skip-smoke)
      skip_smoke=1
      shift
      ;;
    --uninstall)
      uninstall=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "install-cli: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
    esac
  done

  if [[ "$uninstall" == "1" ]]; then
    local link_path="$bin_dir/ice-box-cli"
    if [[ -L "$link_path" ]]; then
      local target
      target="$(readlink "$link_path" 2>/dev/null || true)"
      case "$target" in
      "$install_dir"/*)
        rm -f "$link_path"
        echo "Removed symlink: $link_path"
        ;;
      *)
        echo "Skipped $link_path (points outside $install_dir: $target)"
        ;;
      esac
    fi
    if [[ -d "$install_dir" ]]; then
      rm -rf "$install_dir"
      echo "Removed install directory: $install_dir"
    else
      echo "Nothing to remove under $install_dir"
    fi
    return 0
  fi

  if [[ ! "$repo" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
    die "invalid repository name: $repo"
  fi

  # Detect target archive suffix.
  local archive_target=""
  local is_macos=0
  if [[ -n "$platform_override" ]]; then
    case "$platform_override" in
    mac-arm64 | darwin_aarch64) archive_target="darwin_aarch64"; is_macos=1 ;;
    mac-x64 | darwin_x86_64)
      # Releases publish Apple Silicon archives only. A custom mirror may
      # still host the darwin_x86_64 archive package-cli.sh can build.
      if [[ -z "${ICE_BOX_RELEASE_BASE_URL:-}" ]]; then
        die "prebuilt macOS CLI archives currently ship for Apple Silicon (arm64) only; build locally with: bash scripts/package-cli.sh --platform mac-x64"
      fi
      archive_target="darwin_x86_64"
      is_macos=1
      ;;
    linux-x64 | linux_x86_64) archive_target="linux_x86_64" ;;
    linux-arm64 | linux_aarch64) archive_target="linux_aarch64" ;;
    *)
      die "unsupported --platform '$platform_override' (expected mac-arm64, linux-x64, or linux-arm64)"
      ;;
    esac
  else
    local os_name arch_name
    os_name="$(uname -s)"
    arch_name="$(uname -m)"
    case "$os_name" in
    Darwin)
      is_macos=1
      case "$arch_name" in
      arm64 | aarch64) archive_target="darwin_aarch64" ;;
      x86_64)
        die "prebuilt macOS CLI archives currently ship for Apple Silicon (arm64) only; build locally with: bash scripts/package-cli.sh --platform mac-x64"
        ;;
      *)
        die "unsupported macOS architecture: $arch_name"
        ;;
      esac
      ;;
    Linux)
      case "$arch_name" in
      x86_64 | amd64) archive_target="linux_x86_64" ;;
      aarch64 | arm64) archive_target="linux_aarch64" ;;
      *)
        die "unsupported Linux architecture: $arch_name"
        ;;
      esac
      ;;
    *)
      die "unsupported operating system '$os_name' (on Windows, use scripts/install-cli.ps1)"
      ;;
    esac
  fi

  command -v tar >/dev/null 2>&1 || die "tar is required"

  local downloader=""
  if command -v curl >/dev/null 2>&1; then
    downloader="curl"
  elif command -v wget >/dev/null 2>&1; then
    downloader="wget"
  else
    die "curl or wget is required"
  fi

  download_file() {
    local url="$1"
    local dest="$2"
    if [[ "$downloader" == "curl" ]]; then
      curl -fsSL --retry 3 --retry-delay 1 -o "$dest" "$url"
    else
      wget -qO "$dest" "$url"
    fi
  }

  resolve_latest_tag() {
    local latest_url="${ICE_BOX_LATEST_URL:-https://github.com/${repo}/releases/latest}"
    local effective=""
    if [[ "$downloader" == "curl" ]]; then
      effective="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest_url" 2>/dev/null || true)"
    else
      effective="$(wget --max-redirect=5 --spider -S "$latest_url" 2>&1 | awk '/^[[:space:]]*[Ll]ocation:/ {loc=$2} END {gsub(/\r$/, "", loc); print loc}' || true)"
    fi
    if [[ "$effective" == */releases/tag/* ]]; then
      printf '%s\n' "${effective##*/releases/tag/}"
      return 0
    fi
    # Fallback to GitHub REST API if the redirect was not exposed.
    local api_url="https://api.github.com/repos/${repo}/releases/latest"
    local body=""
    if [[ "$downloader" == "curl" ]]; then
      body="$(curl -fsSL -H "Accept: application/vnd.github+json" "$api_url" 2>/dev/null || true)"
    else
      body="$(wget -qO- "$api_url" 2>/dev/null || true)"
    fi
    local tag
    tag="$(printf '%s\n' "$body" | sed -nE 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' | head -n 1)"
    [[ -n "$tag" ]] || die "failed to resolve latest release tag from $latest_url; pass --version <vX.Y.Z> explicitly"
    printf '%s\n' "$tag"
  }

  local tag=""
  if [[ -z "$version" ]]; then
    tag="$(resolve_latest_tag)"
  else
    tag="v${version#v}"
  fi
  local bare_version="${tag#v}"
  if [[ ! "$bare_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
    die "invalid release version: $tag"
  fi

  local stem="ice-box-cli_${bare_version}_${archive_target}"
  local archive_name="${stem}.tar.gz"
  local base_url="${ICE_BOX_RELEASE_BASE_URL:-https://github.com/${repo}/releases/download/${tag}}"
  base_url="${base_url%/}"

  local work
  work="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-cli-install-XXXXXX")"
  _ICE_BOX_CLI_WORK="$work"
  trap cleanup EXIT

  echo "Downloading ${archive_name} (${tag})..."
  if ! download_file "${base_url}/${archive_name}" "$work/$archive_name"; then
    die "failed to download ${base_url}/${archive_name} (CLI archives are published for releases after v0.4.1)"
  fi

  if [[ "$no_verify" != "1" ]]; then
    echo "Verifying SHA256SUMS..."
    if ! download_file "${base_url}/SHA256SUMS" "$work/SHA256SUMS"; then
      die "failed to download ${base_url}/SHA256SUMS (pass --no-verify to skip checksum check)"
    fi
    local expected_sum=""
    while IFS= read -r line; do
      line="${line%$'\r'}"
      if [[ "$line" =~ ^([0-9a-fA-F]{64})[[:space:]]+\*?(.+)$ ]]; then
        if [[ "${BASH_REMATCH[2]}" == "$archive_name" ]]; then
          expected_sum="$(printf '%s' "${BASH_REMATCH[1]}" | tr '[:upper:]' '[:lower:]')"
          break
        fi
      fi
    done <"$work/SHA256SUMS"
    [[ -n "$expected_sum" ]] || die "SHA256SUMS does not contain an entry for $archive_name"

    local actual_sum=""
    if command -v sha256sum >/dev/null 2>&1; then
      actual_sum="$(sha256sum "$work/$archive_name" | awk '{print tolower($1)}')"
    elif command -v shasum >/dev/null 2>&1; then
      actual_sum="$(shasum -a 256 "$work/$archive_name" | awk '{print tolower($1)}')"
    elif command -v openssl >/dev/null 2>&1; then
      actual_sum="$(openssl dgst -sha256 "$work/$archive_name" | awk '{print tolower($NF)}')"
    else
      die "sha256sum, shasum, or openssl is required to verify checksums"
    fi

    if [[ "$actual_sum" != "$expected_sum" ]]; then
      die "SHA-256 mismatch for $archive_name (expected $expected_sum, got $actual_sum)"
    fi
  fi

  tar -xzf "$work/$archive_name" -C "$work"
  local extracted="$work/$stem"
  if [[ ! -f "$extracted/ice-box-cli" || ! -d "$extracted/resources" ]]; then
    die "archive $archive_name does not contain expected '$stem/ice-box-cli' and '$stem/resources/'"
  fi
  chmod +x "$extracted/ice-box-cli"
  [[ -f "$extracted/resources/sing-box" ]] && chmod +x "$extracted/resources/sing-box"
  [[ -f "$extracted/resources/ice-helper" ]] && chmod +x "$extracted/resources/ice-helper"

  mkdir -p "$install_dir" "$bin_dir"
  local staging
  staging="$(mktemp -d "$install_dir/.staging-XXXXXX")"
  cp -R "$extracted"/. "$staging/"

  if [[ "$is_macos" == "1" && "$(uname -s)" == "Darwin" ]] && command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "$staging" 2>/dev/null || true
  fi

  # Prove the new tree runs before replacing the previous install.
  if [[ "$skip_smoke" != "1" ]]; then
    if ! "$staging/ice-box-cli" --version; then
      rm -rf "$staging"
      die "staged binary failed smoke check"
    fi
  fi

  local current_dir="$install_dir/current"
  local backup_dir="$install_dir/.previous-$$"
  rm -rf "$backup_dir"
  if [[ -e "$current_dir" ]]; then
    mv "$current_dir" "$backup_dir"
  fi
  if ! mv "$staging" "$current_dir"; then
    rm -rf "$staging"
    if [[ -e "$backup_dir" ]]; then
      mv "$backup_dir" "$current_dir" || true
    fi
    die "failed to activate new installation at $current_dir"
  fi
  if ! ln -sfn "$current_dir/ice-box-cli" "$bin_dir/ice-box-cli"; then
    rm -rf "$current_dir"
    if [[ -e "$backup_dir" ]]; then
      mv "$backup_dir" "$current_dir" || true
    fi
    die "failed to symlink $bin_dir/ice-box-cli"
  fi
  rm -rf "$backup_dir"

  echo "Installed ice-box-cli (${tag}) to $current_dir"
  echo "Symlinked binary at $bin_dir/ice-box-cli"

  case ":${PATH:-}:" in
  *":${bin_dir}:"*) ;;
  *)
    echo ""
    echo "Add $bin_dir to your PATH to run \`ice-box-cli\` from any shell:"
    case "${SHELL:-}" in
    */zsh)
      echo "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
      ;;
    */fish)
      echo "  fish_add_path \"$bin_dir\""
      ;;
    *)
      echo "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
      ;;
    esac
    ;;
  esac
}

main "$@"
