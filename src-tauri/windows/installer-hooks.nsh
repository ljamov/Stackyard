; Installer hooks for the Windows setup program (NSIS).
;
; The setup program remembers the folder Stackyard was installed to under
; HKCU\Software\<publisher>\Stackyard, and an update installs to that folder.
; Up to 0.17.0 no publisher was set, so the folder was saved under
; "Software\stackyard\Stackyard". From 0.18.0 the publisher is
; "Kristijan Ljamov", so updates looked under a new key, found nothing, and
; installed a second copy in %LOCALAPPDATA%\Stackyard. The Start menu and
; desktop shortcuts still opened the old copy in the folder the person chose,
; so after a restart Stackyard was back on the old version.
;
; These hooks make an update go back to the folder saved under the old key,
; remove the second copy an earlier update left behind, and then drop the old
; key so this happens once.

!define SY_OLD_KEY "Software\stackyard\${PRODUCTNAME}"

; The folder this update installed to before it was moved back to the
; person's own folder; empty when nothing moved.
Var SyMovedFrom

!macro NSIS_HOOK_PREINSTALL
  StrCpy $SyMovedFrom ""
  ${If} $UpdateMode = 1
    ReadRegStr $R9 SHCTX "${SY_OLD_KEY}" ""
    ${If} $R9 != ""
    ${AndIf} $R9 != $INSTDIR
    ${AndIf} ${FileExists} "$R9\${MAINBINARYNAME}.exe"
      DetailPrint "Updating Stackyard in its own folder: $R9"
      StrCpy $SyMovedFrom $INSTDIR
      StrCpy $INSTDIR $R9
      SetOutPath $INSTDIR
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The copy an earlier update put in the wrong folder: only Stackyard's own
  ; files are removed, and the folder only if nothing else is left in it.
  ${If} $SyMovedFrom != ""
  ${AndIf} ${FileExists} "$SyMovedFrom\uninstall.exe"
  ${AndIf} ${FileExists} "$SyMovedFrom\${MAINBINARYNAME}.exe"
    DetailPrint "Removing the second copy in $SyMovedFrom"
    Delete "$SyMovedFrom\${MAINBINARYNAME}.exe"
    Delete "$SyMovedFrom\stackyard.exe"
    Delete "$SyMovedFrom\stackyard.log"
    Delete "$SyMovedFrom\uninstall.exe"
    RMDir "$SyMovedFrom"
  ${EndIf}
  ; The folder is now saved under the current key.
  DeleteRegKey SHCTX "${SY_OLD_KEY}"
  DeleteRegKey /ifempty SHCTX "Software\stackyard"
!macroend
