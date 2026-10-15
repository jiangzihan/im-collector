#!/usr/bin/env bash
# im-collector 统一管理脚本: 一条命令管理 微信/钉钉/web/mcp 服务(systemd --user)。
# 用法:
#   ./imc.sh start   [wechat|dingtalk|web|mcp ...]  # 省略 = 全部(mcp 需装 uv)
#   ./imc.sh stop    [服务名 ...]
#   ./imc.sh restart [服务名 ...]
#   ./imc.sh status                              # 运行/自启概览
#   ./imc.sh logs    [wechat|dingtalk|web|mcp] [N]  # 跟踪日志(默认 wechat)
#   ./imc.sh enable  [服务名 ...]                # 开机自启(含 enable-linger)
#   ./imc.sh disable [服务名 ...]                # 取消开机自启
#   ./imc.sh keyscan [step]                      # 微信: 从内存找数据库密钥(一次性, 约数分钟)
#   ./imc.sh mcp                                 # 打印把 MCP 配到各 agent 的配置片段
#   ./imc.sh install                             # 仅从 deploy/ 重装/刷新 systemd 单元
# 说明: mcp 服务以 SSE 常驻 127.0.0.1:8900, 供 Claude Code/Codex/WorkBuddy 等按 URL 共享;
#       也可用 stdio 命令式各 agent 自行拉起(见 ./imc.sh mcp)。
set -uo pipefail

R="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
UV="$(command -v uv 2>/dev/null || true)"   # mcp 服务用 uv 拉起(自动装 mcp 依赖)
ALL=(wechat dingtalk web)
[ -n "$UV" ] && ALL+=(mcp)                   # 有 uv 才纳入 mcp 服务
UNITS=()

die(){ echo "错误: $*" >&2; exit 1; }
usage(){ awk 'NR>=2{ if(/^#/){sub(/^# ?/,"");print} else exit }' "$0"; }

have_user_bus(){ systemctl --user show-environment >/dev/null 2>&1; }

render_unit(){ # $1=服务名: deploy/ 里的 %h->$HOME, __UV__->uv 绝对路径, 写入 UNIT_DIR
  local n="$1" src="$R/deploy/imc-$1.service" dst="$UNIT_DIR/imc-$1.service"
  [ -f "$src" ] || die "缺少单元源文件 $src"
  mkdir -p "$UNIT_DIR"; sed -e "s#%h#$HOME#g" -e "s#__UV__#${UV}#g" "$src" > "$dst"
}

ensure_installed(){ # 单元缺失或 deploy 源更新时重新渲染并 daemon-reload
  local changed=0 n src dst
  for n in "${ALL[@]}"; do
    src="$R/deploy/imc-$n.service"; dst="$UNIT_DIR/imc-$n.service"
    if [ ! -f "$dst" ] || [ "$src" -nt "$dst" ]; then render_unit "$n"; changed=1; fi
  done
  [ "$changed" = 1 ] && systemctl --user daemon-reload
  return 0
}

resolve_units(){ # 服务名 -> UNITS[]; 无参=全部; 未知名报错返回 1
  UNITS=(); local a; local list=("$@")
  [ "${#list[@]}" -eq 0 ] && list=("${ALL[@]}")
  for a in "${list[@]}"; do
    case "$a" in
      wechat|dingtalk|web) UNITS+=("imc-$a.service");;
      mcp) [ -n "$UV" ] || { echo "错误: mcp 服务需要 uv(未找到)。装 uv 或改用 stdio 命令式(见 ./imc.sh mcp)" >&2; return 1; }
           UNITS+=("imc-mcp.service");;
      *) echo "错误: 未知服务 '$a' (可选: wechat dingtalk web mcp)" >&2; return 1;;
    esac
  done
}

print_mcp_config(){
  cat <<EOF
把 im-collector MCP 配到任意 agent(Claude Code / Codex / WorkBuddy 等)。二选一传输:

=== A. stdio(命令式, 最通用; agent 自行拉起, 无需先 start) ===
通用 mcp.json(很多客户端用这种 "mcpServers" 结构):
{
  "mcpServers": {
    "im-collector": {
      "command": "uv",
      "args": ["run","--with","mcp<2","--directory","$R/mcp","python","$R/mcp/server.py"]
    }
  }
}
Claude Code:  claude mcp add im-collector -- uv run --with "mcp<2" --directory "$R/mcp" python "$R/mcp/server.py"
Codex(~/.codex/config.toml 追加):
  [mcp_servers.im-collector]
  command = "uv"
  args = ["run","--with","mcp<2","--directory","$R/mcp","python","$R/mcp/server.py"]

=== B. SSE(共享常驻; 先 ./imc.sh start 让 MCP 跑在 127.0.0.1:8900) ===
URL: http://127.0.0.1:8900/sse
通用 mcp.json: { "mcpServers": { "im-collector": { "url": "http://127.0.0.1:8900/sse" } } }
Claude Code:  claude mcp add --transport sse im-collector http://127.0.0.1:8900/sse
(未装 uv 时 stdio 方式把 "uv run --with mcp<2 --directory X python" 换成已装好 mcp 的 "python"。)
EOF
}

