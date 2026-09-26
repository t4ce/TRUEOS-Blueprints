@echo off
setlocal
cd /d "%~dp0"
if not exist "war3.exe" (
  echo Copy this file into your existing Warcraft III 1.21b game directory first.
  pause
  exit /b 1
)
if not exist "w3l.exe" (
  echo No w3l.exe found. Supply a TRUSTED loader compatible with RoC 1.21b.
  echo The modern w3lh/w3l loader excludes versions below 1.22a.
  echo No loader or copyrighted Warcraft III game files are included in W3Box.
  pause
  exit /b 1
)
echo Starting your existing loader. Its compatibility has NOT been verified by W3Box.
echo Select your PRIVATE realm, not an official Battle.net gateway.
start "" "w3l.exe" -classic -window
