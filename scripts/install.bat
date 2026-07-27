@echo off
setlocal EnableExtensions EnableDelayedExpansion

rem Install Chaos CLI from GitHub Releases (cmd.exe, no iex required).
rem
rem Usage:
rem   install.bat
rem   install.bat --version 0.2.113
rem   install.bat --force
rem   install.bat --no-path
rem   install.bat --cn
rem   install.bat --mirror https://ghfast.top
rem
rem One-liner (download then run):
rem   curl -L -o "%TEMP%\install-chaos.bat" https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat && "%TEMP%\install-chaos.bat"
rem
rem Prefer local scripts\install.ps1 when present; otherwise download it.
rem If PowerShell is unavailable, fall back to curl/certutil + setx PATH.
rem
rem China / slow GitHub: set CHAOS_CN=1 or CHAOS_GITHUB_MIRROR=https://ghfast.top

set "REPO=chao2hang/chaos-code"
if defined CHAOS_REPO set "REPO=%CHAOS_REPO%"

set "VERSION="
if defined CHAOS_VERSION set "VERSION=%CHAOS_VERSION%"

set "FORCE=0"
set "NO_PATH=0"
set "DIR="
set "CN=0"
if defined CHAOS_CN if "%CHAOS_CN%"=="1" set "CN=1"
if defined CHAOS_MIRROR_FIRST if "%CHAOS_MIRROR_FIRST%"=="1" set "CN=1"
set "MIRROR="
if defined CHAOS_GITHUB_MIRROR set "MIRROR=%CHAOS_GITHUB_MIRROR%"

