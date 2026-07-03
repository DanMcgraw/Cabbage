@echo off
setlocal

pushd "%~dp0"

cargo +stable build
if errorlevel 1 (
    popd
    exit /b 1
)

if not exist "..\..\plugins" (
    mkdir "..\..\plugins"
)

set "PLUGIN_SOURCE=%CD%\target\debug\cabbage.dll"
set "PLUGIN_DEST=%CD%\..\..\plugins\cabbage.dll"

powershell -NoProfile -ExecutionPolicy Bypass -Command "try { Copy-Item -LiteralPath $env:PLUGIN_SOURCE -Destination $env:PLUGIN_DEST -Force -ErrorAction Stop; exit 0 } catch { Write-Error $_; exit 1 }"
if errorlevel 1 (
    popd
    exit /b 1
)

set "RUNTIME_PLUGIN_DIR=%CD%\..\..\..\plugins"
if exist "%RUNTIME_PLUGIN_DIR%" (
    set "PLUGIN_DEST=%RUNTIME_PLUGIN_DIR%\cabbage.dll"
    powershell -NoProfile -ExecutionPolicy Bypass -Command "try { Copy-Item -LiteralPath $env:PLUGIN_SOURCE -Destination $env:PLUGIN_DEST -Force -ErrorAction Stop; exit 0 } catch { Write-Error $_; exit 1 }"
    if errorlevel 1 (
        popd
        exit /b 1
    )
)

popd
endlocal
