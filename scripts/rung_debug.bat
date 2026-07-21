@echo off
REM ========================================
REM run_debug.bat - Arranca QEMU con monitor
REM ========================================
REM Necesitas DOS terminales:
REM   Terminal 1: ejecuta este script
REM   Terminal 2: telnet localhost 4444 para el monitor
REM ========================================

REM cd a la raiz del repositorio (un nivel arriba de scripts/) para que
REM las rutas relativas (fat:rw:esp, serial.log, ext4_test.img) funcionen correctamente.
cd /d "%~dp0.."

start "QEMU-MAIN" qemu-system-x86_64 ^
  -drive if=pflash,format=raw,readonly=on,file="C:\Program Files\qemu\share\edk2-x86_64-code.fd" ^
  -drive format=raw,file=fat:rw:esp ^
  -drive format=raw,file=tests/disk-images/ext4_test.img ^
  -m 512 ^
  -monitor stdio ^
  -serial file:serial.log ^