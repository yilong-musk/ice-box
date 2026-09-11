; SPDX-License-Identifier: GPL-3.0-or-later

; Plan B: Windows TUN elevation (scheduled task).
;
; customInstall: do not create ice-box-tun here. A per-user NSIS installer is
; not elevated, and schtasks /Create cannot store the SHA-256 pin (that pin
; lives in RegistrationInfo/Description via UTF-16 XML import). A pin-less
; leftover task made every TUN enable look stale and re-prompt UAC. The
; app's one-shot ensure_tun_elevation (a single UAC) creates the pinned task.
; customUnInstall: delete the task (best-effort; the uninstaller may not be
; elevated, a leftover task is inert — it never auto-triggers).

!macro customInstall
  ; The pinned ice-box-tun task is created at first TUN enable (one UAC).
!macroend

!macro customUnInstall
  ; Best-effort: delete the task from an unelevated uninstaller.
  nsExec::ExecToLog 'schtasks /Delete /TN ice-box-tun /F'
  ; Protected Program Files copies and ProgramData run dir are admin-owned.
  ; Request elevation so the leftover tree is removed (the user can cancel;
  ; leftovers are inert once the task is gone). Fall back to the pre-ProgramFiles
  ; launcher path so an upgrade-then-uninstall still wipes %ProgramData%\ice-box.
  IfFileExists "$PROGRAMFILES\ice-box\ice-tun-launcher.exe" 0 try_legacy_protected_cleanup
    ExecShellWait "runas" "$PROGRAMFILES\ice-box\ice-tun-launcher.exe" "--delete-task" SW_HIDE
    Goto skip_protected_cleanup
  try_legacy_protected_cleanup:
  ReadEnvStr $0 PROGRAMDATA
  IfFileExists "$0\ice-box\bin\ice-tun-launcher.exe" 0 skip_protected_cleanup
    ExecShellWait "runas" "$0\ice-box\bin\ice-tun-launcher.exe" "--delete-task" SW_HIDE
  skip_protected_cleanup:
!macroend
