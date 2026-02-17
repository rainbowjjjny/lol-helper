@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd /d D:\lol
C:\Users\rainbow\.cargo\bin\cargo.exe build --release 2>&1
echo EXIT_CODE=%ERRORLEVEL%