in_units(){ local u; for u in "${UNITS[@]}"; do [ "$u" = "$1" ] && return 0; done; return 1; }

ensure_ptrace(){ # 钉钉内存取证需 kernel.yama.ptrace_scope=0; 临时设置(随启动, 重启后失效)
  local f=/proc/sys/kernel/yama/ptrace_scope cur
  [ -r "$f" ] || return 0            # 无 Yama 模块: 默认即可读进程内存, 无需处理
  cur="$(cat "$f" 2>/dev/null || echo 0)"
  [ "$cur" = 0 ] && return 0
  echo "钉钉需 kernel.yama.ptrace_scope=0 (当前 $cur), 临时设置(可能需输入 sudo 密码)..."
  sudo sysctl -w kernel.yama.ptrace_scope=0 >/dev/null 2>&1
  cur="$(cat "$f" 2>/dev/null || echo 1)"
  if [ "$cur" != 0 ]; then
    echo "  !! 仍为 $cur, 钉钉将无法采集。请手动执行: sudo sysctl -w kernel.yama.ptrace_scope=0" >&2
    return 1
  fi
  echo "  已临时设为 0(重启后失效)。"
  return 0
}

cmd="${1:-}"; shift 2>/dev/null || true

case "$cmd" in
  start|stop|restart|enable|disable|status|logs|install) ;;
  mcp) print_mcp_config; exit 0;;
  keyscan) [ -x "$R/bin/imc-wechat" ] || die "缺少 $R/bin/imc-wechat(先 ./build.sh)"
           ensure_ptrace || true; exec "$R/bin/imc-wechat" keyscan "${1:-8}";;
  ""|help|-h|--help) usage; exit 0;;
  *) echo "未知命令 '$cmd'" >&2; usage; exit 2;;
esac

have_user_bus || die "systemd --user 不可用(需在有用户总线的登录会话里运行)"

case "$cmd" in
  start)
    ensure_installed; resolve_units "$@" || exit 2
    in_units imc-dingtalk.service && { ensure_ptrace || true; }
    systemctl --user start "${UNITS[@]}" && echo "已启动: ${UNITS[*]}" ;;
  stop)
    resolve_units "$@" || exit 2
    systemctl --user stop "${UNITS[@]}" && echo "已停止: ${UNITS[*]}" ;;
  restart)
    ensure_installed; resolve_units "$@" || exit 2
    in_units imc-dingtalk.service && { ensure_ptrace || true; }
    systemctl --user restart "${UNITS[@]}" && echo "已重启: ${UNITS[*]}" ;;
  enable)
    ensure_installed; resolve_units "$@" || exit 2
    in_units imc-dingtalk.service && { ensure_ptrace || true; }
    loginctl enable-linger "$USER" 2>/dev/null || true
    systemctl --user enable --now "${UNITS[@]}" && echo "已设开机自启: ${UNITS[*]}" ;;
  disable)
    resolve_units "$@" || exit 2
    systemctl --user disable --now "${UNITS[@]}" && echo "已取消开机自启: ${UNITS[*]}" ;;
  status)
    ensure_installed
    printf "%-10s %-12s %-10s %s\n" 服务 运行 自启 单元
    for n in "${ALL[@]}"; do
      u="imc-$n.service"
      printf "%-10s %-12s %-10s %s\n" "$n" \
        "$(systemctl --user is-active  "$u" 2>/dev/null || true)" \
        "$(systemctl --user is-enabled "$u" 2>/dev/null || true)" "$u"
    done ;;
  logs)
    svc=wechat
    if [ "${1:-}" ]; then
      case "$1" in wechat|dingtalk|web|mcp) svc="$1"; shift;; esac
    fi
    exec journalctl --user -u "imc-$svc.service" -n "${1:-80}" -f ;;
  install)
    for n in "${ALL[@]}"; do render_unit "$n"; done
    systemctl --user daemon-reload
    echo "单元已安装/刷新到 $UNIT_DIR" ;;
esac
