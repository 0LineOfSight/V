@echo off
echo Stopping nodes (node.exe)...
taskkill /IM node.exe /F >nul 2>&1
echo Done.
