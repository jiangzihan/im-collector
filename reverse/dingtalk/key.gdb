set pagination off
set confirm off
set detach-on-fork on
set follow-fork-mode parent
set breakpoint pending on
set logging file /home/fido/wxtools/data/dt_capture/gdbkey.log
set logging overwrite on
set logging redirect on
set logging on

# sqlite3CodecAttach(db, nDb, pKey, nKey): rdi,rsi,rdx,rcx  <- 真正的codec密钥
break sqlite3CodecAttach
commands
silent
printf "=== CodecAttach nKey=%d ===\n", $rcx
eval "x/%dxb $rdx", $rcx
continue
end

# sqlite3_key_interop(db, pKey, nKey): rdi,rsi,rdx
break sqlite3_key_interop
commands
silent
printf "=== key_interop nKey=%d ===\n", $rdx
eval "x/%dxb $rsi", $rdx
continue
end

break sqlite3_activate_see
commands
silent
printf "=== activate_see license=\"%s\" ===\n", (char*)$rdi
continue
end

break _ZN9SQLiteCpp8Database19openDatabaseWithKeyERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEE
commands
silent
set $p=*(char**)$rsi
set $l=*(long*)($rsi+8)
printf "=== openDatabaseWithKey keylen=%ld ===\n", $l
eval "x/%dxb $p", $l
continue
end

run
