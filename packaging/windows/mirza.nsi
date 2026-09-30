; Mirza installer for Windows. Installs for the current user only, so it
; needs no administrator rights.
;   makensis /DVERSION=0.1.0 /DARCH=x64 /DSRC=..\..\target\release packaging\windows\mirza.nsi

!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SRC
  !define SRC "..\..\target\release"
!endif
!ifndef ARCH
  !define ARCH "x64"
!endif

Name "Mirza"
OutFile "..\..\dist\Mirza-${VERSION}-windows-${ARCH}-setup.exe"
Unicode true
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\Mirza"
InstallDirRegKey HKCU "Software\Mirza" "InstallDir"
SetCompressor /SOLID lzma

!define MUI_ICON "..\..\assets\icons\mirza.ico"
!define MUI_UNICON "..\..\assets\icons\mirza.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\mirza.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start Mirza"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

!define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\Mirza"

Section "Mirza"
  ; Close a running copy so its files can be replaced.
  nsExec::Exec 'taskkill /IM mirza.exe /F'
  nsExec::Exec 'taskkill /IM mirza-panel.exe /F'
  SetOutPath "$INSTDIR"
  File "${SRC}\mirza.exe"
  File "${SRC}\mirza-panel.exe"
  File "..\..\assets\icons\mirza.ico"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Mirza.lnk" "$INSTDIR\mirza.exe" "" "$INSTDIR\mirza.ico"
  WriteRegStr HKCU "Software\Mirza" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Mirza"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "Mirza"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\mirza.ico"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM mirza.exe /F'
  nsExec::Exec 'taskkill /IM mirza-panel.exe /F'
  Delete "$INSTDIR\mirza.exe"
  Delete "$INSTDIR\mirza-panel.exe"
  Delete "$INSTDIR\mirza.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Mirza.lnk"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Mirza"
  DeleteRegKey HKCU "${UNINST}"
  DeleteRegKey HKCU "Software\Mirza"
  ; Settings and API keys in %APPDATA%\mirza are kept.
SectionEnd
