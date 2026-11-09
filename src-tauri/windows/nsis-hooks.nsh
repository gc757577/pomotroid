; Current-user Preview entry only. Never enable a new startup registration.
!macro NSIS_HOOK_POSTINSTALL
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