:parse_args
if "%~1"=="" goto args_done
if /I "%~1"=="--version" (
  set "VERSION=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="-Version" (
  set "VERSION=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="/version" (
  set "VERSION=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="--dir" (
  set "DIR=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="--force" (
  set "FORCE=1"
  shift
  goto parse_args
)
if /I "%~1"=="-Force" (
  set "FORCE=1"
  shift
  goto parse_args
)
if /I "%~1"=="--no-path" (
  set "NO_PATH=1"
  shift
  goto parse_args
)
if /I "%~1"=="-NoPath" (
  set "NO_PATH=1"
  shift
  goto parse_args
)
if /I "%~1"=="--cn" (
  set "CN=1"
  shift
  goto parse_args
)
if /I "%~1"=="-Cn" (
  set "CN=1"
  shift
  goto parse_args
)
if /I "%~1"=="--mirror" (
  set "MIRROR=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="-Mirror" (
  set "MIRROR=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="-h" goto usage
if /I "%~1"=="--help" goto usage
if /I "%~1"=="/?" goto usage
echo Unknown option: %~1
goto usage

:args_done

rem Strip leading v from version if present
if defined VERSION (
  if /I "!VERSION:~0,1!"=="v" set "VERSION=!VERSION:~1!"
)

set "SCRIPT_DIR=%~dp0"
set "LOCAL_PS1=%SCRIPT_DIR%install.ps1"
set "PS1=%LOCAL_PS1%"

if not exist "%LOCAL_PS1%" (
  set "PS1=%TEMP%\chaos-install-%RANDOM%%RANDOM%.ps1"
  echo Downloading install.ps1...
  call :download "https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1" "!PS1!"
  if errorlevel 1 (
    echo Failed to download install.ps1 — trying direct binary install...
    goto direct_install
  )
)

where powershell >nul 2>&1
if errorlevel 1 (
  echo PowerShell not found — trying direct binary install...
  goto direct_install
)

rem Build extra args. Avoid set "..." with nested quotes (breaks cmd parsing).
set "PS_EXTRA="
if defined VERSION set "PS_EXTRA=!PS_EXTRA! -Version %VERSION%"
if defined DIR set PS_EXTRA=!PS_EXTRA! -Dir "%DIR%"
if "%FORCE%"=="1" set "PS_EXTRA=!PS_EXTRA! -Force"
if "%NO_PATH%"=="1" set "PS_EXTRA=!PS_EXTRA! -NoPath"
if defined CHAOS_REPO set "PS_EXTRA=!PS_EXTRA! -Repo %REPO%"
if defined MIRROR set PS_EXTRA=!PS_EXTRA! -Mirror "%MIRROR%"
if "%CN%"=="1" set "PS_EXTRA=!PS_EXTRA! -Cn"
rem Also export env so downloaded install.ps1 sees them even without -File args.
if defined MIRROR set "CHAOS_GITHUB_MIRROR=%MIRROR%"
if "%CN%"=="1" set "CHAOS_CN=1"

echo Running PowerShell installer...
powershell -NoProfile -ExecutionPolicy Bypass -File "%PS1%" !PS_EXTRA!
set "RC=%ERRORLEVEL%"

if not exist "%LOCAL_PS1%" if exist "%PS1%" del /f /q "%PS1%" >nul 2>&1

if not "%RC%"=="0" (
  echo PowerShell installer failed ^(exit %RC%^). Trying direct binary install...
  goto direct_install
)
exit /b 0

:direct_install
rem Resolve install dir
if defined DIR (
  set "INSTALL_DIR=%DIR%"
) else if defined CHAOS_HOME (
  set "INSTALL_DIR=%CHAOS_HOME%\bin"
) else if defined GROK_HOME (
  set "INSTALL_DIR=%GROK_HOME%\bin"
) else if exist "%USERPROFILE%\.chaos" (
  set "INSTALL_DIR=%USERPROFILE%\.chaos\bin"
) else if exist "%USERPROFILE%\.grok" (
  set "INSTALL_DIR=%USERPROFILE%\.grok\bin"
) else (
  set "INSTALL_DIR=%USERPROFILE%\.chaos\bin"
)

rem Pick asset by arch
set "ASSET=chaos-win32-x64.exe"
if /I "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "ASSET=chaos-win32-arm64.exe"
if /I "%PROCESSOR_ARCHITEW6432%"=="ARM64" set "ASSET=chaos-win32-arm64.exe"

if not defined VERSION (
  echo Resolving latest release tag...
  set "API_JSON=%TEMP%\chaos-rel-%RANDOM%%RANDOM%.json"
  call :download_github "https://api.github.com/repos/%REPO%/releases/latest" "!API_JSON!"
  if errorlevel 1 (
    echo error: could not fetch latest release. Pass --version X.Y.Z explicitly.
    echo   tip: set CHAOS_CN=1 or CHAOS_GITHUB_MIRROR=https://ghfast.top
    exit /b 1
  )
  rem First matching tag_name line only (no labels inside parentheses).
  set "TAG="
  for /f "usebackq tokens=2 delims=:," %%A in (`findstr /C:"\"tag_name\"" "!API_JSON!"`) do (
    if not defined TAG set "TAG=%%~A"
  )
  if exist "!API_JSON!" del /f /q "!API_JSON!" >nul 2>&1
  set "TAG=!TAG: =!"
  set "TAG=!TAG:"=!"
  if defined TAG if /I "!TAG:~0,1!"=="v" set "TAG=!TAG:~1!"
  if not defined TAG (
    echo error: could not parse latest tag. Pass --version X.Y.Z.
    exit /b 1
  )
  set "VERSION=!TAG!"
)

set "URL=https://github.com/%REPO%/releases/download/v%VERSION%/%ASSET%"
set "SUMS_URL=https://github.com/%REPO%/releases/download/v%VERSION%/SHA256SUMS"
set "DEST=%INSTALL_DIR%\chaos.exe"

echo Chaos installer ^(direct^)
echo   repo:    %REPO%
echo   version: %VERSION%
echo   asset:   %ASSET%
echo   dest:    %DEST%
echo   origin:  %URL%
if defined MIRROR echo   mirror:  %MIRROR%
if "%CN%"=="1" if not defined MIRROR echo   mirror:  public list first ^(CHAOS_CN^)

rem Default is upgrade-in-place. --force re-downloads even when already on target.
if exist "%DEST%" if not "%FORCE%"=="1" (
  set "CUR_VER="
  for /f "usebackq delims=" %%V in (`"%DEST%" --version 2^>nul`) do (
    if not defined CUR_VER set "CUR_VER=%%V"
  )
  if defined CUR_VER (
    echo !CUR_VER! | findstr /C:"%VERSION%" >nul 2>&1
    if not errorlevel 1 (
      echo already installed: !CUR_VER!
      if not "%NO_PATH%"=="1" call :ensure_user_path "%INSTALL_DIR%"
      echo done. open a NEW terminal if chaos is not found.
      exit /b 0
    )
    echo upgrading existing install: !CUR_VER! -^> %VERSION%
  ) else (
    echo replacing existing binary at %DEST%
  )
)

if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%" 2>nul
set "TMP=%TEMP%\chaos-bin-%RANDOM%%RANDOM%.exe"
call :download_github "%URL%" "%TMP%"
if errorlevel 1 (
  echo error: download failed: %URL%
  echo   tip: set CHAOS_CN=1 or CHAOS_GITHUB_MIRROR=https://ghfast.top
  exit /b 1
)

rem Rough size check — HTML error pages are tiny
for %%F in ("%TMP%") do set "SZ=%%~zF"
if defined SZ if !SZ! LSS 1048576 (
  findstr /I /C:"<!DOCTYPE" /C:"<html" "%TMP%" >nul 2>&1
  if not errorlevel 1 (
    del /f /q "%TMP%" >nul 2>&1
    echo error: download looks like HTML, not a binary: %URL%
    exit /b 1
  )
)

rem Integrity: verify against the release's published SHA256SUMS before the
rem binary is copied into place. Set CHAOS_SKIP_CHECKSUM=1 to bypass.
if "%CHAOS_SKIP_CHECKSUM%"=="1" (
  echo warning: checksum verification skipped ^(CHAOS_SKIP_CHECKSUM=1^)
) else (
  set "SUMS=%TEMP%\chaos-sums-%RANDOM%%RANDOM%.txt"
  call :download_github "%SUMS_URL%" "!SUMS!"
  if errorlevel 1 (
    del /f /q "%TMP%" >nul 2>&1
    echo error: could not fetch SHA256SUMS for v%VERSION%.
    echo   This release may predate checksum publishing. To install anyway,
    echo   set CHAOS_SKIP_CHECKSUM=1 ^(you are then trusting the download^).
    exit /b 1
  )

  set "EXPECTED="
  for /f "tokens=1,2" %%A in ('type "!SUMS!"') do (
    if /I "%%B"=="%ASSET%" set "EXPECTED=%%A"
  )
  set "ACTUAL="
  for /f "skip=1 tokens=* delims=" %%H in ('certutil -hashfile "%TMP%" SHA256') do (
    if not defined ACTUAL set "ACTUAL=%%H"
  )
  set "ACTUAL=!ACTUAL: =!"
  del /f /q "!SUMS!" >nul 2>&1

  if not defined EXPECTED (
    del /f /q "%TMP%" >nul 2>&1
    echo error: SHA256SUMS has no entry for %ASSET%
    exit /b 1
  )
  if /I not "!ACTUAL!"=="!EXPECTED!" (
    del /f /q "%TMP%" >nul 2>&1
    echo error: checksum mismatch for %ASSET%
    echo   expected: !EXPECTED!
    echo   actual:   !ACTUAL!
    echo   Refusing to install. This download may be corrupt or tampered with.
    exit /b 1
  )
  echo checksum OK ^(!ACTUAL!^)
)

copy /y "%TMP%" "%DEST%" >nul
del /f /q "%TMP%" >nul 2>&1
if not exist "%DEST%" (
  echo error: failed to write %DEST%
  exit /b 1
)

echo installed: %DEST%
"%DEST%" --version 2>nul

if not "%NO_PATH%"=="1" (
  call :ensure_user_path "%INSTALL_DIR%"
)

echo.
echo OK. Verify ^(open a NEW terminal, or refresh PATH in this session^):
echo   set "PATH=%INSTALL_DIR%;%%PATH%%"
echo   chaos --version
echo Or: "%DEST%" --version
exit /b 0

:ensure_user_path
set "ADD=%~1"
rem Current session only (does not imply user PATH already has ADD)
set "PATH=%ADD%;%PATH%"
rem Persist user PATH via PowerShell if available (handles long paths better than setx)
where powershell >nul 2>&1
if not errorlevel 1 (
  powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$d='%ADD%'.TrimEnd('\');" ^
    "$u=[Environment]::GetEnvironmentVariable('Path','User'); if(-not $u){$u=''};" ^
    "$parts=$u -split ';' | Where-Object { $_ -and $_.Trim() -ne '' };" ^
    "if (-not ($parts | Where-Object { $_.TrimEnd('\') -ieq $d })) {" ^
    "  $n=if([string]::IsNullOrWhiteSpace($u)){$d}else{\"$u;$d\"};" ^
    "  [Environment]::SetEnvironmentVariable('Path',$n,'User');" ^
    "  Write-Host \"added to user PATH: $d\"" ^
    "} else { Write-Host \"already on user PATH: $d\" }"
  exit /b 0
)
rem Fallback: setx (may truncate very long PATH). Read *user* Path from registry —
rem never test session %%PATH%% (already prepended above, so setx would never run).
set "UPATH="
for /f "tokens=2*" %%A in ('reg query "HKCU\Environment" /v Path 2^>nul') do set "UPATH=%%B"
if defined UPATH (
  echo ;%UPATH%; | findstr /I /C:";%ADD%;" >nul 2>&1
  if not errorlevel 1 (
    echo already on user PATH: %ADD%
    exit /b 0
  )
  setx Path "%UPATH%;%ADD%" >nul
) else (
  setx Path "%ADD%" >nul
)
echo added to user PATH: %ADD%
exit /b 0

:download
set "DL_URL=%~1"
set "DL_OUT=%~2"
where curl >nul 2>&1
if not errorlevel 1 (
  curl -fsSL --connect-timeout 12 -A "chaos-code-installer" -o "%DL_OUT%" "%DL_URL%"
  if not errorlevel 1 if exist "%DL_OUT%" exit /b 0
)
where powershell >nul 2>&1
if not errorlevel 1 (
  powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "Invoke-WebRequest -Uri '%DL_URL%' -OutFile '%DL_OUT%' -Headers @{ 'User-Agent'='chaos-code-installer' } -UseBasicParsing"
  if not errorlevel 1 if exist "%DL_OUT%" exit /b 0
)
rem Last-resort fallback: certutil does not enforce TLS strictness as
rem strongly as curl/Invoke-WebRequest, but works on locked-down systems.
where certutil >nul 2>&1
if not errorlevel 1 (
  certutil -urlcache -split -f "%DL_URL%" "%DL_OUT%" >nul
  if not errorlevel 1 if exist "%DL_OUT%" exit /b 0
)
exit /b 1

rem Try origin + public ghproxy-style mirrors for github.com / api.github.com URLs.
:download_github
set "GH_ORIGIN=%~1"
set "GH_OUT=%~2"
set "GH_CAND="
if defined MIRROR (
  call :download "!MIRROR!/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
)
if "%CN%"=="1" (
  call :download "https://ghfast.top/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "https://ghproxy.net/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "https://mirror.ghproxy.com/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
) else (
  call :download "!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "https://ghfast.top/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "https://ghproxy.net/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
  call :download "https://mirror.ghproxy.com/!GH_ORIGIN!" "!GH_OUT!"
  if not errorlevel 1 exit /b 0
)
exit /b 1

:usage
echo.
echo Install Chaos from GitHub Releases ^(cmd, no iex^).
echo.
echo Usage:
echo   install.bat
echo   install.bat --version 0.2.113
echo   install.bat --force
echo   install.bat --no-path
echo   install.bat --cn
echo   install.bat --mirror https://ghfast.top
echo   install.bat --dir "C:\tools\chaos\bin"
echo.
echo Environment:
echo   CHAOS_VERSION         Same as --version
echo   CHAOS_HOME            Install under %%CHAOS_HOME%%\bin
echo   CHAOS_REPO            owner/repo ^(default chao2hang/chaos-code^)
echo   CHAOS_CN=1            Prefer public GitHub mirrors
echo   CHAOS_GITHUB_MIRROR   Mirror prefix, e.g. https://ghfast.top
echo.
echo One-liner:
echo   curl -L -o "%%TEMP%%\install-chaos.bat" https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat ^&^& "%%TEMP%%\install-chaos.bat"
echo.
exit /b 2
