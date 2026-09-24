#!/usr/bin/env bash
set -u
APPDIR=/opt/apps/com.alibabainc.dingtalk/files/8.1.0-Release.6021101
LOG=$HOME/wxtools/data/dt_capture/gdbkey.log
pkill -f com.alibabainc.dingtalk 2>/dev/null; sleep 2
: > "$LOG"
export QT_QPA_PLATFORM=xcb QT_PLUGIN_PATH="$APPDIR" DISPLAY="${DISPLAY:-:0.0}"
export LD_PRELOAD="$APPDIR/libgbm.so $APPDIR/plugins/dtwebview/libcef.so"
cd "$APPDIR" || exit 1
echo "在 gdb 下启动钉钉... 断点日志: $LOG"
gdb -batch -x "$HOME/wxtools/dingtalk/key.gdb" --args ./com.alibabainc.dingtalk
