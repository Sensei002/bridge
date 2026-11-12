!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.2.0"
!endif
!ifndef PUBLISH_DIR
  !define PUBLISH_DIR "target\\release"
!endif
!ifndef SERVICE_DIR
  !define SERVICE_DIR "target\\x86_64-pc-windows-msvc\\release"
!endif
!ifndef SCRIPTS_DIR
  !define SCRIPTS_DIR "scripts\\windows"
!endif
!ifndef PUBLISHER_PIN
  !define PUBLISHER_PIN "bridge-desktop-publisher.sha256"
!endif

Name "Bridge"
Caption "Bridge Setup"
OutFile "Bridge-${PRODUCT_VERSION}-Setup-x64.exe"
InstallDir "$PROGRAMFILES64\\Bridge"
InstallDirRegKey HKLM "Software\\Bridge" "InstallDir"
RequestExecutionLevel admin
Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 32
ShowInstDetails show
ShowUnInstDetails show

!define APP_UNINSTALL_KEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Bridge"
!define APP_REG_KEY "Software\\Bridge"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Bridge" SEC_MAIN
  SetShellVarContext all
  SetRegView 64
  SetOutPath "$INSTDIR"

  ; Stop an older desktop process before replacing its executable.
  nsExec::ExecToLog 'taskkill.exe /IM bridge-desktop.exe /F'
  Pop $0

  File /oname=bridge-desktop.exe "${PUBLISH_DIR}\\bridge-desktop.exe"

  SetOutPath "$INSTDIR\\DisplayServiceSetup"
  File /oname=bridge-display-service.exe "${SERVICE_DIR}\\bridge-display-service.exe"
  File /oname=bridge-desktop-publisher.sha256 "${PUBLISHER_PIN}"
  File "${SCRIPTS_DIR}\\Install-BridgeDisplayService.ps1"
  File "${SCRIPTS_DIR}\\Uninstall-BridgeDisplayService.ps1"
  File "${SCRIPTS_DIR}\\Install-BridgeVirtualDisplayDriver.ps1"
  File "${SCRIPTS_DIR}\\Uninstall-BridgeVirtualDisplayDriver.ps1"

  ; Install the pinned, already-signed upstream VDD and the authenticated
  ; LocalSystem service. Both scripts independently require administrator rights.
  DetailPrint "Installing the verified upstream Virtual Display Driver..."
  nsExec::ExecToLog '"$SYSDIR\\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\\DisplayServiceSetup\\Install-BridgeVirtualDisplayDriver.ps1"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "Virtual Display Driver installation failed (exit code $0). Bridge setup cannot enable Second Screen."
    Abort
  ${EndIf}

  DetailPrint "Installing and starting Bridge Display Service..."
  nsExec::ExecToLog '"$SYSDIR\\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\\DisplayServiceSetup\\Install-BridgeDisplayService.ps1" -ServiceExecutable "$INSTDIR\\DisplayServiceSetup\\bridge-display-service.exe"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "Bridge Display Service installation failed (exit code $0). See the setup details for more information."
    Abort
  ${EndIf}

  SetOutPath "$INSTDIR"
  CreateDirectory "$SMPROGRAMS\\Bridge"
  CreateShortcut "$SMPROGRAMS\\Bridge\\Bridge.lnk" "$INSTDIR\\bridge-desktop.exe" "" "$INSTDIR\\bridge-desktop.exe" 0
  CreateShortcut "$DESKTOP\\Bridge.lnk" "$INSTDIR\\bridge-desktop.exe" "" "$INSTDIR\\bridge-desktop.exe" 0

  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  WriteRegStr HKLM "${APP_REG_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "DisplayName" "Bridge"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "Publisher" "Bridge Project"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\\Uninstall.exe"'
  WriteRegDWORD HKLM "${APP_UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${APP_UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64

  ; Stop the app and remove only Bridge's service. Keep the signed VDD package
  ; installed so Windows never loses an active display during uninstall.
  nsExec::ExecToLog 'taskkill.exe /IM bridge-desktop.exe /F'
  Pop $0
  IfFileExists "$INSTDIR\\DisplayServiceSetup\\Uninstall-BridgeDisplayService.ps1" 0 +3
    nsExec::ExecToLog '"$SYSDIR\\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\\DisplayServiceSetup\\Uninstall-BridgeDisplayService.ps1"'
    Pop $0

  Delete "$SMPROGRAMS\\Bridge\\Bridge.lnk"
  RMDir "$SMPROGRAMS\\Bridge"
  Delete "$DESKTOP\\Bridge.lnk"
  DeleteRegKey HKLM "${APP_UNINSTALL_KEY}"
  DeleteRegKey HKLM "${APP_REG_KEY}"
  RMDir /r "$INSTDIR\\DisplayServiceSetup"
  Delete "$INSTDIR\\bridge-desktop.exe"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
