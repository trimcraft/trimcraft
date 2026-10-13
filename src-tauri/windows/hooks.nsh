; Trimcraft's additions to the Tauri NSIS installer (bundle.windows.nsis.installerHooks).

; Uninstall removes "Start with Windows" (Settings > General): the per-user Run value and the
; elevated Task Scheduler task. Names as in src-tauri/src/autostart.rs. Not on updates.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Trimcraft"
    ; Removing the task needs administrator rights: ask (UAC) only if it exists.
    ReadEnvStr $R0 USERNAME
    nsExec::Exec 'schtasks.exe /Query /TN "Trimcraft autostart $R0"'
    Pop $R1
    ${If} $R1 = 0
      ExecShellWait "runas" "schtasks.exe" '/Delete /TN "Trimcraft autostart $R0" /F' SW_HIDE
    ${EndIf}
  ${EndIf}
!macroend
