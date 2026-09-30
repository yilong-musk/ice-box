#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install the Android SDK packages the mobile build needs.
# ANDROID_HOME must already point at an SDK (android-actions/setup-android on CI).
set -euo pipefail

if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
  echo "ANDROID_HOME is not set" >&2
  exit 1
fi
if ! command -v sdkmanager >/dev/null 2>&1; then
  echo "sdkmanager is not on PATH" >&2
  exit 1
fi

# License prompts are not a failure. A later install still errors if a package
# cannot be accepted.
yes | sdkmanager --licenses >/dev/null || true
sdkmanager --install \
  "platforms;android-36" \
  "build-tools;35.0.0" \
  "ndk;28.0.13004108"
