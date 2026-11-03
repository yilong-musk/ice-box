# AGENTS.md

## Compliance

The rules in this document are mandatory. Strictly follow every rule constraint. If an action would violate, or could potentially violate, any rule below, stop and ask the user first before proceeding.

## Rules

1. **Code comments**: All code comments must be written in English.
2. **Documentation**: All documentation must be written in English by default, unless it is specifically intended as Chinese-language documentation.
3. **Git operations**: Do not commit or push unless the user explicitly asks you to.
4. **Pre-commit gate**: Run `scripts/gate-local.sh` before every commit and only commit after it passes. It selects checks from the files changed on the branch; do not narrow it further by hand. Run `scripts/gate-local.sh --full` before a formal release.
5. **Documentation screenshots**: The gates do not refresh screenshots. Run `scripts/capture-docs.sh` when the Live Demo UI changes and before a formal release; commit only `docs/images/hero.png` and `docs/images/hero.zh-CN.png`.
6. **Release process**: Formal (non-pre-release) version releases must follow the process in `docs/release-process.md` (bump via `scripts/bump-version.sh`, update `CHANGELOG.md`, gate + merge to `main`, tag `vX.Y.Z` and push). Do not cut a formal release any other way.