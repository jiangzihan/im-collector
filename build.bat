@echo off
rem im-collector Windows 构建脚本: 编译 release 并把 exe 拷到 bin\
rem 需要: Rust(rustup) + MSVC 生成工具(VS Build Tools 的 "使用 C++ 的桌面开发")
rem 原因: rusqlite bundled 要编译 C, Windows 上需 MSVC 工具链(cl.exe/link.exe)。
setlocal
set "R=%~dp0"

where cargo >nul 2>&1
if errorlevel 1 (
  echo [错误] 未找到 cargo。请先安装 Rust: https://rustup.rs  (并安装 VS Build Tools 的 C++ 工作负载)
  exit /b 1
)

echo [1/2] cargo build --release ...
pushd "%R%src"
cargo build --release
set "RC=%errorlevel%"
popd
if not "%RC%"=="0" (
  echo [错误] 构建失败。若报缺少 link.exe/cl.exe, 请安装 "Visual Studio 生成工具" 并勾选 "使用 C++ 的桌面开发"。
  exit /b 1
)

echo [2/2] 拷贝可执行文件到 bin\ ...
if not exist "%R%bin" mkdir "%R%bin"
copy /y "%R%src\target\release\imc-wechat.exe"  "%R%bin\imc-wechat.exe"  >nul
copy /y "%R%src\target\release\imc-dingtalk.exe" "%R%bin\imc-dingtalk.exe" >nul

echo 完成 -^> %R%bin
echo   imc-wechat.exe  imc-dingtalk.exe
endlocal
