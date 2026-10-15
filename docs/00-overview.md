# im-collector 总览

按 IM 类型采集聊天记录 → 统一本地 SQLite(`~/.local/share/im-collector/imstore.db`) → 统一筛选/统计/分析(MCP)。

## 目录(隔离)
- `reverse/`  逆向实验文件(钉钉 LD_PRELOAD 钩子/ gdb 脚本/内存取证原型) —— 与生产实现隔离
- `docs/`     文档(总览 + 各 IM 逆向要点)
- `src/`      Rust 工作区: `common`(共享库) + `wechat` + `dingtalk`(各一个 bin)
- `bin/`      编译产物 imc-wechat / imc-dingtalk
- `mcp/`      MCP 服务(多 IM, 接入 Claude)
- `scripts/`  便捷脚本
- `deploy/`   systemd 用户服务 + install.sh + sysctl + 迁移
- 运行时数据不在源码树内: 默认 `~/.local/share/im-collector/`(DB + 各 IM keys/decrypted), 可用 `IMC_DATA` / `XDG_DATA_HOME` 覆盖 —— 敏感, 不入库

## 共性流水线(所有 IM 抽象为同样阶段, 见 common)
1. acquire  取访问: 微信=内存扫 SQLCipher 原始密钥; 钉钉=进程内存已解密(SEE, 见 reverse-dingtalk)
2. extract  取数:   微信=解密 db 文件; 钉钉=解析进程内存中的 SQLite 叶子页
3. store     归一化: 统一 messages(im,chat_id,chat_name,ts,sender,type,content,src_id) + chats + sync_state
4. watch     监听:   触发→增量 extract→入库(微信按 local_id 游标; 钉钉按 src_id 去重)
5. serve     分析:   MCP 统一查询/搜索/统计

## 名称解析: 微信 vs 钉钉(重要差异)
两家的"会话名 / 发送者名"来源完全不同:

- **微信**: 解密后的本地库里直接带现成的昵称/群名/联系人表, 采集时一次性读出即可落库,
  `chat_name`/`sender` 入库就是最终可读值, 无需后续映射。

- **钉钉**: 内存取证拿到的消息里, 会话和发送者**默认只有数字 id**
  (群 cid=纯数字, 单聊 cid=`uidA:uidB`, 发送者=uid), 名称与 id **分开存在不同结构里**,
  且只在钉钉把该会话/成员"加载进内存"时才短暂出现。因此钉钉采用**动态映射**模型:

  1. 维护一张累积的 `id -> 名称` 映射表(DB `names` 表), 来源:
     - 会话标题行 `NTTTII=[∅,群cid,群名,"",≈2,ts]` → `群cid -> 群名`
     - 联系人/成员行 `NIT=[∅,uid,名字]`、名册 JSON `[{"uId","nick"}]`、@提及 JSON → `uid -> 人名`
     - 当前账号 `user_id` 用于单聊判定"对方"
  2. **有映射的, 必须在落库时写入**: `chat_name`/`sender` 存入已映射的可读名,
     同时把映射持久化到 `names`(累积, 下次直接可用)。
  3. **暂无映射的, 先按 id 显示**(`chat_name`=群cid / 单聊对方uid, `sender`=空),
     不丢数据。
  4. **一旦解析出映射值, 立即回填**: 对历史库里仍是 id / 空发送者的记录,
     用新得到的映射批量 `UPDATE`(见 `store::backfill_sender` + chat_name 回填),
     于是同一会话/人名"解析完成后"就从 id 变为可读名。

  => 钉钉覆盖率随使用自然增长: 你在钉钉里打开越多会话, 内存里出现的标题/名册越全,
     映射表越完整, 历史记录被回填得越多。

## CLI 命令
| 功能 | 命令 |
|------|------|
| 微信 取密钥 | `imc-wechat keys` |
| 微信 解密单库 | `imc-wechat decrypt <keyfile|hex> <in.db> <out.db>` |
| 微信 采集入库 | `imc-wechat collect` |
| 微信 监听 | `imc-wechat watch [--interval 30]` |
| 钉钉 采集入库(内存取证) | `imc-dingtalk collect` |
| 钉钉 监听 | `imc-dingtalk watch [--interval 60]` |
| 钉钉 取口令(可选,gdb) | `reverse/dingtalk/run_gdb.sh` |
| 统一查询 | 通过 MCP `im-collector`: list_ims/list_chats/get_messages/search/refresh |

