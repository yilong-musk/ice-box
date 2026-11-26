#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify that Linux ELF binaries do not require a non-weak GLIBC symbol version
# above the configured floor (e.g. 2.35 for Ubuntu 22.04).
#
# Usage:
#   scripts/check-glibc-floor.sh <max_version> <elf_file...>
#   scripts/check-glibc-floor.sh --parse-stdin <max_version> [label]
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage:
  scripts/check-glibc-floor.sh <max_version> <elf_file...>
  scripts/check-glibc-floor.sh --parse-stdin <max_version> [label]
EOF
}

if (($# == 0)); then
  usage
  exit 2
fi

case "$1" in
-h | --help)
  usage
  exit 0
  ;;
esac

PY_SCRIPT="$(cat <<'PY'
import os
import re
import shutil
import subprocess
import sys

VERSION_RE = re.compile(r"^\d+(?:\.\d+)+$")
READELF_NEED_RE = re.compile(
    r"Name:\s+GLIBC_([0-9]+(?:\.[0-9]+)*)\s+Flags:\s+([^\n]*?)(?:\s+Version:|\s*$)"
)
OBJDUMP_NEED_RE = re.compile(
    r"^\s+0x[0-9a-fA-F]+\s+0x([0-9a-fA-F]+)\s+[0-9]+\s+GLIBC_([0-9]+(?:\.[0-9]+)*)\s*$"
)
WEAK_FLAG_RE = re.compile(r"\bWEAK\b")
VER_FLG_WEAK = 0x2


def usage_error(message: str) -> None:
    print(f"check-glibc-floor: {message}", file=sys.stderr)
    sys.exit(2)


def parse_version(raw: str) -> list[int]:
    return [int(part) for part in raw.split(".")]


def version_key(raw: str) -> tuple[int, ...]:
    parts = parse_version(raw)
    while len(parts) > 2 and parts[-1] == 0:
        parts.pop()
    return tuple(parts)


def version_gt(left: str, right: str) -> bool:
    a = parse_version(left)
    b = parse_version(right)
    length = max(len(a), len(b))
    a.extend([0] * (length - len(a)))
    b.extend([0] * (length - len(b)))
    return a > b


def parse_readelf_v(text: str) -> list[str]:
    """Return all non-weak GLIBC_<ver> versions from `readelf -V` output."""
    seen_section_header = False
    in_version_needs = False
    versions: list[str] = []

    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("Version needs section"):
            seen_section_header = True
            in_version_needs = True
            continue
        if stripped.startswith("Version symbols section") or stripped.startswith(
            "Version definition section"
        ):
            seen_section_header = True
            in_version_needs = False
            continue

        if seen_section_header and not in_version_needs:
            continue

        match = READELF_NEED_RE.search(line)
        if not match:
            continue
        ver, flags = match.group(1), match.group(2)
        if WEAK_FLAG_RE.search(flags):
            continue
        versions.append(ver)

    return versions


def parse_objdump_p(text: str) -> list[str]:
    """Return all non-weak GLIBC_<ver> versions from `objdump -p` output."""
    in_refs = False
    versions: list[str] = []

    for line in text.splitlines():
        if line.startswith("Version References:"):
            in_refs = True
            continue
        if in_refs:
            if not line.strip() or not line.startswith(" "):
                in_refs = False
                continue
            match = OBJDUMP_NEED_RE.match(line)
            if not match:
                continue
            flags_hex, ver = match.group(1), match.group(2)
            if (int(flags_hex, 16) & VER_FLG_WEAK) != 0:
                continue
            versions.append(ver)

    return versions


def check_versions(label: str, max_version: str, versions: list[str]) -> str | None:
    offending = sorted(
        {ver for ver in versions if version_gt(ver, max_version)},
        key=version_key,
    )
    if offending:
        rendered = ", ".join(f"GLIBC_{ver}" for ver in offending)
        return (
            f"check-glibc-floor: FAIL {label}: requires {rendered} "
            f"(max allowed GLIBC_{max_version})"
        )
    if versions:
        highest = max(set(versions), key=version_key)
        print(
            f"check-glibc-floor: OK {label} "
            f"(max GLIBC_{highest} <= GLIBC_{max_version})"
        )
    else:
        print(
            f"check-glibc-floor: OK {label} "
            "(static / no non-weak GLIBC requirements)"
        )
    return None


def inspect_elf(path: str) -> tuple[list[str] | None, str | None]:
    if not os.path.isfile(path):
        return None, f"check-glibc-floor: FAIL {path}: file not found"
    try:
        with open(path, "rb") as handle:
            magic = handle.read(4)
    except OSError as error:
        return None, f"check-glibc-floor: FAIL {path}: could not read file ({error})"
    if magic != b"\x7fELF":
        return None, f"check-glibc-floor: FAIL {path}: not an ELF binary"

    env = dict(os.environ, LC_ALL="C")
    readelf = shutil.which("readelf")
    if readelf:
        proc = subprocess.run(
            [readelf, "-V", path],
            capture_output=True,
            text=True,
            env=env,
        )
        if proc.returncode != 0:
            detail = (proc.stderr or proc.stdout).strip()
            return (
                None,
                f"check-glibc-floor: FAIL {path}: readelf -V failed ({detail})",
            )
        return parse_readelf_v(proc.stdout), None

    objdump = shutil.which("objdump")
    if objdump:
        proc = subprocess.run(
            [objdump, "-p", path],
            capture_output=True,
            text=True,
            env=env,
        )
        if proc.returncode != 0:
            detail = (proc.stderr or proc.stdout).strip()
            return (
                None,
                f"check-glibc-floor: FAIL {path}: objdump -p failed ({detail})",
            )
        return parse_objdump_p(proc.stdout), None

    return (
        None,
        "check-glibc-floor: neither readelf nor objdump is installed on PATH",
    )


args = sys.argv[1:]
if args[0] == "--parse-stdin":
    if len(args) < 2 or len(args) > 3:
        usage_error("expected: --parse-stdin <max_version> [label]")
    max_version = args[1]
    label = args[2] if len(args) == 3 else "<stdin>"
    if not VERSION_RE.match(max_version):
        usage_error(f"invalid max version '{max_version}' (expected e.g. 2.35)")
    versions = parse_readelf_v(sys.stdin.read())
    failure = check_versions(label, max_version, versions)
    if failure:
        print(failure, file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if len(args) < 2:
    usage_error("expected: <max_version> <elf_file...>")

max_version = args[0]
files = args[1:]
if not VERSION_RE.match(max_version):
    usage_error(f"invalid max version '{max_version}' (expected e.g. 2.35)")

failures: list[str] = []
for path in files:
    versions, error = inspect_elf(path)
    if error is not None:
        failures.append(error)
        continue
    assert versions is not None
    failure = check_versions(path, max_version, versions)
    if failure is not None:
        failures.append(failure)

if failures:
    for line in failures:
        print(line, file=sys.stderr)
    sys.exit(1)
PY
)"

python3 -c "$PY_SCRIPT" "$@"
