# Builds the C oracle (clang) from the UNMODIFIED FFmpeg 9.0.1 + libjpeg-turbo sources and writes tests/data/fdct_kat.txt.
# TEST-ONLY tooling. Needs clang on PATH, the FFmpeg tree (ffmpeg-rs/data/_ref/FFmpeg) and libjpeg-turbo (D:/Projects/vfx.ref).
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$ff = Join-Path $here '../../../ffmpeg-rs/data/_ref/FFmpeg'
$tj = 'D:/Projects/vfx.ref/libjpeg-turbo/src'
$tmp = Join-Path $env:TEMP 'fdct_kat_build'
New-Item -ItemType Directory -Force $tmp, (Join-Path $here '../data') | Out-Null
$inc = Join-Path $here 'include'
$incff = Join-Path $here 'include_ff'
$common = @('-O1', '-fwrapv', '-w', '-c')
& clang @common -DBITS_IN_JSAMPLE=8 -DKAT_SIMD "-I$inc" "-I$tj" (Join-Path $here 'tj_islow.c') -o (Join-Path $tmp 'tj8.o')
if ($LASTEXITCODE) { throw 'tj8 failed' }
& clang @common -DBITS_IN_JSAMPLE=12 "-I$inc" "-I$tj" (Join-Path $here 'tj_islow.c') -o (Join-Path $tmp 'tj12.o')
if ($LASTEXITCODE) { throw 'tj12 failed' }
& clang @common -DBITS_IN_JSAMPLE=8 -DKAT_INT -Djpeg_fdct_islow=jpeg_fdct_islow_int "-I$inc" "-I$tj" (Join-Path $here 'tj_islow.c') -o (Join-Path $tmp 'tj8i.o')
if ($LASTEXITCODE) { throw 'tj8i failed' }
& clang @common -DBITS_IN_JSAMPLE=8 "-I$inc" "-I$tj" (Join-Path $here 'tj_ifast.c') -o (Join-Path $tmp 'tjf8.o')
if ($LASTEXITCODE) { throw 'tjf8 failed' }
& clang @common -DBITS_IN_JSAMPLE=12 "-I$inc" "-I$tj" (Join-Path $here 'tj_ifast.c') -o (Join-Path $tmp 'tjf12.o')
if ($LASTEXITCODE) { throw 'tjf12 failed' }
& clang @common "-I$inc" "-I$tj" (Join-Path $here 'tj_float.c') -o (Join-Path $tmp 'tjfl.o')
if ($LASTEXITCODE) { throw 'tjfl failed' }
& clang @common "-I$incff" "-I$ff" "-I$ff/libavcodec" (Join-Path $here 'fdct_kat.c') -o (Join-Path $tmp 'kat.o')
if ($LASTEXITCODE) { throw 'kat failed' }
& clang (Join-Path $tmp 'kat.o') (Join-Path $tmp 'tj8.o') (Join-Path $tmp 'tj12.o') (Join-Path $tmp 'tj8i.o') (Join-Path $tmp 'tjf8.o') (Join-Path $tmp 'tjf12.o') (Join-Path $tmp 'tjfl.o') -o (Join-Path $tmp 'fdct_kat.exe')
if ($LASTEXITCODE) { throw 'link failed' }
& (Join-Path $tmp 'fdct_kat.exe') | Set-Content -Encoding ascii (Join-Path $here '../data/fdct_kat.txt')
Get-Item (Join-Path $here '../data/fdct_kat.txt') | Select-Object Name, Length
