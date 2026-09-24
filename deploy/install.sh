#!/usr/bin/env bash
# 部署 im-collector。默认只安装不启用开机自启。
# 用法: deploy/install.sh [--build] [--enable wechat|dingtalk|all] [--register-mcp]
set -euo pipefail
R="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD=0 ENABLE="" MCP=0
while [ $# -gt 0 ]; do case "$1" in
  --build) BUILD=1;; --enable) ENABLE="$2"; shift;; --register-mcp) MCP=1;; *) echo "未知参数 $1"; exit 2;; esac; shift; done
command -v uv >/dev/null || echo "  ! 缺 uv (MCP 需要)"
if [ "$BUILD" = 1 ] || [ ! -x "$R/bin/imc-wechat" ]; then command -v cargo >/dev/null || { echo "需要 Rust/cargo"; exit 1; }; "$R/build.sh"; fi
mkdir -p "$HOME/.config/systemd/user"
sed "s#%h#$HOME#g" "$R/deploy/imc-wechat.service"  > "$HOME/.config/systemd/user/imc-wechat.service"
sed "s#%h#$HOME#g" "$R/deploy/imc-dingtalk.service" > "$HOME/.config/systemd/user/imc-dingtalk.service"
systemctl --user daemon-reload 2>/dev/null || echo "  (无 systemd --user, 跳过)"
echo "权限(取密钥/取证需要): sudo cp '$R/deploy/99-imc-ptrace.conf' /etc/sysctl.d/ && sudo sysctl --system"
if [ "$MCP" = 1 ]; then
  claude mcp add im-collector --scope user -- uv run --with 'mcp<2' --directory "$R/mcp" python "$R/mcp/server.py"
  echo "  已注册 MCP im-collector"
else
  echo "手动注册 MCP: claude mcp add im-collector --scope user -- uv run --with 'mcp<2' --directory '$R/mcp' python '$R/mcp/server.py'"
fi
enable_one(){ systemctl --user enable --now "imc-$1.service"; echo "  已启用 imc-$1"; }
case "$ENABLE" in
  all) loginctl enable-linger "$USER" 2>/dev/null||true; enable_one wechat; enable_one dingtalk; enable_one web;;
  wechat|dingtalk) loginctl enable-linger "$USER" 2>/dev/null||true; enable_one "$ENABLE";;
  "") echo ">> 未启用开机自启。手动: loginctl enable-linger \$USER && systemctl --user enable --now imc-wechat.service imc-dingtalk.service";;
esac
echo "完成。"
