@echo off
chcp 65001 >nul
rem im-collector Windows manager: build / collect / watch / web / mcp.
rem Reading a same-user process memory needs no admin. If a target is denied,
rem re-run this window as Administrator (right-click -> Run as administrator).
rem No systemd on Windows; watch runs in foreground (close window to stop). Autostart: Task Scheduler.
setlocal EnableExtensions EnableDelayedExpansion
set "R=%~dp0"
if "%R:~-1%"=="\" set "R=%R:~0,-1%"
set "CMD=%~1"
if "%CMD%"=="" set "CMD=help"

set "IMC_HOME=%R%"

if /i "%CMD%"=="help"    goto :help
if /i "%CMD%"=="build"   goto :build
if /i "%CMD%"=="collect" goto :collect
if /i "%CMD%"=="watch"   goto :watch
if /i "%CMD%"=="web"     goto :web
if /i "%CMD%"=="mcp"     goto :mcp
echo Unknown command: %CMD%
goto :help

:build
call "%R%\build.bat"
goto :end

:collect
if not exist "%R%\bin\imc-dingtalk.exe" ( echo bin\ not found, run: imc.bat build & goto :end )
set "WHO=%~2"
if "%WHO%"=="" set "WHO=all"
if /i "%WHO%"=="all" (
  echo [wechat] collect ...
  "%R%\bin\imc-wechat.exe" collect
  echo [dingtalk] collect ...
  "%R%\bin\imc-dingtalk.exe" collect
) else (
  "%R%\bin\imc-%WHO%.exe" collect
)
goto :end

:watch
if not exist "%R%\bin\imc-dingtalk.exe" ( echo bin\ not found, run: imc.bat build & goto :end )
set "WHO=%~2"
if "%WHO%"=="" set "WHO=dingtalk"
set "IV=%~3"
if "%IV%"=="" set "IV=60"
echo [%WHO%] watch every %IV%s (close window to stop)
"%R%\bin\imc-%WHO%.exe" watch --interval %IV%
goto :end

:web
set "PY="
where python >nul 2>&1 && set "PY=python"
if not defined PY ( where py >nul 2>&1 && set "PY=py" )
if not defined PY ( echo Python 3 required for web/MCP & goto :end )
set "IMC_WEB_PORT=8899"
echo Local web: http://127.0.0.1:8899  (Ctrl+C to stop)
%PY% "%R%\web\serve.py"
goto :end

:mcp
where claude >nul 2>&1
if errorlevel 1 ( echo Claude Code CLI required & goto :end )
set "HASUV="
where uv >nul 2>&1 && set "HASUV=1"
if defined HASUV (
  echo Registering MCP via uv ...
  claude mcp add im-collector --scope user -- uv run --with "mcp<2" --directory "%R%\mcp" python "%R%\mcp\server.py"
) else (
  echo uv not found. Install uv then rerun, or "pip install mcp" then run:
  echo   claude mcp add im-collector --scope user -- python "%R%\mcp\server.py"
)
goto :end

:help
echo im-collector manager ^(Windows^)
echo   imc.bat build                       build and copy exe into bin\ ^(needs Rust + MSVC^)
echo   imc.bat collect [wechat^|dingtalk]   collect once ^(default both; needs admin, auto-elevates^)
echo   imc.bat watch   [wechat^|dingtalk] [sec]  keep collecting ^(default dingtalk 60s; needs admin^)
echo   imc.bat web                         local web http://127.0.0.1:8899 ^(needs Python 3^)
echo   imc.bat mcp                         register MCP to Claude Code ^(needs uv or pip mcp^)
echo.
echo collect/watch are Rust exe ^(no Python^). DingTalk names are dynamic-mapped:
echo if a chat shows a numeric id, open that chat in DingTalk then collect again. See docs\00-overview.md
goto :end

:end
endlocal
