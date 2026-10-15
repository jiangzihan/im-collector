@echo off
chcp 65001 >nul
rem im-collector Windows manager: build / start / stop / restart / status / collect / watch / web / mcp.
rem No systemd on Windows: start launches each service as a background minimized window;
rem stop/status use taskkill/tasklist (collectors have unique exe names; web by window title + port).
rem Reading a same-user process memory needs NO admin. If a target is denied, run this window as admin.
rem Autostart on login: Task Scheduler -> action "imc.bat start".
setlocal EnableExtensions
set "R=%~dp0"
if "%R:~-1%"=="\" set "R=%R:~0,-1%"
set "CMD=%~1"
if "%CMD%"=="" set "CMD=help"
set "IMC_HOME=%R%"

if /i "%CMD%"=="help"    goto :help
if /i "%CMD%"=="build"   goto :build
if /i "%CMD%"=="start"   goto :start
if /i "%CMD%"=="stop"    goto :stop
if /i "%CMD%"=="restart" goto :restart
if /i "%CMD%"=="status"  goto :status
if /i "%CMD%"=="collect" goto :collect
if /i "%CMD%"=="watch"   goto :watch
if /i "%CMD%"=="web"     goto :web
if /i "%CMD%"=="mcp"     goto :mcp
echo Unknown command: %CMD%
goto :help

:build
call "%R%\build.bat"
goto :end

:start
if not exist "%R%\bin\imc-dingtalk.exe" ( echo bin\ not found, run: imc.bat build & goto :end )
set "WHO=%~2"
if "%WHO%"=="" set "WHO=all"
if /i "%WHO%"=="all" ( call :startone wechat & call :startone dingtalk & call :startone web ) else ( call :startone %WHO% )
echo (use "imc.bat status" to check, "imc.bat stop" to stop)
goto :end

:stop
set "WHO=%~2"
if "%WHO%"=="" set "WHO=all"
if /i "%WHO%"=="all" ( call :stopone wechat & call :stopone dingtalk & call :stopone web ) else ( call :stopone %WHO% )
goto :end

:restart
set "WHO=%~2"
if "%WHO%"=="" set "WHO=all"
if /i "%WHO%"=="all" (
  call :stopone wechat & call :stopone dingtalk & call :stopone web
  call :startone wechat & call :startone dingtalk & call :startone web
) else (
  call :stopone %WHO% & call :startone %WHO%
)
goto :end

:status
echo service    state
call :st wechat   imc-wechat.exe
call :st dingtalk imc-dingtalk.exe
netstat -ano 2>nul | find ":8899" >nul && (echo web        running ^(http://127.0.0.1:8899^)) || (echo web        stopped)
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
echo   imc.bat build                        build and copy exe into bin\ ^(needs Rust + MSVC^)
echo   imc.bat start   [wechat^|dingtalk^|web] start as background windows ^(default all^)
echo   imc.bat stop    [wechat^|dingtalk^|web] stop ^(default all^)
echo   imc.bat restart [wechat^|dingtalk^|web] restart ^(default all^)
echo   imc.bat status                       show running state
echo   imc.bat collect [wechat^|dingtalk]    collect once ^(default both^)
echo   imc.bat watch   [wechat^|dingtalk] [sec]  foreground watch ^(default dingtalk 60s^)
echo   imc.bat web                          foreground web http://127.0.0.1:8899 ^(needs Python 3^)
echo   imc.bat mcp                          register MCP to Claude Code ^(needs uv or pip mcp^)
echo.
echo Notes: collect/watch are Rust exe ^(no Python^). Reading same-user process memory needs no admin.
echo   DingTalk names are dynamic-mapped: if a chat shows a numeric id, open it in DingTalk then collect again.
goto :end

rem ---------- subroutines ----------
:startone
if /i "%~1"=="wechat"   ( start "imc-wechat" /min "%R%\bin\imc-wechat.exe" watch --interval 30 & echo started wechat & goto :eof )
if /i "%~1"=="dingtalk" ( start "imc-dingtalk" /min "%R%\bin\imc-dingtalk.exe" watch --interval 60 & echo started dingtalk & goto :eof )
if /i "%~1"=="web"      goto :startweb
echo unknown service: %~1
goto :eof
:startweb
set "PY="
where python >nul 2>&1 && set "PY=python"
if not defined PY ( where py >nul 2>&1 && set "PY=py" )
if not defined PY ( echo [web] skipped: Python 3 not found & goto :eof )
start "imc-web" /min cmd /c "%PY% ""%R%\web\serve.py"""
echo started web (http://127.0.0.1:8899)
goto :eof

:stopone
if /i "%~1"=="wechat"   ( taskkill /f /im imc-wechat.exe   >nul 2>&1 & echo stopped wechat   & goto :eof )
if /i "%~1"=="dingtalk" ( taskkill /f /im imc-dingtalk.exe >nul 2>&1 & echo stopped dingtalk & goto :eof )
if /i "%~1"=="web"      ( taskkill /f /fi "WINDOWTITLE eq imc-web" >nul 2>&1 & echo stopped web & goto :eof )
echo unknown service: %~1
goto :eof

:st
tasklist /fi "imagename eq %~2" 2>nul | find /i "%~2" >nul && (echo %~1    running) || (echo %~1    stopped)
goto :eof

:end
endlocal
