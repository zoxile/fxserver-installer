; Compile only. Never run this fixture: hook runtime needs a disposable VM.
Unicode true
RequestExecutionLevel user
Name "fxins hook compile fixture - do not run"
OutFile "${FXINS_TEST_OUT}"
InstallDir "$TEMP\fxins-hook-fixture-do-not-run"
!include "LogicLib.nsh"
!include "WinMessages.nsh"
!include "${__FILEDIR__}\..\..\src-tauri\packaging\windows\hooks.nsh"

Section
  !insertmacro NSIS_HOOK_POSTINSTALL
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  !insertmacro NSIS_HOOK_PREUNINSTALL
SectionEnd
