; Installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).

; Uninstalling takes "Open in Filezy" (Settings > Windows integration, see
; src-tauri/src/shellreg.rs) out of Explorer's menus, so no entry is left
; pointing at a removed program. An update keeps it, as the template keeps
; its own shortcuts.
;
; "Delete the application data" removes %APPDATA%\Filezy as well (pins,
; favorites and window placement, see pins.rs and session.rs). The template
; removes only the folders named after the bundle id.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\Classes\Directory\shell\Filezy"
    DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\Filezy"
    DeleteRegKey HKCU "Software\Classes\Drive\shell\Filezy"
    ${If} $DeleteAppDataCheckboxState = 1
      SetShellVarContext current
      RmDir /r "$APPDATA\Filezy"
    ${EndIf}
  ${EndIf}
!macroend
