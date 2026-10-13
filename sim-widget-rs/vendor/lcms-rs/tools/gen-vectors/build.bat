@echo off
call "C:\Programs\Vs\23\VC\Auxiliary\Build\vcvars64.bat" >nul
cl /nologo /O2 /I"C:\vcpkg\installed\x64-windows\include" ^
   "%~dp0gen.c" ^
   "C:\vcpkg\installed\x64-windows\lib\lcms2.lib" ^
   /Fe:"%~dp0gen.exe" /Fo:"%~dp0gen.obj"
