#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that every Rust toolchain pin under .github/ matches the channel in
# rust-toolchain.toml, so CI, the release builds, and local builds use one
# Rust version. Bump rust-toolchain.toml and the workflow pins together.
#
# Usage: scripts/check-toolchain-pins.sh [repo-root]
#
# Checked forms, quoted or not:
#   - `uses: dtolnay/rust-toolchain@<ref>`: a version ref (@1.94.1) must match;
#     a channel ref (@stable, @master, ...) must set a matching `toolchain:`
#     input, otherwise the step floats to whatever that channel is today
#   - any other `toolchain: <version>` input
#   - `rustup toolchain install|default|override set <version>` in a script
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"

python3 - "$ROOT" <<'PY'
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
toolchain_file = root / "rust-toolchain.toml"
match = re.search(r'^channel = "([^"]*)"$', toolchain_file.read_text(), re.MULTILINE)
if not match:
    sys.exit("check-toolchain-pins: no channel in rust-toolchain.toml")
channel = match.group(1)

USES = re.compile(r"^(\s*)(?:-\s+)?uses:\s*[\"']?dtolnay/rust-toolchain@([^\s\"'#]+)")
TOOLCHAIN = re.compile(r"^\s*(?:-\s+)?toolchain:\s*[\"']?([^\s\"'#]+)")
RUSTUP = re.compile(r"rustup\s+(?:toolchain\s+install|default|override\s+set)\s+[\"']?([^\s\"';|&]+)")
VERSION = re.compile(r"^\d+\.\d+(?:\.\d+)?$")

errors = []
pins = 0


def check(where: str, line: str, pin: str) -> None:
    global pins
    pins += 1
    if pin != channel:
        errors.append(f"{where} pins {pin}; rust-toolchain.toml has {channel}\n  {line.strip()}")


def indent(line: str) -> int:
    return len(line) - len(line.lstrip())


files = sorted(
    path
    for pattern in ("*.yml", "*.yaml")
    for path in (root / ".github").rglob(pattern)
)
for path in files:
    lines = path.read_text(encoding="utf-8").splitlines()
    relative = path.relative_to(root)
    claimed = set()  # toolchain: lines already checked as part of a uses step
    for number, line in enumerate(lines):
        where = f"{relative}:{number + 1}"
        uses = USES.match(line)
        if uses:
            ref = uses.group(2)
            # The step's other keys are indented deeper than its "- ".
            step_indent = indent(line)
            toolchain = None
            for offset in range(number + 1, len(lines)):
                follower = lines[offset]
                if follower.strip() and indent(follower) <= step_indent:
                    break
                found = TOOLCHAIN.match(follower)
                if found:
                    toolchain = (offset, found.group(1))
                    claimed.add(offset)
                    break
            if toolchain is not None:
                offset, pin = toolchain
                check(f"{relative}:{offset + 1}", lines[offset], pin)
            if VERSION.match(ref):
                check(where, line, ref)
            elif toolchain is None:
                errors.append(
                    f"{where} uses dtolnay/rust-toolchain@{ref} without a toolchain "
                    f"input; pin it to {channel}\n  {line.strip()}"
                )
            continue
        if number not in claimed:
            found = TOOLCHAIN.match(line)
            if found:
                check(where, line, found.group(1))
        for found in RUSTUP.finditer(line):
            check(where, line, found.group(1))

if pins == 0 and not errors:
    sys.exit("check-toolchain-pins: no toolchain pins found under .github/")
if errors:
    for error in errors:
        print(f"check-toolchain-pins: {error}", file=sys.stderr)
    sys.exit(1)
print(f"check-toolchain-pins: OK ({channel}, {pins} pins)")
PY
