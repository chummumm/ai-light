; User preferences are retained; remove only our own login entry.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AILightRust"
!macroend
