# im-collector
多 IM(微信/钉钉) 聊天记录采集 → 统一 SQLite → MCP 分析。详见 docs/00-overview.md。
快速: `sudo sysctl kernel.yama.ptrace_scope=0` → `./build.sh` → `bin/imc-wechat collect` / `bin/imc-dingtalk collect` → 注册 MCP。
数据在 `~/.local/share/im-collector/`(DB/keys/解密库, 敏感, 不在源码树内; 可用 `IMC_DATA` 覆盖)。逆向过程见 docs/reverse-*.md 与 reverse/。
