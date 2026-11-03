; Trimcraft's additions to the Tauri NSIS installer (bundle.windows.nsis.installerHooks).

; --- Publisher change (0.1.4) ------------------------------------------------------------------
; Up to 0.1.3 bundle.publisher was unset and Tauri derived "breu" from the identifier
; cloud.breu.trimcraft. The uninstall entry (Software\...\Uninstall\Trimcraft) depends only on the
; product name and is simply updated, but the installer keeps the install location in
; HKCU\Software\<publisher>\<product>. Without the old key's value an update would not find a
; custom install folder, and "Uninstall before installing" would run the old uninstaller without
; a folder. So the location is taken over before the first page, and the old key removed after
; the installation.
;
; This file is included before the template defines MANUPRODUCTKEY and friends, so the function
; below spells the keys out (a test in src-tauri/src/migration.rs checks publisher and product).
!define TRIMCRAFT_OLD_MANUKEY "Software\breu"
!define TRIMCRAFT_OLD_KEY "${TRIMCRAFT_OLD_MANUKEY}\Trimcraft"
!define TRIMCRAFT_KEY "Software\Trimcraft\Trimcraft"

; Writes the old key's install location under the current publisher if that has none yet, and
; installs there when the default folder was about to be used.
!macro TRIMCRAFT_ADOPT_OLD_LOCATION
  Push $R0
  Push $R1
  ReadRegStr $R0 HKCU "${TRIMCRAFT_KEY}" ""
  ReadRegStr $R1 HKCU "${TRIMCRAFT_OLD_KEY}" ""
  ${If} $R0 == ""
  ${AndIf} $R1 != ""
    WriteRegStr HKCU "${TRIMCRAFT_KEY}" "" $R1
    ${If} $INSTDIR == "$LOCALAPPDATA\Trimcraft"
      StrCpy $INSTDIR $R1
    ${EndIf}
  ${EndIf}
  Pop $R1
  Pop $R0
!macroend

; Runs after .onInit (default or restored location set), before the reinstall page reads the key.
; Not in silent mode; NSIS_HOOK_PREINSTALL covers that.
!define MUI_CUSTOMFUNCTION_GUIINIT TrimcraftGuiInit
Function TrimcraftGuiInit
  !insertmacro TRIMCRAFT_ADOPT_OLD_LOCATION
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  ; Silent installs (winget) have no GUI init; the location is still free to change here.
  !insertmacro TRIMCRAFT_ADOPT_OLD_LOCATION
  SetOutPath $INSTDIR
!macroend

!macro TRIMCRAFT_SET_AUMID shortcut
  !insertmacro IsShortcutTarget "${shortcut}" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetLnkAppUserModelId "${shortcut}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The template wrote the location under the current publisher; drop the old key.
  DeleteRegKey HKCU "${TRIMCRAFT_OLD_KEY}"
  DeleteRegKey /ifempty HKCU "${TRIMCRAFT_OLD_MANUKEY}"
  ; The shortcuts carry the identifier as AppUserModelID, which notifications need. Updates
  ; keep existing shortcuts, so give them the current one (0.1.3 and older: cloud.breu.trimcraft).
  !insertmacro TRIMCRAFT_SET_AUMID "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro TRIMCRAFT_SET_AUMID "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro TRIMCRAFT_SET_AUMID "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\${PRODUCTNAME}.lnk"
!macroend

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

; "Delete the application data" also removes the folders of 0.1.3 and older, which the app copied
; to the current ones on its first start.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\cloud.breu.trimcraft"
    RmDir /r "$LOCALAPPDATA\cloud.breu.trimcraft"
  ${EndIf}
!macroend
