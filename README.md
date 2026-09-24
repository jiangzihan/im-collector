# im-collector
多 IM(微信/钉钉) 聊天记录采集 → 统一 SQLite → MCP 分析。详见 docs/00-overview.md。
快速: `sudo sysctl kernel.yama.ptrace_scope=0` → `./build.sh` → `bin/imc-wechat collect` / `bin/imc-dingtalk collect` → 注册 MCP。
数据在 data/imstore.db(敏感)。逆向过程见 docs/reverse-*.md 与 reverse/。
