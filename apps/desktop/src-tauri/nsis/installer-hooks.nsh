; Plan B: one-time Windows TUN elevation (scheduled task).
;
; customInstall: create the highest-privilege scheduled task that runs the
; TUN core elevated (ice-tun-launcher -> sing-box). Best-effort: a per-user
; NSIS installer is not elevated, so the creation may fail here — the app's
; runtime one-shot setup (ensure_tun_elevation, a single UAC) covers it.
; customUnInstall: delete the task (best-effort; the uninstaller may not be
; elevated, a leftover task is inert — it never auto-triggers).

!macro customInstall
  nsExec::ExecToLog 'schtasks /Create /TN ice-box-tun /TR "\"$INSTDIR\ice-tun-launcher.exe\" --data \"$APPDATA\com.yilong-musk.icebox\"" /SC ONCE /ST 23:59 /RL HIGHEST /F'
!macroend

!macro customUnInstall
  nsExec::ExecToLog 'schtasks /Delete /TN ice-box-tun /F'
!macroend