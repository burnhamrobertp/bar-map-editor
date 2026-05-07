; BAR - Map Editor — NSIS Installer Script
; Requires NSIS 3.x
;
; Version is supplied at build time via the workflow (`/DVERSION=...`).
; The fallback is "0.0.0" so a local build without that argument
; doesn't fail the !define rule below.

!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

; General
Name "BAR - Map Editor"
OutFile "bar-map-editor-Setup.exe"
InstallDir "$PROGRAMFILES64\BAR Map Editor"
InstallDirRegKey HKLM "Software\BarMapEditor" "InstallDir"
RequestExecutionLevel admin

; Version info
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "BAR - Map Editor"
VIAddVersionKey "FileDescription" "BAR - Map Editor Installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "bar-editor Contributors"

; Interface Settings
!define MUI_ABORTWARNING
!define MUI_ICON "..\..\assets\bar.ico"

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Language
!insertmacro MUI_LANGUAGE "English"

; Installer Section
Section "Install"
    SetOutPath "$INSTDIR"

    ; Application files
    File "bar-editor.exe"
    File "bar-cli.exe"

    ; Create uninstaller
    WriteUninstaller "$INSTDIR\Uninstall.exe"

    ; Start menu shortcuts
    CreateDirectory "$SMPROGRAMS\BAR Map Editor"
    CreateShortCut "$SMPROGRAMS\BAR Map Editor\BAR Map Editor.lnk" "$INSTDIR\bar-editor.exe"
    CreateShortCut "$SMPROGRAMS\BAR Map Editor\BAR Map Editor CLI.lnk" "$INSTDIR\bar-cli.exe"
    CreateShortCut "$SMPROGRAMS\BAR Map Editor\Uninstall.lnk" "$INSTDIR\Uninstall.exe"

    ; Desktop shortcut
    CreateShortCut "$DESKTOP\BAR Map Editor.lnk" "$INSTDIR\bar-editor.exe"

    ; Registry info for Add/Remove Programs
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BarMapEditor" \
        "DisplayName" "BAR - Map Editor"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BarMapEditor" \
        "UninstallString" "$\"$INSTDIR\Uninstall.exe$\""
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BarMapEditor" \
        "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BarMapEditor" \
        "Publisher" "bar-editor Contributors"
    WriteRegStr HKLM "Software\BarMapEditor" "InstallDir" "$INSTDIR"

    ; Add to PATH (optional — CLI access)
    EnVar::AddValue "PATH" "$INSTDIR"
SectionEnd

; Uninstaller Section
Section "Uninstall"
    Delete "$INSTDIR\bar-editor.exe"
    Delete "$INSTDIR\bar-cli.exe"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"

    ; Remove shortcuts
    Delete "$SMPROGRAMS\BAR Map Editor\*.lnk"
    RMDir "$SMPROGRAMS\BAR Map Editor"
    Delete "$DESKTOP\BAR Map Editor.lnk"

    ; Remove registry entries
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BarMapEditor"
    DeleteRegKey HKLM "Software\BarMapEditor"

    ; Remove from PATH
    EnVar::DeleteValue "PATH" "$INSTDIR"
SectionEnd
