use imc_common::{crypto, store, md5hex, unzstd, fmt_time, type_label, mem};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

const IM:&str="wechat";
fn home()->String{ std::env::var("HOME").or_else(|_|std::env::var("USERPROFILE")).unwrap_or_else(|_|".".into()) }
// 运行时数据(数据库/密钥/解密库)目录: 不放在源码树内, 默认 home 共享目录 ~/.local/share/im-collector
// 覆盖顺序: $IMC_DATA > $XDG_DATA_HOME/im-collector > ~/.local/share/im-collector
fn data()->String{
    let d = std::env::var("IMC_DATA").ok()
        .or_else(|| std::env::var("XDG_DATA_HOME").ok().filter(|s|!s.is_empty()).map(|x|format!("{}/im-collector",x)))
        .unwrap_or_else(|| format!("{}/.local/share/im-collector",home()));
    std::fs::create_dir_all(&d).ok();
    d
}

fn find_src()->Option<String>{
    // 候选根目录: IMC_WECHAT_DIR 覆盖 > Documents > OneDrive\Documents > home 下 xwechat_files。
    // Windows 的"文档"可能被 OneDrive 重定向, 或微信自定义了存储路径, 故多处探测 + 可直接指定。
    let h=home();
    let mut bases:Vec<String>=Vec::new();
    if let Ok(d)=std::env::var("IMC_WECHAT_DIR"){ if !d.is_empty(){ bases.push(d.clone()); bases.push(format!("{d}/xwechat_files")); } }
    bases.push(format!("{h}/Documents/xwechat_files"));
    bases.push(format!("{h}/OneDrive/Documents/xwechat_files"));
    bases.push(format!("{h}/xwechat_files"));
    for base in &bases {
        // base 本身可能就是某账号目录(直接含 db_storage)
        if Path::new(&format!("{base}/db_storage")).is_dir(){ return Some(format!("{base}/db_storage")); }
        // 否则 base 是 xwechat_files, 遍历其下各账号目录
        if let Ok(rd)=std::fs::read_dir(base){
            for e in rd.flatten(){
                let p=format!("{}/db_storage",e.path().display());
                if Path::new(&p).is_dir(){ return Some(p); }
            }
        }
    }
    None
}
// Linux: cmdline 含 "wechat"; Windows: 映像名 Weixin.exe(4.0) 或 WeChat.exe(3.x)
#[cfg(unix)]
fn wechat_pid()->Option<String>{ mem::pid_by_cmd("wechat") }
#[cfg(windows)]
fn wechat_pid()->Option<String>{ mem::pid_by_cmd("Weixin").or_else(||mem::pid_by_cmd("WeChat")) }

