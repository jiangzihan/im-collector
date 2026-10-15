# im-collector
多 IM(微信/钉钉) 聊天记录采集 → 统一 SQLite → MCP 分析。详见 docs/00-overview.md。
快速(Linux): 免编译可到 **Releases** 下 `im-collector-linux-x64.tar.gz`(musl 静态, 任何 Linux 可跑); 或自编 `./build.sh`。然后 `sudo sysctl kernel.yama.ptrace_scope=0` → `./imc.sh start`(一条命令管理 微信/钉钉/web, 另有 stop/restart/status/logs) → 注册 MCP。
Windows(推荐,免编译): 到 GitHub **Releases** 下载 `im-collector-windows-x64.zip` → 解压 → 客户端登录着跑 `imc.bat collect`(自动提权)。出版本只需推 `v*` 标签, CI(`.github/workflows/windows-build.yml`)在 windows-latest 原生编译并发 Release。想本地自编: 装 Rust + MSVC → `build.bat`。详见 docs/00-overview.md「跨平台(Windows)」。
数据在 `~/.local/share/im-collector/`(DB/keys/解密库, 敏感, 不在源码树内; 可用 `IMC_DATA` 覆盖)。逆向过程见 docs/reverse-*.md 与 reverse/。
名称解析: 微信落库即带现成昵称/群名; 钉钉默认只有数字 id, 需动态映射(有映射即入库并回填历史, 无映射先显示 id)。某会话仍显示 id 时, 在钉钉打开该会话 → 网页点「立即采集」即可补全。详见 docs/00-overview.md「名称解析」与「补全钉钉群名/发送者」。
