#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Export NDK clang wrappers for cargo check of the Android targets.
# Usage: source scripts/android-rust-linkers.sh
#
# ANDROID_HOME must already point at an SDK that contains NDK 28
# (scripts/ci-install-android-sdk.sh installs that NDK on CI).
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  echo "source this script; do not execute it" >&2
  exit 2
fi

if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
  echo "ANDROID_HOME is not set" >&2
  return 1
fi

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  if [[ -d "$ANDROID_HOME/ndk/28.0.13004108" ]]; then
    ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.0.13004108"
  else
    echo "NDK 28.0.13004108 not found under $ANDROID_HOME/ndk" >&2
    return 1
  fi
fi
export ANDROID_NDK_HOME

prebuilt="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt"
host_tag=""
for candidate in linux-x86_64 darwin-arm64 darwin-x86_64; do
  if [[ -d "$prebuilt/$candidate" ]]; then
    host_tag="$candidate"
    break
  fi
done
if [[ -z "$host_tag" ]]; then
  echo "no NDK prebuilt toolchain under $prebuilt" >&2
  return 1
fi

bin="$prebuilt/$host_tag/bin"
# minSdk is 34. The NDK ships a clang wrapper per API level.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android34-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$bin/x86_64-linux-android34-clang"
export CC_aarch64_linux_android="$bin/aarch64-linux-android34-clang"
export CC_x86_64_linux_android="$bin/x86_64-linux-android34-clang"
export CXX_aarch64_linux_android="$bin/aarch64-linux-android34-clang++"
export CXX_x86_64_linux_android="$bin/x86_64-linux-android34-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar"
export AR_x86_64_linux_android="$bin/llvm-ar"

for tool in \
  "$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER" \
  "$CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER" \
  "$AR_aarch64_linux_android"; do
  if [[ ! -x "$tool" ]]; then
    echo "NDK tool not executable: $tool" >&2
    return 1
  fi
done