// 从字符串解析出若干 64-hex 密钥(空白/逗号分隔)
fn parse_hex_keys(s:&str)->Vec<Vec<u8>>{
    s.split(|c:char| c.is_whitespace()||c==',').filter_map(|t|{let t=t.trim(); if t.len()==64 {hex::decode(t).ok()} else {None}}).collect()
}
fn get_keys(force:bool)->Result<Vec<Vec<u8>>,String>{
    // 1) 环境变量直供(Windows 等无法自动扫描时, 用外部工具拿到密钥后设 IMC_WECHAT_KEY=<64hex>)
    if let Ok(k)=std::env::var("IMC_WECHAT_KEY"){ let v=parse_hex_keys(&k); if !v.is_empty(){return Ok(v);} }
    let kf=format!("{}/wechat/keys.txt",data());
    // 2) 已保存的 keys.txt(keyscan/历史扫描写入的)
    if !force { if let Ok(k)=crypto::load_keys(&kf){ if !k.is_empty(){return Ok(k);} } }
    // 3) Linux: 扫内存里的 x'<hex>' 字面量
    let pid=wechat_pid().ok_or("微信未运行")?;
    let keys=crypto::scan_raw_keys(&pid)?;
    std::fs::create_dir_all(format!("{}/wechat",data())).ok();
    let mut s=String::new(); for k in &keys{ s.push_str(&hex::encode(k)); s.push('\n'); }
    std::fs::write(&kf,s).ok();
    Ok(keys.into_iter().map(|a|a.to_vec()).collect())
}
// 深度取密钥(跨平台): 读定位到的加密库首页, 在进程内存里逐候选校验, 命中则写入 keys.txt
fn keyscan(step:usize)->Result<(),String>{
    let src=find_src().ok_or("找不到 db_storage(可设 IMC_WECHAT_DIR 指向 xwechat_files)")?;
    let dbp=format!("{}/message/message_0.db",src);
    let mut f=std::fs::File::open(&dbp).map_err(|e|format!("open {dbp}: {e}"))?;
    use std::io::Read; let mut p1=vec![0u8;4096]; f.read_exact(&mut p1).map_err(|e|e.to_string())?;
    if &p1[..15]==b"SQLite format 3"{ return Err("该库已是明文/未加密, 无需密钥".into()); }
    // 诊断: 打印库与首页 salt(应为非零随机值; 全零=异常)
    eprintln!("[wechat] 库: {dbp}");
    eprintln!("[wechat] 首页 salt(前16字节): {}", hex::encode(&p1[..16]));
    // 枚举所有微信进程(Linux: cmdline 含 wechat; Windows: 映像名 Weixin/WeChat), 逐个扫描
    #[cfg(unix)] let needles:&[&str]=&["wechat"];
    #[cfg(windows)] let needles:&[&str]=&["Weixin","WeChat"];
    let mut procs:Vec<(String,String,u64)>=Vec::new();
    for nd in needles { for p in mem::pids_by_cmd(nd){ if !procs.iter().any(|(pid,_,_)|*pid==p.0){ procs.push(p); } } }
    if procs.is_empty(){ return Err("微信未运行(未找到匹配进程)".into()); }
    eprintln!("[wechat] 匹配到 {} 个微信进程, 逐个扫描(step={step}, 多线程, 私有堆→全部可写):", procs.len());
    let kf=format!("{}/wechat/keys.txt",data());
    for (pid,name,ws_kb) in &procs {
        eprintln!("[wechat] -> 进程 pid={pid} {name} 内存≈{}MB", ws_kb/1024);
        if let Some(k)=crypto::find_key_validate(pid,&p1,step)?{
            std::fs::create_dir_all(format!("{}/wechat",data())).ok();
            std::fs::write(&kf, format!("{}\n",hex::encode(k))).ok();
            println!("{}", hex::encode(k));
            eprintln!("[wechat] 命中密钥(pid={pid}), 已写入 {kf} (之后 collect 直接可用)");
            return Ok(());
        }
    }
    Err(format!("在 {} 个微信进程里都没找到可校验密钥(step={step})。\n  排查:\n  1) 上面 salt 若全为 0 或库大小异常, 说明该库未真正加密/未初始化, 换个账号或等微信完全登录。\n  2) step>1 可能未对齐, 试 `imc-wechat keyscan 2`。\n  3) 若确属本机微信版本的密钥格式差异, 用 PyWxDump / wechat-dump-rs 取到 64hex 后: set IMC_WECHAT_KEY=<64hex> 再 collect(最稳)。\n  把上面几行诊断(salt、各进程名与内存)发给作者可进一步定位。", procs.len()))
}

