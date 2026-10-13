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

## 跨平台(Windows)
进程内存访问层已抽象为跨平台(`common::mem`):Linux 走 `/proc/<pid>/{maps,mem}`,Windows 走
`CreateToolhelp32Snapshot` + `VirtualQueryEx` + `ReadProcessMemory`(`#[cfg]` 分发,见 `src/common/src/lib.rs`)。
其余(SQLCipher 解密 / SQLite 叶子页解析 / 存储 / MCP)本就跨平台。

Windows 构建与运行:
```
# 需 Rust + MSVC 生成工具(VS Build Tools 的 C++ 工作负载; rusqlite bundled 要编 C)
cd src && cargo build --release          # 产物 src\target\release\imc-*.exe
# 需提权: 读取其他进程内存要管理员 / SeDebugPrivilege(相当于 Linux 的 ptrace_scope=0)
imc-wechat.exe collect
imc-dingtalk.exe collect
```
平台差异(进程名/路径,已在代码中按 `#[cfg]` 处理):
- 进程定位:Linux 匹配 cmdline(钉钉 `com.alibabainc.dingtalk`、微信 `wechat`);
  Windows 匹配映像名(`DingTalk.exe`;微信 4.0 `Weixin.exe`,3.x `WeChat.exe`)。
- 数据目录 / 微信库目录:`home()` 在 Windows 回退 `USERPROFILE`;数据默认 `~/.local/share/im-collector`,
  可用 `IMC_DATA` 覆盖(建议 Windows 设 `%LOCALAPPDATA%\im-collector`)。
- 微信库布局沿用 `Documents\xwechat_files\<wxid>\db_storage`(与 Linux 4.0 客户端一致)。

> 未在真机验证:Windows 客户端的内存布局 / 密钥字面量格式 / 库文件路径可能与 Linux 版不同,
> 首次在 Windows 跑需实测核对扫描特征(见 docs/reverse-*.md)。systemd 单元为 Linux 专用;
> Windows 常驻用「计划任务」或 NSSM 包装服务。
