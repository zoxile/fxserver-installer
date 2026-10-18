!include "LogicLib.nsh"
!include "WinMessages.nsh"
!define FXINS_HOOK_DIR "${__FILEDIR__}"

!macro FXINS_UPDATE_PATH ACTION
  Push $0
  Push $1
  InitPluginsDir
  File /oname=$PLUGINSDIR\fxins-path.ps1 "${FXINS_HOOK_DIR}\path.ps1"
  ; Keep PATH out of NSIS strings (which have a fixed maximum length).
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\fxins-path.ps1" -Action ${ACTION} -InstallDir "$INSTDIR\."'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "fxins PATH ${ACTION} failed: $1"
    MessageBox MB_OK|MB_ICONEXCLAMATION "Could not update your user PATH for fxins. Existing PATH entries were preserved. You can use the full path to fxins.exe; see the terminal installation guide." /SD IDOK
  ${Else}
    ; Notify Explorer; already-open terminals still need to be restarted.
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
  Delete "$PLUGINSDIR\fxins-path.ps1"
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro FXINS_UPDATE_PATH Install
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro FXINS_UPDATE_PATH Uninstall
!macroend