fn collect()->Result<(),String>{
    let src=find_src().ok_or("找不到 db_storage")?;
    let dec=format!("{}/wechat/decrypted",data());
    std::fs::create_dir_all(&dec).ok();
    let mut keys=get_keys(false)?;
    let mo_out=format!("{}/message_0.db",dec);
    let ct_out=format!("{}/contact.db",dec);
    let dec_one=|keys:&Vec<Vec<u8>>,rel:&str,out:&str|->Result<(),String>{
        crypto::decrypt_db(keys,&format!("{}/{}",src,rel),out)
    };
    // 解密主库+联系人; 失败则重扫密钥
    if dec_one(&keys,"message/message_0.db",&mo_out).is_err(){
        keys=get_keys(true)?;
        if let Err(e)=dec_one(&keys,"message/message_0.db",&mo_out){
            return Err(format!("{e}\n  取不到有效密钥(Windows 微信密钥是原始字节, 不以 x'..' 字面量出现, 普通扫描拿不到)。\n  办法: 1) imc-wechat keyscan   (在内存里自动找密钥并缓存, 一次性约数分钟, 之后 collect/watch 直接可用) —— 推荐, 自带\n        2) 或用 PyWxDump / wechat-dump-rs 取到 64hex 后: set IMC_WECHAT_KEY=<64hex> 再 collect (更快)"));
        }
    }
    let _=dec_one(&keys,"contact/contact.db",&ct_out);

    let cdb=Connection::open_with_flags(&ct_out,OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
    let mdb=Connection::open_with_flags(&mo_out,OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
    // 名称表
    let mut disp:HashMap<String,String>=HashMap::new();
    let mut users:Vec<String>=Vec::new();
    {
        let mut st=cdb.prepare("SELECT username,COALESCE(NULLIF(remark,''),''),COALESCE(NULLIF(nick_name,''),'') FROM contact").map_err(|e|e.to_string())?;
        let rows=st.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
        for row in rows.flatten(){let(u,rm,nk)=row;let dn=if !rm.is_empty(){rm}else if !nk.is_empty(){nk}else{u.clone()};disp.insert(u.clone(),dn);users.push(u);}
    }
    let mut roommem:HashMap<String,String>=HashMap::new();
    if let Ok(mut st)=cdb.prepare("SELECT room_username,member_username,COALESCE(NULLIF(display_name,''),'') FROM chatroom_member"){
        if let Ok(rows)=st.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))){
            for row in rows.flatten(){let(rm,mu,dn)=row;if !dn.is_empty(){roommem.insert(format!("{rm}|{mu}"),dn);}}
        }
    }
    let mut exist=std::collections::HashSet::new();
    { let mut st=mdb.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'Msg_%'").map_err(|e|e.to_string())?;
      for n in st.query_map([],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.flatten(){exist.insert(n.trim_start_matches("Msg_").to_string());} }
    let mut n2i:HashMap<i64,String>=HashMap::new();
    { let mut st=mdb.prepare("SELECT rowid,user_name FROM Name2Id").map_err(|e|e.to_string())?;
      for row in st.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?.flatten(){n2i.insert(row.0,row.1);} }

    let mut s=store::Store::open(&format!("{}/imstore.db",data()))?;
    let mut newn=0i64; let mut touched:Vec<(String,String,bool)>=Vec::new();
    {
        let tx=s.begin();
        for u in &users{
            let tbl=md5hex(u); if !exist.contains(&tbl){continue;}
            let gname=disp.get(u).cloned().unwrap_or_else(||u.clone());
            let grp=u.ends_with("@chatroom")||u.ends_with("@openim");
            let last={ // 读游标
                tx.query_row("SELECT v FROM sync_state WHERE im=?1 AND k=?2",rusqlite::params![IM,tbl],|r|r.get::<_,i64>(0)).unwrap_or(0)
            };
            let q=format!("SELECT local_id,local_type,create_time,real_sender_id,message_content FROM Msg_{tbl} WHERE local_id>{last} ORDER BY local_id");
            let mut st=match mdb.prepare(&q){Ok(s)=>s,Err(_)=>continue};
            let rows=st.query_map([],|r|{
                let mc:Option<Vec<u8>>=match r.get::<_,Vec<u8>>(4){Ok(v)=>Some(v),Err(_)=>r.get::<_,String>(4).ok().map(|x|x.into_bytes())};
                Ok((r.get::<_,Option<i64>>(0)?.unwrap_or(0),r.get::<_,Option<i64>>(1)?.unwrap_or(0),r.get::<_,Option<i64>>(2)?.unwrap_or(0),r.get::<_,Option<i64>>(3)?.unwrap_or(0),mc))
            }).map_err(|e|e.to_string())?;
            let mut maxid=last; let mut cnt=0i64;
            for row in rows.flatten(){
                let(lid,lt,ct,sid,mc)=row;
                let mut content=unzstd(&mc.unwrap_or_default());
                let mut wxid=n2i.get(&sid).cloned().unwrap_or_default();
                if grp { if let Some(ix)=content.find('\n'){ if ix>0&&ix<64&&content[..ix].ends_with(':'){let w=content[..ix-1].to_string(); if !w.is_empty(){wxid=w;} content=content[ix+1..].to_string();}}}
                let sname=roommem.get(&format!("{u}|{wxid}")).cloned().or_else(||disp.get(&wxid).cloned()).unwrap_or_else(||wxid.clone());
                let content=content.trim().to_string();
                store::ins_msg(&tx,IM,u,&gname,ct,&fmt_time(ct),&sname,&type_label(lt),&content,&lid.to_string());
                if lid>maxid{maxid=lid;} cnt+=1;
            }
            if cnt>0 { tx.execute("INSERT OR REPLACE INTO sync_state(im,k,v) VALUES(?1,?2,?3)",rusqlite::params![IM,tbl,maxid]).ok();
                       newn+=cnt; touched.push((u.clone(),gname,grp)); }
        }
        for (u,name,grp) in &touched{ store::upsert_chat(&tx,IM,u,name,if *grp{1}else{0}); }
        tx.commit().map_err(|e|e.to_string())?;
    }
    let total:i64=s.c.query_row("SELECT count(*) FROM messages WHERE im=?1",rusqlite::params![IM],|r|r.get(0)).unwrap_or(0);
    eprintln!("[wechat] +{newn} 新消息, 累计 {total}");
    Ok(())
}

