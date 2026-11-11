; Kiln installer (NSIS 3). Built by the release workflow:
;   makensis -DVERSION=0.003.06 -DSRCDIR=<folder with kiln-shell.exe> -DOUTFILE=<setup.exe> installer/kiln.nsi
;
; Installs for the current user only (no admin prompt) into %LOCALAPPDATA%\Programs\Kiln,
; adds Start menu (and optional desktop) shortcuts and an "Installed apps" entry with an
; uninstaller. The library and settings in %LOCALAPPDATA%\Kiln are left alone.
;
; In-app updates run this silently:  kiln-<version>-setup.exe /S /UPDATE
; It then waits for Kiln to close, replaces the files and starts Kiln again.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "dev"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\target\release"
!endif
!ifndef OUTFILE
  !define OUTFILE "kiln-${VERSION}-setup.exe"
!endif

!define APPNAME "Kiln"
!define EXE "Kiln.exe"
!define PUBLISHER "Kiln"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kiln"
!define ICON "..\crates\kiln-shell\assets\kiln.ico"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "${APPNAME}"
Caption "${APPNAME} ${VERSION} Setup"
BrandingText "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Kiln"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"

VIProductVersion "1.0.0.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APPNAME} Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Install ${APPNAME} ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "Kiln turns this PC into a couch-friendly game console.$\r$\n$\r$\nIt installs just for you; no administrator rights are needed. Your game library and settings are kept if you reinstall or update.$\r$\n$\r$\nClick Next to continue."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start Kiln now"
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Add a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Var IsUpdate

; Exit code 0 when a process named Kiln.exe is running for anyone on this PC.
!macro KilnRunning OUT
  nsExec::Exec 'cmd.exe /c tasklist /FI "IMAGENAME eq ${EXE}" /NH | find /I "${EXE}"'
  Pop ${OUT}
!macroend

Function .onInit
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  ${IfNot} ${Errors}
    StrCpy $IsUpdate "1"
  ${EndIf}
FunctionEnd

; Makes sure Kiln isn't running so its files can be replaced.
Function CloseKiln
  ${If} $IsUpdate == "1"
    ; Kiln exits by itself after starting the update; give it a moment.
    StrCpy $R2 0
    ${Do}
      !insertmacro KilnRunning $R1
      ${If} $R1 != "0"
      ${OrIf} $R2 >= 20
        ${Break}
      ${EndIf}
      Sleep 500
      IntOp $R2 $R2 + 1
    ${Loop}
  ${Else}
    !insertmacro KilnRunning $R1
    ${If} $R1 == "0"
      MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "Kiln is running. Click OK to close it and continue." /SD IDOK IDOK close_it
      Abort
      close_it:
    ${EndIf}
  ${EndIf}
  nsExec::Exec 'taskkill /IM ${EXE} /F'
  Pop $R1
  Sleep 500
FunctionEnd

Function CreateDesktopShortcut
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
FunctionEnd

Section "Kiln" SecMain
  SectionIn RO
  Call CloseKiln

  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "/oname=${EXE}" "${SRCDIR}\kiln-shell.exe"
  File "${SRCDIR}\kiln-updater.exe"
  File "${SRCDIR}\kiln-scan.exe"
  Delete "$INSTDIR\${EXE}.old"

  WriteUninstaller "$INSTDIR\Uninstall Kiln.exe"
  CreateShortCut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  ; "Installed apps" / "Add or remove programs" entry
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://github.com/srijankulal/kiln"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall Kiln.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\Uninstall Kiln.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"

  ; Silent update: start Kiln again (the finish page offers this otherwise).
  ${If} $IsUpdate == "1"
    Exec '"$INSTDIR\${EXE}"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM ${EXE} /F'
  Pop $R1
  Sleep 500

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\${EXE}.old"
  Delete "$INSTDIR\kiln-updater.exe"
  Delete "$INSTDIR\kiln-scan.exe"
  Delete "$INSTDIR\Uninstall Kiln.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"

  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete your Kiln library, artwork and settings?$\r$\n$\r$\nChoose No to keep them for a future install." /SD IDNO IDNO keep
    RMDir /r "$LOCALAPPDATA\Kiln"
  keep:
SectionEnd
