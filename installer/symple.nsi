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
!define MUI_COMPONENTSPAGE_SMALLDESC

!define MUI_WELCOMEPAGE_TITLE "Bienvenue dans l'installation de ${APP}"
!define MUI_WELCOMEPAGE_TEXT "${APP} synchronise vos dossiers en temps réel, sur votre PC ou votre NAS.$\r$\n$\r$\nAucun droit administrateur n'est nécessaire.$\r$\n$\r$\nCliquez sur Suivant pour continuer."

!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Lancer ${APP} maintenant"
!define MUI_FINISHPAGE_LINK "Projet sur GitHub"
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

; -------------------------------------------------------------- sections
Section "${APP} (requis)" SecApp
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
    MessageBox MB_YESNO|MB_ICONEXCLAMATION "Microsoft WebView2 semble absent : ${APP} en a besoin pour s'afficher.$\r$\nOuvrir la page de téléchargement ?" IDNO +2
      ExecShell "open" "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
  ${EndIf}
SectionEnd

Section "Raccourci sur le Bureau" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\icon.ico"
SectionEnd

Section "Raccourci dans le menu Démarrer" SecStartMenu
  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\icon.ico"
  CreateShortcut "$SMPROGRAMS\${APP}\Désinstaller ${APP}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section /o "Lancer au démarrage de Windows" SecAutostart
  ; Même clé que l'option « Lancer au démarrage » de l'application.
  WriteRegStr HKCU "${RUN_KEY}" "${APP}" '"$INSTDIR\${EXE}" --minimized'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "L'application ${APP}."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Ajoute une icône ${APP} sur le Bureau."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "Ajoute ${APP} au menu Démarrer."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "Démarre ${APP} réduit dans la barre des tâches à l'ouverture de session."
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

Section /o "un.Supprimer mes paires et réglages" UnSecData
  RMDir /r "$APPDATA\org.symple.app"
  RMDir /r "$LOCALAPPDATA\org.symple.app"
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecApp} "Supprime l'application et ses raccourcis. Vos fichiers synchronisés ne sont jamais touchés."
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecData} "Supprime aussi la configuration des paires et l'historique de synchro."
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
