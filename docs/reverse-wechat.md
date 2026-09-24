# 微信(Linux 4.x) 逆向要点

- 加密: **标准 SQLCipher4**。库文件头 16B 随机 salt。
- 密钥: 微信 4.x(≤4.1.12) 把**已派生 enc key** 以 ASCII `x'<64hex>...'` 缓存在进程内存, **每库一把**(共约16)。
- 参数: 页 4096, PBKDF2-HMAC-SHA512, HMAC-SHA512(64B), reserve=80。
  - 校验/解密用 **enc key 直用**(不再 256000 轮 PBKDF2); macKey=PBKDF2(encKey, salt^0x3a, 2, 32)。
- 取密钥: 扫 `/proc/<pid>/mem` 找 `x'...'` → 逐把用页1 HMAC 校验(见 common::crypto)。
- 消息: `db_storage/message/message_0.db` 里 `Msg_<md5(username)>`; 群文本=`wxid:\n正文`; 名称经 Name2Id+contact+chatroom_member; 长消息 zstd。
- 实现: `imc-wechat`(keys/decrypt/collect/watch)。与钉钉不同, 微信可**离线解密文件**, 干净稳定。
