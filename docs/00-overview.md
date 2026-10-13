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

## 前置 & 启动
```
sudo cp deploy/99-imc-ptrace.conf /etc/sysctl.d/ && sudo sysctl --system   # 一次
deploy/install.sh --build --register-mcp                                    # 部署(不自动开机启动)
# 开机常驻(确认后): loginctl enable-linger $USER && systemctl --user enable --now imc-wechat imc-dingtalk
```
各客户端需登录运行(数据在其内存/文件)。详见 docs/reverse-*.md。
