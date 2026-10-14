@echo off
rem im-collector Windows 管理脚本: 构建 / 采集 / 监听 / web。
rem 采集需读取 钉钉/微信 进程内存 -> 需要管理员(SeDebugPrivilege), 本脚本会自动请求提权。
rem (Windows 下没有 systemd; watch 就是前台常驻, 关窗即停。开机自启可用"任务计划程序"。)
setlocal EnableExtensions EnableDelayedExpansion
set "R=%~dp0"
if "%R:~-1%"=="\" set "R=%R:~0,-1%"
set "CMD=%~1"
if "%CMD%"=="" set "CMD=help"

rem ---- 需要管理员的命令: collect / watch ----
set "NEEDADMIN="
if /i "%CMD%"=="collect" set "NEEDADMIN=1"
if /i "%CMD%"=="watch"   set "NEEDADMIN=1"
if defined NEEDADMIN (
  net session >nul 2>&1
  if errorlevel 1 (
    echo 需要管理员权限读取进程内存, 正在提权(会弹 UAC)...
    powershell -NoProfile -Command "Start-Process cmd -Verb RunAs -ArgumentList '/k','cd /d \"%R%\" && \"%~f0\" %*'"
    exit /b
  )
)

set "IMC_HOME=%R%"

if /i "%CMD%"=="help"    goto :help
if /i "%CMD%"=="build"   goto :build
if /i "%CMD%"=="collect" goto :collect
if /i "%CMD%"=="watch"   goto :watch
if /i "%CMD%"=="web"     goto :web
if /i "%CMD%"=="mcp"     goto :mcp
echo 未知命令: %CMD%
goto :help

:build
call "%R%\build.bat"
goto :end

:collect
if not exist "%R%\bin\imc-dingtalk.exe" ( echo 未找到 bin\, 请先 imc.bat build & goto :end )
set "WHO=%~2"
if "%WHO%"=="" set "WHO=all"
if /i "%WHO%"=="all" (
  echo [微信] 采集...
  "%R%\bin\imc-wechat.exe" collect
  echo [钉钉] 采集...
  "%R%\bin\imc-dingtalk.exe" collect
) else (
  "%R%\bin\imc-%WHO%.exe" collect
)
goto :end

:watch
if not exist "%R%\bin\imc-dingtalk.exe" ( echo 未找到 bin\, 请先 imc.bat build & goto :end )
set "WHO=%~2"
if "%WHO%"=="" set "WHO=dingtalk"
set "IV=%~3"
if "%IV%"=="" set "IV=60"
echo [%WHO%] watch 每 %IV%s 采集一次 (关闭本窗口即停)
"%R%\bin\imc-%WHO%.exe" watch --interval %IV%
goto :end

:web
set "PY="
where python >nul 2>&1 && set "PY=python"
if not defined PY ( where py >nul 2>&1 && set "PY=py" )
if not defined PY ( echo 需要 Python 3 运行 web/MCP & goto :end )
set "IMC_WEB_PORT=8899"
echo 本地 Web: http://127.0.0.1:8899  (Ctrl+C 停止) [仅用 Python 标准库, 无需 pip]
%PY% "%R%\web\serve.py"
goto :end

:mcp
rem 注册 MCP 到 Claude Code。MCP 需要 mcp 包: 有 uv 则自动按需装, 否则需自行 pip install mcp。
where claude >nul 2>&1
if errorlevel 1 ( echo 需要 Claude Code CLI(claude^) & goto :end )
set "HASUV="
where uv >nul 2>&1 && set "HASUV=1"
if defined HASUV (
  echo 用 uv 注册(自动装 mcp 依赖^)...
  claude mcp add im-collector --scope user -- uv run --with "mcp<2" --directory "%R%\mcp" python "%R%\mcp\server.py"
) else (
  echo 未找到 uv。两种办法:
  echo   A^) 装 uv(推荐^): https://docs.astral.sh/uv/  然后重跑 imc.bat mcp
  echo   B^) 已 "pip install mcp" 的话手动注册:
  echo      claude mcp add im-collector --scope user -- python "%R%\mcp\server.py"
)
goto :end

:help
echo im-collector 管理(Windows)
echo   imc.bat build                 编译并拷到 bin\ (需 Rust + MSVC 生成工具)
echo   imc.bat collect [wechat^|dingtalk]   采集一次(省略=两者; 需管理员, 会自动提权)
echo   imc.bat watch   [wechat^|dingtalk] [秒]  常驻监听(默认 dingtalk 60s; 需管理员)
echo   imc.bat web                   启动本地网页 http://127.0.0.1:8899 (需 Python3, 仅标准库)
echo   imc.bat mcp                   注册 MCP 到 Claude Code (需 uv 或已 pip install mcp)
echo.
echo Python 说明: 采集(collect/watch)是 Rust exe, 不需要 Python;
echo   web 只用 Python 标准库(无需 pip); MCP 需要 mcp 包(uv 自动装, 或 pip install mcp)。
echo 前置: 对应客户端已登录运行。钉钉群名/发送者为动态映射, 某会话显示数字 id 时,
echo       在钉钉打开该会话后再 collect(或网页点"立即采集")即可补全。详见 docs\00-overview.md。
goto :end

:end
endlocal
