@echo off
REM cd a la raíz del repositorio (un nivel arriba de scripts/)
cd /d "%~dp0.."
echo Compilando NWIN_OS...
cargo build 2>&1
if %ERRORLEVEL% EQU 0 (
    echo.
    echo ✅ Compilacion exitosa!
    echo Binario: target\debug\NWIN_OS
) else (
    echo.
    echo ❌ Error de compilacion
    exit /b 1
)
