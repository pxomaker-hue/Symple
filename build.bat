@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul
title Symple - compilation de l'installeur
cd /d "%~dp0"

echo.
echo  ===  Symple : compilation de l'installeur NSIS  ===
echo.

rem ---------------------------------------------------------------- outils
where cargo >nul 2>&1 || (echo [ERREUR] Rust est introuvable. Installe-le depuis https://rustup.rs & goto :fail)
where npm   >nul 2>&1 || (echo [ERREUR] Node.js est introuvable. Installe-le depuis https://nodejs.org & goto :fail)

set "MAKENSIS="
for %%M in (makensis.exe) do if not "%%~$PATH:M"=="" set "MAKENSIS=%%~$PATH:M"
if not defined MAKENSIS if exist "%ProgramFiles(x86)%\NSIS\makensis.exe" set "MAKENSIS=%ProgramFiles(x86)%\NSIS\makensis.exe"
if not defined MAKENSIS if exist "%ProgramFiles%\NSIS\makensis.exe" set "MAKENSIS=%ProgramFiles%\NSIS\makensis.exe"
if not defined MAKENSIS (
  echo [INFO] NSIS est introuvable, installation avec winget...
  winget install -e --id NSIS.NSIS --accept-package-agreements --accept-source-agreements || (echo [ERREUR] Installe NSIS depuis https://nsis.sourceforge.io & goto :fail)
  set "MAKENSIS=%ProgramFiles(x86)%\NSIS\makensis.exe"
)

rem --------------------------------------------------------------- version
for /f "usebackq delims=" %%V in (`powershell -NoProfile -Command "(Get-Content 'src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json).version"`) do set "VERSION=%%V"
if not defined VERSION set "VERSION=0.1.0"
echo [1/3] Version %VERSION%

rem ------------------------------------------------------------ dépendances
if not exist node_modules (
  echo [2/3] Installation des dépendances npm...
  call npm install --silent || goto :fail
) else (
  echo [2/3] Dépendances npm déjà présentes
)

rem ------------------------------------------------------------ compilation
echo [3/3] Compilation de Symple en mode release...
call npx tauri build --no-bundle || goto :fail
if not exist "target\release\symple.exe" (echo [ERREUR] target\release\symple.exe introuvable & goto :fail)

rem -------------------------------------------------------------- installeur
if not exist dist mkdir dist
echo.
echo Création de l'installeur...
"%MAKENSIS%" /V2 /INPUTCHARSET UTF8 /DVERSION=%VERSION% installer\symple.nsi || goto :fail

echo.
echo  [OK] Installeur prêt : dist\Symple-%VERSION%-setup.exe
echo.
start "" explorer "dist"
pause
exit /b 0

:fail
echo.
echo  [ECHEC] La compilation a échoué. Voir les messages ci-dessus.
pause
exit /b 1