fn main(){
    let a:Vec<String>=std::env::args().collect();
    let cmd=a.get(1).map(|s|s.as_str()).unwrap_or("help");
    match cmd {
        "keys"=>{ match get_keys(true){Ok(k)=>{for x in &k{println!("{}",hex::encode(x));} eprintln!("{} 把密钥",k.len());},Err(e)=>{eprintln!("{e}");std::process::exit(1);}} }
        "keyscan"=>{ let step=a.get(2).and_then(|x|x.parse().ok()).unwrap_or(8usize);
            if let Err(e)=keyscan(step){eprintln!("{e}");std::process::exit(1);} }
        "decrypt"=>{ if a.len()!=5{eprintln!("usage: imc-wechat decrypt <keyfile|hex> <in.db> <out.db>");std::process::exit(2);}
            match crypto::load_keys(&a[2]).and_then(|k|crypto::decrypt_db(&k,&a[3],&a[4])){Ok(_)=>eprintln!("OK {}",a[4]),Err(e)=>{eprintln!("{e}");std::process::exit(1);}} }
        "collect"=>{ if let Err(e)=collect(){eprintln!("collect 失败: {e}");std::process::exit(1);} }
        "watch"=>{ let iv:u64=a.iter().position(|x|x=="--interval").and_then(|i|a.get(i+1)).and_then(|x|x.parse().ok()).unwrap_or(30);
            eprintln!("[wechat] watch 每 {iv}s 采集一次 (Ctrl+C 停止)");
            loop{ if let Err(e)=collect(){eprintln!("[wechat] {e}");} std::thread::sleep(std::time::Duration::from_secs(iv)); } }
        _=>{ eprintln!("imc-wechat <keys|keyscan [step]|decrypt|collect|watch [--interval N]>\n  keyscan: 深度扫描密钥(Windows 等自动取不到时用; step 默认8, 可试1更全)\n  或设 IMC_WECHAT_KEY=<64hex> 直接提供密钥; IMC_WECHAT_DIR 指定库目录"); std::process::exit(2); }
    }
}
