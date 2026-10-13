# im-collector
多 IM(微信/钉钉) 聊天记录采集 → 统一 SQLite → MCP 分析。详见 docs/00-overview.md。
快速(Linux): `sudo sysctl kernel.yama.ptrace_scope=0` → `./build.sh` → `./imc.sh start`(一条命令管理 微信/钉钉/web, 另有 stop/restart/status/logs) → 注册 MCP。
Windows: 装 Rust + MSVC 生成工具 → `cd src && cargo build --release` → 以管理员运行 `imc-*.exe collect`(进程内存访问层已跨平台,见 docs/00-overview.md「跨平台(Windows)」)。
数据在 `~/.local/share/im-collector/`(DB/keys/解密库, 敏感, 不在源码树内; 可用 `IMC_DATA` 覆盖)。逆向过程见 docs/reverse-*.md 与 reverse/。
