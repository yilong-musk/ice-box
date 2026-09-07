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
  nsExec::ExecToLog 'schtasks /Delete /TN ice-box-tun /F'
!macroend
