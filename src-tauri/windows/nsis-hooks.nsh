; Current-user stable identity only. Never enable a new startup registration.
!macro ClearPomoRenameShortcut OLD NEW
  !insertmacro IsShortcutTarget "${OLD}" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $R8
  ${If} $R8 = 1
    ${If} ${FileExists} "${NEW}"
      !insertmacro IsShortcutTarget "${NEW}" "$INSTDIR\${MAINBINARYNAME}.exe"
      Pop $R8
    ${EndIf}
    ${If} $R8 = 1
      ClearErrors
      CreateShortcut "${NEW}" "$INSTDIR\${MAINBINARYNAME}.exe"
      ${IfNot} ${Errors}
        !insertmacro SetLnkAppUserModelId "${NEW}"
        Delete "${OLD}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Rename only shortcuts whose target is this app's stable executable.
  !insertmacro ClearPomoRenameShortcut "$DESKTOP\Pomotroid Fork Preview.lnk" "$DESKTOP\ClearPomo.lnk"
  ${If} $AppStartMenuFolder == ""
    StrCpy $AppStartMenuFolder "ClearPomo"
  ${EndIf}
  !insertmacro IsShortcutTarget "$SMPROGRAMS\Pomotroid Fork Preview\Pomotroid Fork Preview.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $R8
  ${If} $R8 = 1
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    !insertmacro ClearPomoRenameShortcut "$SMPROGRAMS\Pomotroid Fork Preview\Pomotroid Fork Preview.lnk" "$SMPROGRAMS\$AppStartMenuFolder\ClearPomo.lnk"
    RMDir "$SMPROGRAMS\Pomotroid Fork Preview"
  ${EndIf}
  !insertmacro IsShortcutTarget "$SMPROGRAMS\Pomotroid Fork Preview.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $R8
  ${If} $R8 = 1
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    !insertmacro ClearPomoRenameShortcut "$SMPROGRAMS\Pomotroid Fork Preview.lnk" "$SMPROGRAMS\$AppStartMenuFolder\ClearPomo.lnk"
  ${EndIf}
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${BUNDLEID}"
  ${If} $R8 != ""
    ; Refresh the executable path after an in-place upgrade. Keep Windows'
    ; StartupApproved bytes intact, including a user-disabled state.
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${BUNDLEID}" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --from-autostart'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${BUNDLEID}"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${BUNDLEID}"
  ${EndIf}
!macroend
