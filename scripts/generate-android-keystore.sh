#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Create the project release keystore outside the repository.
# Usage: scripts/generate-android-keystore.sh /absolute/path/ice-box-release.jks
#
# Required environment:
#   ICE_BOX_ANDROID_KEYSTORE_PASSWORD
#   ICE_BOX_ANDROID_KEY_PASSWORD
# Optional:
#   ICE_BOX_ANDROID_KEY_ALIAS (default: ice-box)
#
# Back the file up offline, then store it in GitHub Actions secrets.
# Losing it means installed clients cannot upgrade in place.
# Never commit the keystore or its passwords.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-}"
if [[ -z "$DEST" ]]; then
  echo "usage: $0 /absolute/path/ice-box-release.p12" >&2
  exit 1
fi
: "${ICE_BOX_ANDROID_KEYSTORE_PASSWORD:?set ICE_BOX_ANDROID_KEYSTORE_PASSWORD}"
: "${ICE_BOX_ANDROID_KEY_PASSWORD:?set ICE_BOX_ANDROID_KEY_PASSWORD}"
ALIAS="${ICE_BOX_ANDROID_KEY_ALIAS:-ice-box}"

dest="$(realpath -m "$DEST")"
root="$(realpath "$ROOT")"
case "$dest" in
"$root" | "$root"/*)
  echo "refusing to write the keystore inside the repository: $dest" >&2
  exit 1
  ;;
esac
if [[ -e "$dest" ]]; then
  echo "keystore already exists: $dest" >&2
  exit 1
fi
mkdir -p "$(dirname "$dest")"

# JKS keeps the store password and the key password independent. PKCS12
# ignores -keypass, and Gradle then fails to unlock the key.
keytool -genkeypair -v \
  -keystore "$dest" \
  -storetype JKS \
  -alias "$ALIAS" \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -storepass "$ICE_BOX_ANDROID_KEYSTORE_PASSWORD" \
  -keypass "$ICE_BOX_ANDROID_KEY_PASSWORD" \
  -dname "CN=ice-box, OU=ice-box, O=ice-box, L=Unknown, ST=Unknown, C=US"
chmod 600 "$dest"

echo "wrote $dest"
echo "alias: $ALIAS"
keytool -list -keystore "$dest" -alias "$ALIAS" \
  -storepass "$ICE_BOX_ANDROID_KEYSTORE_PASSWORD"

cat <<EOF

Back this file up offline. Then set the GitHub Actions secrets
(the commands read the secret from stdin; this script does not print the passwords):

  base64 -w0 '$dest' | gh secret set ANDROID_KEYSTORE_BASE64
  gh secret set ANDROID_KEYSTORE_PASSWORD
  printf '%s' '$ALIAS' | gh secret set ANDROID_KEY_ALIAS
  gh secret set ANDROID_KEY_PASSWORD

A local release build reads the keystore from ICE_BOX_ANDROID_KEYSTORE instead of
the base64 secret. See docs/release-process.md.
EOF
