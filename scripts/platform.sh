#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Shared platform aliases for ice-box scripts.
# Source and call ice_resolve_platform <alias>; exports ICE_* variables below.
#
# Aliases: host | mac-arm64 | mac-x64 | win | linux-x64 | linux-arm64
# Also accepts: darwin-aarch64 | darwin-x86_64 | windows-x86_64 | linux-x86_64 | linux-aarch64
#               aarch64-apple-darwin | x86_64-apple-darwin | x86_64-pc-windows-msvc
#               x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu

ice_host_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os/$arch" in
    Darwin/arm64) echo "mac-arm64" ;;
    Darwin/x86_64) echo "mac-x64" ;;
    Linux/x86_64) echo "linux-x64" ;;
    Linux/aarch64 | Linux/arm64) echo "linux-arm64" ;;
    MINGW*/* | MSYS*/* | CYGWIN*/*) echo "win" ;;
    *)
      if [[ "${OS:-}" == "Windows_NT" ]]; then
        echo "win"
      else
        echo ""
      fi
      ;;
  esac
}

ice_resolve_platform() {
  local raw="${1:-host}"
  local alias="$raw"

  if [[ "$alias" == "host" ]]; then
    alias="$(ice_host_platform)"
    if [[ -z "$alias" ]]; then
      echo "unsupported host: $(uname -s)/$(uname -m)" >&2
      return 1
    fi
  fi

  case "$alias" in
    mac-arm64 | darwin-aarch64 | aarch64-apple-darwin)
      ICE_PLATFORM_ALIAS="mac-arm64"
      ICE_TARGET_DIR="darwin-aarch64"
      ICE_BIN="sing-box"
      ICE_CARGO_TARGET="aarch64-apple-darwin"
      ;;
    mac-x64 | darwin-x86_64 | x86_64-apple-darwin)
      ICE_PLATFORM_ALIAS="mac-x64"
      ICE_TARGET_DIR="darwin-x86_64"
      ICE_BIN="sing-box"
      ICE_CARGO_TARGET="x86_64-apple-darwin"
      ;;
    win | windows | windows-x86_64 | x86_64-pc-windows-msvc)
      ICE_PLATFORM_ALIAS="win"
      ICE_TARGET_DIR="windows-x86_64"
      ICE_BIN="sing-box.exe"
      ICE_CARGO_TARGET="x86_64-pc-windows-msvc"
      ;;
    linux-x64 | linux-x86_64 | x86_64-unknown-linux-gnu)
      ICE_PLATFORM_ALIAS="linux-x64"
      ICE_TARGET_DIR="linux-x86_64"
      ICE_BIN="sing-box"
      ICE_CARGO_TARGET="x86_64-unknown-linux-gnu"
      ;;
    linux-arm64 | linux-aarch64 | aarch64-unknown-linux-gnu)
      ICE_PLATFORM_ALIAS="linux-arm64"
      ICE_TARGET_DIR="linux-aarch64"
      ICE_BIN="sing-box"
      ICE_CARGO_TARGET="aarch64-unknown-linux-gnu"
      ;;
    *)
      echo "unknown platform: $raw (use host | mac-arm64 | mac-x64 | win | linux-x64 | linux-arm64)" >&2
      return 1
      ;;
  esac

  ICE_PLATFORM_LABEL="${ICE_PLATFORM_ALIAS} (${ICE_TARGET_DIR})"
  export ICE_PLATFORM_ALIAS ICE_TARGET_DIR ICE_BIN ICE_CARGO_TARGET ICE_PLATFORM_LABEL
}

# Pick cargo --target only when cross-compiling on the current host.
ice_dev_cargo_target() {
  local host
  host="$(ice_host_platform)"

  case "$host" in
    mac-arm64)
      [[ "$ICE_PLATFORM_ALIAS" == "mac-x64" ]] && echo "$ICE_CARGO_TARGET"
      ;;
    mac-x64)
      [[ "$ICE_PLATFORM_ALIAS" == "mac-arm64" ]] && echo "$ICE_CARGO_TARGET"
      ;;
    linux-x64)
      [[ "$ICE_PLATFORM_ALIAS" == "linux-arm64" ]] && echo "$ICE_CARGO_TARGET"
      ;;
    linux-arm64)
      [[ "$ICE_PLATFORM_ALIAS" == "linux-x64" ]] && echo "$ICE_CARGO_TARGET"
      ;;
    win)
      [[ "$ICE_PLATFORM_ALIAS" == "win" ]] || return 0
      ;;
    *)
      echo "$ICE_CARGO_TARGET"
      ;;
  esac
}

ice_assert_dev_host() {
  local host
  host="$(ice_host_platform)"

  case "$ICE_PLATFORM_ALIAS" in
    mac-arm64 | mac-x64)
      if [[ "$(uname -s)" != "Darwin" ]]; then
        echo "dev:${ICE_PLATFORM_ALIAS} must run on macOS (current: $(uname -s))" >&2
        return 1
      fi
      ;;
    linux-x64 | linux-arm64)
      if [[ "$(uname -s)" != "Linux" ]]; then
        echo "dev:${ICE_PLATFORM_ALIAS} must run on Linux (current: $(uname -s))" >&2
        return 1
      fi
      ;;
    win)
      if [[ "$host" != "win" ]]; then
        echo "dev:win must run on Windows (current: $(uname -s))" >&2
        return 1
      fi
      ;;
  esac
}
