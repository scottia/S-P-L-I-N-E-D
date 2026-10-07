!macro NSIS_HOOK_PREINSTALL
  ReadEnvStr $0 "SPLINED_PORTABLE_ROOT"
  ${If} $0 == ""
    MessageBox MB_ICONSTOP|MB_OK "This signed update package must be launched from SPLINED after the user approves an update."
    Abort
  ${EndIf}
  StrCpy $INSTDIR $0
  SetOutPath $INSTDIR
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Delete "$INSTDIR\uninstall.exe"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  RMDir "$SMPROGRAMS\$AppStartMenuFolder"
  DeleteRegKey SHCTX "${UNINSTKEY}"
  DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
  DeleteRegKey /ifempty SHCTX "${MANUKEY}"
!macroend
