# AGENTS.md

## Compliance

The rules in this document are mandatory. Strictly follow every rule constraint. If an action would violate, or could potentially violate, any rule below, stop and ask the user first before proceeding.

## Rules

1. **Code comments**: All code comments must be written in English.
2. **Documentation**: All documentation must be written in English by default, unless it is specifically intended as Chinese-language documentation.
3. **Git operations**: Do not commit or push unless the user explicitly asks you to.
4. **Pre-commit gate**: Run `scripts/gate-local.sh` before every commit and only commit after it passes. It scopes checks to uncommitted changes vs `HEAD` (or the full branch vs `main` when clean or with `--branch`); do not narrow it further by hand. Run `scripts/gate-local.sh --full` before a formal release.
5. **Documentation screenshots**: The gates do not refresh screenshots. Run `scripts/capture-docs.sh` when the Live Demo UI changes and before a formal release; commit only `docs/images/hero.png` and `docs/images/hero.zh-CN.png`.
6. **Release process**: Formal (non-pre-release) version releases must follow the process in `docs/release-process.md` (bump via `scripts/bump-version.sh`, update `CHANGELOG.md`, gate + merge to `main`, tag `vX.Y.Z` and push). Do not cut a formal release any other way.
7. **System tray menu parity**: The macOS native tray menu (`apps/desktop/src-tauri/src/tray.rs`, `apps/desktop/src-tauri/src/tray_delay_view.rs`) and the Windows custom tray panel (`apps/desktop/src-tauri/src/tray_panel.rs`, `apps/desktop/src/tray-panel/TrayPanel.tsx`, `apps/desktop/src/tray-panel/tray-panel.css`) must stay visually and structurally in sync (item order, labels, separators, icons/colours, delay badges, submenu hierarchy, and interaction style). Any change to one platform's tray menu style or structure must be mirrored to the other platform in the same change.
