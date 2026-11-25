; Franconvert for Windows: a per-user install in %LOCALAPPDATA%\Franconvert,
; laid out like the Tauri installer the 0.9.x releases used (same folder,
; uninstall entry and Start menu shortcut), so it replaces those installs in
; place.
;
;   makensis /DVERSION=0.9.5 /DEXE=..\target\release\franconvert.exe installer\franconvert.nsi
;
; Flags it understands:
;   /S   silent (the native updater runs it this way, then starts the app)
;   /P   passive, as the Tauri updater asks: silent, then reopen with /R
;   /R   start Franconvert when done
; Tests pass /DPRODUCT=<other name> so they never touch a real install.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!ifndef VERSION
  !error "Pass /DVERSION=x.y.z"
!endif
!ifndef EXE
  !define EXE "..\target\release\franconvert.exe"
!endif
!ifndef PRODUCT
  !define PRODUCT "Franconvert"
!endif
!ifndef OUTFILE
  !define OUTFILE "${PRODUCT}_${VERSION}_x64-setup.exe"
!endif
!define PUBLISHER "franconvert"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define DIR_KEY "Software\${PUBLISHER}\${PRODUCT}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APP_EXE "franconvert.exe"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
InstallDirRegKey HKCU "${DIR_KEY}" ""
BrandingText "${PRODUCT} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIFileVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT} Setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "MIT License"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "..\app\icons\icon.ico"
!define MUI_UNICON "..\app\icons\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Open ${PRODUCT}"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var Relaunch

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    SetSilent silent
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    StrCpy $Relaunch 1
  ${EndIf}
FunctionEnd

; Waits for the app to quit (an updater starts this while the app exits),
; and closes it if it's still running.
!macro CloseApp UN
Function ${UN}CloseApp
  ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
    Return
  ${EndIf}
  StrCpy $1 0
  ${Do}
    ; Deleting fails while it's running.
    ClearErrors
    Delete "$INSTDIR\${APP_EXE}"
    ${IfNot} ${Errors}
      Return
    ${EndIf}
    ${If} $1 = 8
      nsExec::Exec 'taskkill /F /IM "${APP_EXE}"'
      Pop $2
    ${EndIf}
    IntOp $1 $1 + 1
    Sleep 250
  ${LoopUntil} $1 > 40
  MessageBox MB_ICONSTOP "${PRODUCT} is still running. Quit it from its tray icon and try again." /SD IDOK
  Abort
FunctionEnd
!macroend
!insertmacro CloseApp ""
!insertmacro CloseApp "un."

; The Tauri MSI build installed for all users in Program Files. Remove it so
; only this copy is left (Windows asks for permission).
Function RemoveOldMsi
  SetRegView 64
  StrCpy $0 0
  ${Do}
    EnumRegKey $1 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall" $0
    ${If} $1 == ""
      ${Break}
    ${EndIf}
    ReadRegStr $2 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegDWORD $3 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\$1" "WindowsInstaller"
    ${If} $2 == "${PRODUCT}"
    ${AndIf} $3 = 1
      ExecWait '"$SYSDIR\msiexec.exe" /x $1 /passive /norestart'
      ${Break}
    ${EndIf}
    IntOp $0 $0 + 1
  ${Loop}
  SetRegView default
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  Call CloseApp
  File "/oname=${APP_EXE}" "${EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${APP_EXE}"

  WriteRegStr HKCU "${DIR_KEY}" "" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${APP_EXE}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" '"$INSTDIR"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "MainBinaryName" "${APP_EXE}"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0

  ; Launch at login, if it was on, now points at this copy.
  ReadRegStr $0 HKCU "${RUN_KEY}" "${PRODUCT}"
  ${If} $0 != ""
    WriteRegStr HKCU "${RUN_KEY}" "${PRODUCT}" '"$INSTDIR\${APP_EXE}"'
  ${EndIf}

  Call RemoveOldMsi
SectionEnd

Function .onInstSuccess
  ${If} $Relaunch == 1
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Call un.CloseApp
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "${PRODUCT}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${DIR_KEY}"
  DeleteRegKey /ifempty HKCU "Software\${PUBLISHER}"
  ; Settings and learned words stay in %APPDATA%\com.franconvert.app.
SectionEnd
