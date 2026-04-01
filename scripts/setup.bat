@echo off
setlocal enabledelayedexpansion

echo === Playlist Dev Environment Setup (Windows) ===
echo.

:: --- Node.js ---
where node >nul 2>&1
if %ERRORLEVEL% equ 0 (
    for /f "tokens=*" %%v in ('node --version') do echo [OK] Node.js %%v
) else (
    echo [!!] Node.js not found.
    echo      Download from https://nodejs.org/
    echo      Or install via winget: winget install OpenJS.NodeJS.LTS
    exit /b 1
)

:: --- npm ---
where npm >nul 2>&1
if %ERRORLEVEL% equ 0 (
    for /f "tokens=*" %%v in ('npm --version') do echo [OK] npm %%v
) else (
    echo [!!] npm not found. It should come with Node.js.
    exit /b 1
)

:: --- Rust ---
where rustc >nul 2>&1
if %ERRORLEVEL% equ 0 (
    for /f "tokens=2" %%v in ('rustc --version') do echo [OK] Rust %%v
) else (
    echo [..] Rust not found. Installing via rustup...
    echo      Downloading rustup-init.exe...
    curl -sSf -o rustup-init.exe https://win.rustup.rs/x86_64
    if %ERRORLEVEL% neq 0 (
        echo [!!] Failed to download rustup. Install manually from https://rustup.rs/
        exit /b 1
    )
    rustup-init.exe -y --default-toolchain stable-x86_64-pc-windows-msvc
    del rustup-init.exe
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    for /f "tokens=2" %%v in ('rustc --version') do echo [OK] Rust %%v
)

:: --- Check for MSVC Build Tools ---
echo.
echo [..] Checking for Visual Studio Build Tools...
where cl.exe >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo [OK] MSVC compiler found
) else (
    :: Check common VS paths
    if exist "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC" (
        echo [OK] VS 2022 Build Tools found
    ) else if exist "C:\Program Files\Microsoft Visual Studio\2022\Community\VC" (
        echo [OK] VS 2022 Community found
    ) else if exist "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC" (
        echo [OK] VS 2019 Build Tools found
    ) else (
        echo [!!] Visual Studio Build Tools not found.
        echo      Install from: https://visualstudio.microsoft.com/visual-cpp-build-tools/
        echo      Select "Desktop development with C++" workload.
        echo      Or run: winget install Microsoft.VisualStudio.2022.BuildTools
        exit /b 1
    )
)

:: --- Check for WebView2 ---
echo.
reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BEB-EB81BBD72085}" >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo [OK] WebView2 runtime installed
) else (
    reg query "HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BEB-EB81BBD72085}" >nul 2>&1
    if %ERRORLEVEL% equ 0 (
        echo [OK] WebView2 runtime installed
    ) else (
        echo [!!] WebView2 runtime not found.
        echo      Download from https://developer.microsoft.com/en-us/microsoft-edge/webview2/
        echo      (Usually pre-installed on Windows 10/11)
    )
)

:: --- Install npm dependencies ---
echo.
echo [..] Installing npm dependencies...
call npm install
if %ERRORLEVEL% neq 0 (
    echo [!!] npm install failed
    exit /b 1
)

:: --- Verify Tauri CLI ---
echo.
for /f "tokens=*" %%v in ('npx tauri --version 2^>nul') do (
    echo [OK] Tauri CLI %%v
)

:: --- Pre-build Rust dependencies ---
echo.
echo [..] Pre-building Rust dependencies (this may take a few minutes on first run)...
cd src-tauri
cargo check
if %ERRORLEVEL% neq 0 (
    echo [!!] Rust build check failed
    cd ..
    exit /b 1
)
cd ..

echo.
echo === Setup complete! ===
echo.
echo Run the app:  npx tauri dev
echo Build:        npx tauri build

endlocal
