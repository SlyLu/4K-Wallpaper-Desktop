; The GNU Rust target links WebView2Loader dynamically, so it must sit beside the executable.
!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\resources\WebView2Loader.dll"
    CopyFiles /SILENT "$INSTDIR\resources\WebView2Loader.dll" "$INSTDIR"
  ${EndIf}

  ; Fail installation instead of leaving an application that cannot start.
  ${IfNot} ${FileExists} "$INSTDIR\WebView2Loader.dll"
    Abort "WebView2Loader.dll could not be installed."
  ${EndIf}
!macroend

; The copied loader is outside Tauri's tracked resource directory.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\WebView2Loader.dll"
!macroend
