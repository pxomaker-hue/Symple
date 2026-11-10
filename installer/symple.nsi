; Installeur Symple (NSIS + Modern UI 2)
; Compilé par build.bat :  makensis /DVERSION=x.y.z installer\symple.nsi

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!define APP      "Symple"
!define EXE      "symple.exe"
!define PUBLISHER "Symple"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"
!define RUN_KEY    "Software\Microsoft\Windows\CurrentVersion\Run"
!define ROOT       ".."

Name "${APP}"
OutFile "${ROOT}\dist\Symple-${VERSION}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "Software\${APP}" "InstallDir"
RequestExecutionLevel user
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "Installeur ${APP}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT"

!include "MUI2.nsh"
!include "LogicLib.nsh"

; ------------------------------------------------------------- apparence
!define MUI_ICON   "${ROOT}\src-tauri\icons\icon.ico"
!define MUI_UNICON "${ROOT}\src-tauri\icons\icon.ico"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "header.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP "sidebar.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "sidebar.bmp"
!define MUI_ABORTWARNING
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "Software\${APP}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "InstallerLanguage"
!define MUI_COMPONENTSPAGE_SMALLDESC

!define MUI_WELCOMEPAGE_TITLE "$(WELCOME_TITLE)"
!define MUI_WELCOMEPAGE_TEXT "$(WELCOME_TEXT)"

!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "$(RUN_TEXT)"
!define MUI_FINISHPAGE_LINK "$(LINK_TEXT)"
!define MUI_FINISHPAGE_LINK_LOCATION "https://github.com/pxomaker-hue/Symple"

; ----------------------------------------------------------------- pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${ROOT}\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "French"
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_RESERVEFILE_LANGDLL

; ------------------------------------------------------------- textes FR / EN
LangString WELCOME_TITLE ${LANG_FRENCH}  "Bienvenue dans l'installation de ${APP}"
LangString WELCOME_TITLE ${LANG_ENGLISH} "Welcome to ${APP} Setup"
LangString WELCOME_TEXT ${LANG_FRENCH}  "${APP} synchronise vos dossiers en temps réel, sur votre PC ou votre NAS.$\r$\n$\r$\nAucun droit administrateur n'est nécessaire.$\r$\n$\r$\nCliquez sur Suivant pour continuer."
LangString WELCOME_TEXT ${LANG_ENGLISH} "${APP} keeps your folders in sync in real time, on your PC or your NAS.$\r$\n$\r$\nNo administrator rights needed.$\r$\n$\r$\nClick Next to continue."
LangString RUN_TEXT ${LANG_FRENCH}  "Lancer ${APP} maintenant"
LangString RUN_TEXT ${LANG_ENGLISH} "Launch ${APP} now"
LangString LINK_TEXT ${LANG_FRENCH}  "Projet sur GitHub"
LangString LINK_TEXT ${LANG_ENGLISH} "Project on GitHub"
LangString SEC_APP ${LANG_FRENCH}  "${APP} (requis)"
LangString SEC_APP ${LANG_ENGLISH} "${APP} (required)"
LangString SEC_DESKTOP ${LANG_FRENCH}  "Raccourci sur le Bureau"
LangString SEC_DESKTOP ${LANG_ENGLISH} "Desktop shortcut"
LangString SEC_START ${LANG_FRENCH}  "Raccourci dans le menu Démarrer"
LangString SEC_START ${LANG_ENGLISH} "Start menu shortcut"
LangString SEC_AUTO ${LANG_FRENCH}  "Lancer au démarrage de Windows"
LangString SEC_AUTO ${LANG_ENGLISH} "Start with Windows"
LangString DESC_APP ${LANG_FRENCH}  "L'application ${APP}."
LangString DESC_APP ${LANG_ENGLISH} "The ${APP} application."
LangString DESC_DESKTOP ${LANG_FRENCH}  "Ajoute une icône ${APP} sur le Bureau."
LangString DESC_DESKTOP ${LANG_ENGLISH} "Adds a ${APP} icon to the desktop."
LangString DESC_START ${LANG_FRENCH}  "Ajoute ${APP} au menu Démarrer."
LangString DESC_START ${LANG_ENGLISH} "Adds ${APP} to the Start menu."
LangString DESC_AUTO ${LANG_FRENCH}  "Démarre ${APP} réduit dans la barre des tâches à l'ouverture de session."
LangString DESC_AUTO ${LANG_ENGLISH} "Starts ${APP} minimized to the taskbar when you sign in."
LangString UNINST_LNK ${LANG_FRENCH}  "Désinstaller ${APP}"
LangString UNINST_LNK ${LANG_ENGLISH} "Uninstall ${APP}"
LangString WEBVIEW_MSG ${LANG_FRENCH}  "Microsoft WebView2 semble absent : ${APP} en a besoin pour s'afficher.$\r$\nOuvrir la page de téléchargement ?"
LangString WEBVIEW_MSG ${LANG_ENGLISH} "Microsoft WebView2 seems to be missing: ${APP} needs it to display.$\r$\nOpen the download page?"
LangString UNSEC_DATA ${LANG_FRENCH}  "Supprimer mes paires et réglages"
LangString UNSEC_DATA ${LANG_ENGLISH} "Remove my pairs and settings"
LangString UNDESC_APP ${LANG_FRENCH}  "Supprime l'application et ses raccourcis. Vos fichiers synchronisés ne sont jamais touchés."
LangString UNDESC_APP ${LANG_ENGLISH} "Removes the application and its shortcuts. Your synced files are never touched."
LangString UNDESC_DATA ${LANG_FRENCH}  "Supprime aussi la configuration des paires et l'historique de synchro."
LangString UNDESC_DATA ${LANG_ENGLISH} "Also removes the pair configuration and sync history."