## 前置 & 启动(Linux)
**免编译下载**: GitHub Actions 产出 x86_64 **musl 静态**二进制(任何 Linux 都能跑, 不挑 glibc),
到 **Releases** 下载 `im-collector-linux-x64.tar.gz`,解压后 `./imc.sh start`(或 `bin/imc-* collect`)。
出版本: 推一个 `v*` 标签即同时触发 Linux + Windows 两条工作流, 产物都挂到同一个 Release。
从源码自编:
```
sudo cp deploy/99-imc-ptrace.conf /etc/sysctl.d/ && sudo sysctl --system   # 一次(取内存需 ptrace_scope=0)
deploy/install.sh --build --register-mcp                                    # 部署(不自动开机启动)
```
各客户端需登录运行(数据在其内存/文件)。详见 docs/reverse-*.md。

### 统一管理(一条命令): `./imc.sh`
封装 systemd --user, 一并管理 微信/钉钉/web 三个服务:
```
./imc.sh start            # 启动全部(省略服务名=全部; 也可 ./imc.sh start wechat web)
./imc.sh stop             # 停止全部
./imc.sh restart          # 重启全部
./imc.sh status           # 运行/自启概览
./imc.sh logs dingtalk    # 跟踪某服务日志
./imc.sh enable           # 开机自启(含 enable-linger)
./imc.sh disable          # 取消开机自启
```
首次或 deploy/*.service 改动后, start/restart/enable/status 会自动把单元重装到
`~/.config/systemd/user/`(无需单独跑 install.sh 的单元步骤)。
启动项含钉钉时, 脚本会自动**临时**设置 `kernel.yama.ptrace_scope=0`(内存取证前置,
会提示输入 sudo 密码, 重启后失效; 仅钉钉需要, 微信走已缓存密钥不读进程内存)。

### 补全钉钉群名 / 发送者(动态映射的操作流程)
钉钉名称靠动态映射(见「名称解析」), 某会话仍显示数字 id, 说明它的标题/成员还没被
加载进内存。补全办法(全程图形界面即可):
1. 在**钉钉客户端**里打开该会话(群聊点进去、滚一下加载历史与成员; 看得到群名/成员即可)。
2. 触发一次采集(二选一):
   - **网页**: 打开 http://127.0.0.1:8899 → 左上选「钉钉」→ 点「**立即采集**」
     (等于后台跑 `imc-dingtalk collect`);
   - **命令**: `./imc.sh restart dingtalk` 让 watch 继续, 或直接 `bin/imc-dingtalk collect`。
3. 这次采集会抓到该会话的标题/成员映射, 落库并**回填历史**: 原先显示 id 的记录变成可读名。
> 映射一旦入库即长期有效(累积在 `names` 表), 之后无需重复操作该会话。
> 重启电脑后 `ptrace_scope` 归 1, 钉钉采集前需再 `./imc.sh restart dingtalk`(会重新临时放开)。

## 跨平台(Windows)
**最省事: 直接下预编译 exe(无需装 Rust/MSVC)。** GitHub Actions 在 `windows-latest` 上原生编译,
到仓库 **Releases** 下载 `im-collector-windows-x64.zip`,解压后客户端登录着跑 `imc.bat collect` 即可。
- 出版本: 推一个 `v*` 标签(`git tag v0.1.0 && git push origin v0.1.0`),CI 自动编译并发 Release;
  或在 GitHub 的 **Actions → windows-build → Run workflow** 手动触发(填版本号发 Release,留空则只在该次运行的
  Artifacts 里下载)。工作流见 `.github/workflows/windows-build.yml`。
- zip 内含: `bin\imc-*.exe` + `imc.bat` + `web\` + `mcp\` + `docs\`(采集即用; web 需 Python, MCP 需 mcp 包)。

想本地自己编(下方)时才需要 Rust+MSVC。进程内存访问层已抽象为跨平台(`common::mem`):Linux 走 `/proc/<pid>/{maps,mem}`,Windows 走
`CreateToolhelp32Snapshot` + `VirtualQueryEx` + `ReadProcessMemory`(`#[cfg]` 分发,见 `src/common/src/lib.rs`)。
其余(SQLCipher 解密 / SQLite 叶子页解析 / 存储 / MCP)本就跨平台。

Windows 构建与运行(用自带的 `build.bat` / `imc.bat`):
```
:: 前置: 安装 Rust(rustup) + VS Build Tools 勾选"使用 C++ 的桌面开发"(rusqlite bundled 要编 C)
build.bat                      :: 编译 release 并拷到 bin\(imc-wechat.exe / imc-dingtalk.exe)
imc.bat start                  :: 后台常驻 微信+钉钉+web(各自最小化窗口)
imc.bat status                 :: 查看各服务运行状态
imc.bat stop                   :: 停止全部(stop/start/restart 都可带 wechat|dingtalk|web)
imc.bat collect [wechat^|dingtalk] :: 前台采集一次(默认两者)
imc.bat watch dingtalk 60      :: 前台常驻监听(关窗即停)
imc.bat web                    :: 前台网页 http://127.0.0.1:8899
imc.bat mcp                    :: 注册 MCP 到 Claude Code
```
`start`/`stop`/`restart`/`status` 是 Windows 下的常驻管理(无 systemd, 用后台最小化窗口 +
`taskkill`/`tasklist` 实现; 开机自启可在「任务计划程序」里挂 `imc.bat start`)。
读取**同用户**的进程内存在 Windows 上**无需管理员**(钉钉/微信与采集器同用户即可)。
个别进程若报拒绝访问(如目标以更高权限运行), 右键「以管理员身份运行」该窗口再跑即可。

### 微信密钥(Windows 必读)
钉钉**不需要密钥**(内存取证直接解析), 开箱即用。微信不同: 要先拿到数据库的 32 字节密钥才能解密。
- **Linux**: 微信以 `x'<hex>'` 字面量把密钥放内存, 采集器自动扫到, 无需手动。
- **Windows**: 微信 4.0 用**原始字节**设密钥, 内存里没有该字面量, **自动扫不到**(表现为
  `no key validates ...`)。需手动取得密钥后提供:
  1. 用成熟的微信 v4 取密钥工具(如开源 PyWxDump / wechat-dump-rs)拿到 64 位十六进制密钥;
  2. 提供给采集器(二选一):
     - 环境变量: `set IMC_WECHAT_KEY=<64hex>` 然后 `imc.bat collect wechat`;
     - 或写入 `%USERPROFILE%\.local\share\im-collector\wechat\keys.txt`(每行一个 64hex)。
- 实验性自动取密钥: `imc-wechat keyscan [step]`(在可写内存里逐候选校验数据库首页)。**很慢**
  (微信 Electron 堆很大, step=8 数分钟未必命中, step=1 更全但更慢), 仅作兜底, 优先用上面的直供方式。
Windows 无 systemd; 开机自启用「任务计划程序」挂 `imc.bat watch`。

Python 依赖(采集不需要 Python, 以下仅用于可选组件):
- **web**(`web/serve.py`): 只用 Python 3 **标准库**, 装好 Python 即可, **无需 pip 安装**。
- **MCP**(`mcp/server.py`): 需要 `mcp` 包。推荐装 [uv](https://docs.astral.sh/uv/), 则 `imc.bat mcp`
  用 `uv run --with "mcp<2"` 自动按需装并注册; 没有 uv 则先 `pip install mcp` 再手动
  `claude mcp add im-collector --scope user -- python <repo>\mcp\server.py`。
平台差异(进程名/路径,已在代码中按 `#[cfg]` 处理):
- 进程定位:Linux 匹配 cmdline(钉钉 `com.alibabainc.dingtalk`、微信 `wechat`);
  Windows 匹配映像名(`DingTalk.exe`;微信 4.0 `Weixin.exe`,3.x `WeChat.exe`)。
- 数据目录 / 微信库目录:`home()` 在 Windows 回退 `USERPROFILE`;数据默认 `~/.local/share/im-collector`,
  可用 `IMC_DATA` 覆盖(建议 Windows 设 `%LOCALAPPDATA%\im-collector`)。
- 微信库目录: 默认探测 `Documents\xwechat_files`、`OneDrive\Documents\xwechat_files`、`%USERPROFILE%\xwechat_files`
  下各账号的 `db_storage`。若微信自定义了存储路径(微信→设置→文件管理 可看到), 用环境变量
  `IMC_WECHAT_DIR` 指向该 `xwechat_files`(或直接指向含 `db_storage` 的账号目录)。报"找不到 db_storage"即是此处未命中。

> 未在真机验证:Windows 客户端的内存布局 / 密钥字面量格式 / 库文件路径可能与 Linux 版不同,
> 首次在 Windows 跑需实测核对扫描特征(见 docs/reverse-*.md)。systemd 单元为 Linux 专用;
> Windows 常驻用「计划任务」或 NSSM 包装服务。
