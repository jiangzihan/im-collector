# 钉钉(Linux) 逆向核心过程

目标：从本地恢复钉钉聊天记录。环境：`com.alibabainc.dingtalk` 8.1.0-Release，Linux，Electron/NW.js。
数据目录：`~/.config/DingTalk/<id>_v3/`，DB 在 `DBFiles/*.db`、`UserKms/*.db` 等。

## 1. 判定：不是 SQLCipher，是 SEE
- 库文件头是固定 magic：`56 59 BB BD F5 CF C5 CE DE 3F 4D 2D B7 B6 AB F6`（所有库一致，非 SQLCipher 的随机 salt）。
- 进程加载 **系统 `libsqlite3.so`（被 Chromium/NSS 用）** + **`libcrypto/libssl 1.1`**。
- `libutforpc.so` 导出 `sqlite3_activate_see`、`sqlite3CodecAttach`、`sqlite3_key`、`sqlite3CodecGetKey`
  → 确认是 **SEE（SQLite Encryption Extension）**，且 sqlite+codec **静态编入 libutforpc**。
- 页级熵分析：空页(全0)加密后 = 16 固定字节 + 后续 0；数据页高熵 → AES 类分组加密，密钥未知。

## 2. LD_PRELOAD 拦截为何失败（重要教训）
逐一尝试并排除：
- `libahencrypt_sdk.so::decryptData/decryptBuffer`：能拦到，但只处理**小字段**(16B)，非 DB 页。
- OpenSSL `AES_set_decrypt_key`/`EVP_DecryptInit_ex`：拦了会**让 app 的 `Util::Encry::decryptDataAES` 断言 abort**（干扰其 AES），且非 DB 页路径。
- 系统 `libsqlite3::sqlite3_step`：只拦到 **Chromium 的 cert9.db**；钉钉消息库不走系统 libsqlite3。
- `sqlite3_key`/`sqlite3_key_v2`：**零命中** → app 不用该 C API 设密钥。
- 根因：**libutforpc 静态内联 SEE-sqlite 且 `-Bsymbolic` 内部自绑定**，其内部 `sqlite3_step`/codec 调用不经全局符号表 → LD_PRELOAD 无法介入 DB 路径。
  （对微信 `decryptDataAES` 等**跨模块**符号有效；钉钉消息路径全内部。）

## 3. gdb 取口令（成功）
- ptrace 放开：`sudo sysctl kernel.yama.ptrace_scope=0`。
- 在 `dingtalk_dll.so` 导出的 `SQLiteCpp::Database::openDatabaseWithKey(const std::string&)`（偏移 `+0x5c773f0`）下断点，
  从 `rsi`(std::string&) 读出 **DB 口令 = `<机器相关口令-gdb现取>`**（16 字节 ASCII）。
- 反汇编该函数(237B)：`call +0x4b4b145(db, key, 16)` 装密钥（**内部函数**，把 16B 口令加工成 ~180B key context），
  随后仅两条 `PRAGMA journal_mode=WAL;` 等——**cipher 配置全在内部**，不走 `sqlite3_key`/`PRAGMA key`。
- 结论：拿到口令也**无法用 libutforpc 公开 API 打开**（`sqlite3_key`+口令→ rc=26 "encrypted"）；
  离线解密需**逆向内部 codec（key schedule + 页加密模式）**，工程量大，未做。

## 4. 内存取证（最终采用，成功）
- 关键事实：钉钉运行时把库**解密在进程页缓存**里。`/proc/<pid>/mem` 扫到大量明文 SQLite（`CREATE TABLE ...`、页头 `SQLite format 3`）。
- 消息表：**`tbmsg_<n>`**（分片消息）、`tbconversation`（会话）、`tblastmsg`。
- 方法：扫可读内存找 **SQLite 叶子表页（首字节 `0x0D`）** → 解析 cell/record（varint + serial types）→
  取含"13 位毫秒时间戳 + 文本正文"的记录 = 聊天消息。
- 消息行结构（实测, 叶子记录字段序）：
  `[∅, 发送者uid(Int), cid(Text), 正文(Text), @映射json(Text), 类型(Int), "1"/"2", 0, ts_ms(Int)]`
  cid 两种形态：群聊=纯数字 openConversationId；单聊=`uidA:uidB`。据此判会话类型(is_group)。
- 名称相关结构（与消息分开存, 按 id）：
  - 会话标题行 `NTTTII = [∅, 群cid(Text), 群名(Text), "", ≈2(Int), ts]`（每群唯一）→ `cid→群名`。
  - 联系人/成员行 `NIT = [∅, uid, 名字]`、名册 JSON `[{"uId":"..","nick":".."}]`（注意 **uId 在 nick 前**）、
    @提及 JSON `{"uid":"名(别名)"}` → `uid→人名`。
  - 账号自身 `login_history_key`/`DTLastUserData` 里 `"user_id"`/`nick` → 当前用户 uid（用于单聊取"对方"）。
- 名称是**动态映射**：消息默认只有 id，需用上面结构建 `id→名称` 表，落库时写入已映射名并持久化
  (`names` 表累积)，未映射先按 id 显示，解析出映射后批量回填历史。详见 docs/00-overview.md「名称解析」。
- 局限：只得**当前缓存**（近期/活跃会话），非全量历史；某会话的标题/成员只在钉钉把它
  加载进内存时才出现 → 打开该会话再采即可补全映射（见 overview「补全钉钉群名/发送者」）。

## 5. 复现命令（速查）
```
sudo sysctl kernel.yama.ptrace_scope=0            # 一次
# 取口令(可选, 需要离线解密时): reverse/dingtalk/run_gdb.sh  (断点 openDatabaseWithKey)
# 取消息(常用): dingtalk collect   → 内存取证入库 (见 bin)
```
偏移/符号随版本变化，升级后用 `objdump -T libutforpc.so | grep sqlite3_` 与 `objdump -d dingtalk_dll.so` 复核。
