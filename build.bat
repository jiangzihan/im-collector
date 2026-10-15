@echo off
chcp 65001 >nul
rem im-collector Windows build: compile release and copy exe into bin\
rem Requires: Rust (rustup) + MSVC Build Tools ("Desktop development with C++").
rem Reason: rusqlite bundled compiles C, so MSVC toolchain (cl.exe/link.exe) is needed.
setlocal
set "R=%~dp0"

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] cargo not found. Install Rust from https://rustup.rs  and VS Build Tools C++ workload.
  exit /b 1
)

echo [1/2] cargo build --release ...
pushd "%R%src"
cargo build --release
set "RC=%errorlevel%"
popd
if not "%RC%"=="0" (
  echo [ERROR] build failed. If link.exe/cl.exe missing, install "Visual Studio Build Tools" with "Desktop development with C++".
  exit /b 1
)

echo [2/2] copy exe into bin\ ...
if not exist "%R%bin" mkdir "%R%bin"
copy /y "%R%src\target\release\imc-wechat.exe"  "%R%bin\imc-wechat.exe"  >nul
copy /y "%R%src\target\release\imc-dingtalk.exe" "%R%bin\imc-dingtalk.exe" >nul

echo Done -^> %R%bin
endlocal