Function .onInit
  !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Function un.onInit
  !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

; -------------------------------------------------------------- sections
Section "$(SEC_APP)" SecApp
  SectionIn RO
  ; Ferme une instance en cours pour pouvoir remplacer l'exécutable.
  nsExec::Exec 'taskkill /f /im "${EXE}"'
  Sleep 500

  SetOutPath "$INSTDIR"
  File "${ROOT}\target\release\${EXE}"
  File /oname=icon.ico "${ROOT}\src-tauri\icons\icon.ico"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\${APP}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; WebView2 est fourni avec Windows 10/11 ; on prévient s'il manque.
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ReadRegStr $1 HKCU "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${If} $0 == ""
  ${AndIf} $1 == ""
    MessageBox MB_YESNO|MB_ICONEXCLAMATION "$(WEBVIEW_MSG)" IDNO +2
      ExecShell "open" "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
  ${EndIf}
SectionEnd

Section "$(SEC_DESKTOP)" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\icon.ico"
SectionEnd

Section "$(SEC_START)" SecStartMenu
  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\icon.ico"
  CreateShortcut "$SMPROGRAMS\${APP}\$(UNINST_LNK).lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section /o "$(SEC_AUTO)" SecAutostart
  ; Même clé que l'option « Lancer au démarrage » de l'application.
  WriteRegStr HKCU "${RUN_KEY}" "${APP}" '"$INSTDIR\${EXE}" --minimized'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "$(DESC_APP)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "$(DESC_DESKTOP)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "$(DESC_START)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "$(DESC_AUTO)"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---------------------------------------------------------- désinstallation
Section "un.${APP}" UnSecApp
  SectionIn RO
  nsExec::Exec 'taskkill /f /im "${EXE}"'
  Sleep 500
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$DESKTOP\${APP}.lnk"
  RMDir /r "$SMPROGRAMS\${APP}"
  DeleteRegValue HKCU "${RUN_KEY}" "${APP}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${APP}"
SectionEnd

Section /o "un.$(UNSEC_DATA)" UnSecData
  RMDir /r "$APPDATA\org.symple.app"
  RMDir /r "$LOCALAPPDATA\org.symple.app"
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecApp} "$(UNDESC_APP)"
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecData} "$(UNDESC_DATA)"
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
