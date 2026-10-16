; Installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).

; Uninstalling takes "Open in Filezy" (Settings > Windows integration, see
; src-tauri/src/shellreg.rs) out of Explorer's menus, so no entry is left
; pointing at a removed program. An update keeps it, as the template keeps
; its own shortcuts.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\Classes\Directory\shell\Filezy"
    DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\Filezy"
    DeleteRegKey HKCU "Software\Classes\Drive\shell\Filezy"
  ${EndIf}
!macroend
