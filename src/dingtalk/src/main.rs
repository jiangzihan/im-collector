use imc_common::{store, fmt_time, mem, leaf};
use imc_common::leaf::Val;

const IM:&str="dingtalk";
// 毫秒时间戳下界(2020-01-01); 上界在运行时取"当前时间+1天"(见 collect 的 hi)——
// 消息不会来自未来, 用它过滤把二进制垃圾大整数误判成时间戳的情况。
const LO:i64=1577836800000;
fn now_ms_plus_day()->i64{
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d|d.as_millis() as i64 + 86_400_000).unwrap_or(i64::MAX)
}
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

fn cjk(s:&str)->usize{ s.chars().filter(|c|('\u{4e00}'..='\u{9fff}').contains(c)).count() }
fn djb2(s:&str)->u64{ let mut h:u64=5381; for b in s.bytes(){h=h.wrapping_mul(33).wrapping_add(b as u64);} h }

fn clean_name(v:&str)->String{ match v.find('('){Some(i)=>v[..i].trim().to_string(),None=>v.trim().to_string()} }
// 解析 {"uid":"name",...} 形式, 填入 uid->name
fn parse_mentions(s:&str, map:&mut std::collections::HashMap<String,String>){
    let b=s.as_bytes(); let mut i=0;
    while i<b.len(){
        if b[i]==b'"'{
            let ks=i+1; let mut j=ks; while j<b.len()&&b[j]!=b'"'{j+=1;}
            if j>=b.len(){break;}
            let key=&s[ks..j];
            if j+2<b.len() && b[j+1]==b':' && b[j+2]==b'"'{
                let vs=j+3; let mut k=vs; while k<b.len()&&b[k]!=b'"'{k+=1;}
                if k<=b.len(){
                    let val=&s[vs..k.min(s.len())];
                    if !key.is_empty() && key.bytes().all(|c|c.is_ascii_digit()) && !val.is_empty(){
                        map.entry(key.to_string()).or_insert_with(||clean_name(val));
                    }
                    i=k+1; continue;
                }
            }
            i=j+1;
        } else { i+=1; }
    }
}
// 把正文里的 @<uid> 换成 @<姓名>
fn rewrite_mentions(content:&str, map:&std::collections::HashMap<String,String>)->String{
    let b=content.as_bytes(); let mut out=String::with_capacity(content.len()); let mut i=0; let mut last=0;
    while i<b.len(){
        if b[i]==b'@'{
            let sc=i+1; let mut j=sc; while j<b.len()&&b[j].is_ascii_digit(){j+=1;}
            if j>sc { let uid=&content[sc..j]; if let Some(nm)=map.get(uid){ out.push_str(&content[last..i]); out.push('@'); out.push_str(nm); i=j; last=j; continue; } }
        }
        i+=1;
    }
    out.push_str(&content[last..]); out
}

fn good_content(s:&str)->bool{
    if s.len()<2 {return false;}
    if s.starts_with("@lA")||s.starts_with("dingtalk://")||s.starts_with('{')||s.starts_with('[')||s.starts_with("$:"){return false;}
    if s.contains("\"uId\"")||s.contains("\"nick\"")||s.contains("\"mediaId\"")||s.contains("\"emplId\""){return false;}
    if s.chars().all(|c|c.is_ascii_digit()){return false;}
    cjk(s)>=2 || s.contains("http") || (s.contains(' ') && s.len()>6)
}
// 会话id文本: 纯数字(群聊 openConversationId) 或 "uidA:uidB"(单聊)
fn is_cid_text(t:&str)->bool{
    let p:Vec<&str>=t.splitn(2,':').collect();
    match p.len(){
        1=> (4..=18).contains(&t.len()) && t.bytes().all(|b|b.is_ascii_digit()),
        2=> !p[0].is_empty()&&!p[1].is_empty()&&p[0].bytes().all(|b|b.is_ascii_digit())&&p[1].bytes().all(|b|b.is_ascii_digit()),
        _=>false,
    }
}
// 把 Val 当作 uid(>=5位数字)取出
fn as_uid(v:&Val)->String{ match v {
    Val::Int(i)=>{ let s=i.to_string(); if s.len()>=5 {s} else {String::new()} }
    Val::Text(t) if t.bytes().all(|b|b.is_ascii_digit()) && t.len()>=5 => t.clone(),
    _=>String::new(),
}}
// 解码 JSON \uXXXX 转义
fn decode_ju(s:&str)->String{
    if !s.contains("\\u"){ return s.to_string(); }
    let b:Vec<char>=s.chars().collect(); let mut out=String::new(); let mut i=0;
    while i<b.len(){
        if b[i]=='\\' && i+5<b.len() && b[i+1]=='u' {
            let hex:String=b[i+2..i+6].iter().collect();
            if let Ok(cp)=u32::from_str_radix(&hex,16){ if let Some(c)=char::from_u32(cp){ out.push(c); i+=6; continue; } }
        }
        out.push(b[i]); i+=1;
    }
    out
}
fn is_person_name(s:&str)->bool{
    let n=s.chars().count();
    if n<1||n>40 {return false;}
    if s.starts_with('{')||s.starts_with('[')||s.starts_with('@')||s.contains("://"){return false;}
    if s.bytes().all(|b|b.is_ascii_digit()){return false;}
    s.chars().any(|c|('\u{4e00}'..='\u{9fff}').contains(&c)||c.is_ascii_alphabetic())
}
// 取 JSON 片段里 "key": 之后的值(带引号字符串或裸值), 不含外层引号
fn json_val(frag:&str, key:&str)->Option<String>{
    let pat=format!("\"{key}\"");
    let i=frag.find(&pat)?; let rest=frag[i+pat.len()..].trim_start();
    let rest=rest.strip_prefix(':')?.trim_start();
    if let Some(r)=rest.strip_prefix('"'){ let end=r.find('"')?; Some(r[..end].to_string()) }
    else { let end=rest.find(|c:char| c==','||c=='}').unwrap_or(rest.len()); let v=rest[..end].trim(); if v.is_empty(){None}else{Some(v.to_string())} }
}
// 名册 JSON(数组或对象): 按每个 {} 对象内配对 uid<->nick(与字段先后顺序无关)
fn parse_roster_json(s:&str, map:&mut std::collections::HashMap<String,String>){
    for frag in s.split('{'){
        let frag=frag.split('}').next().unwrap_or("");
        let uid=json_val(frag,"uId").or_else(||json_val(frag,"uid")).or_else(||json_val(frag,"userId"));
        let nick=json_val(frag,"nick").or_else(||json_val(frag,"name")).or_else(||json_val(frag,"userNick"));
        if let (Some(uid),Some(nick))=(uid,nick){
            if uid.bytes().all(|b|b.is_ascii_digit()) && uid.len()>=5 {
                let nm=clean_name(&decode_ju(&nick));
                if is_person_name(&nm){ map.entry(uid).or_insert(nm); }
            }
        }
    }
}
fn is_cidmsgid(s:&str)->bool{
    let p:Vec<&str>=s.splitn(2,':').collect();
    p.len()==2 && !p[0].is_empty() && !p[1].is_empty() && p[0].bytes().all(|b|b.is_ascii_digit()) && p[1].bytes().all(|b|b.is_ascii_digit())
}

fn collect()->Result<(),String>{
    // 进程定位: Linux 匹配 cmdline(反域名包名), Windows 匹配映像名 DingTalk.exe
    #[cfg(unix)] let needle="com.alibabainc.dingtalk";
    #[cfg(windows)] let needle="DingTalk";
    let pid=mem::pid_by_cmd(needle).ok_or("钉钉未运行")?;
    let mut recs:Vec<Vec<Val>>=Vec::new();
    mem::for_regions(&pid,|buf|{ leaf::scan(buf,&mut recs); })?;
    let hi=now_ms_plus_day(); // 时间戳上界: 当前时间+1天(消息不会来自未来)
    // 会话标题行: NTTTII = [Null, cid(纯数字群), 标题, "", 小整数(≈2), ts]。每群一条, 非聊天消息。
    let title_of=|v:&[Val]| -> Option<(String,String)> {
        if v.len()!=6 { return None; }
        let (Val::Null, Val::Text(cid), Val::Text(title), Val::Text(_), Val::Int(f4), Val::Int(ts))
            = (&v[0],&v[1],&v[2],&v[3],&v[4],&v[5]) else { return None; };
        if cid.len()<5 || !cid.bytes().all(|b|b.is_ascii_digit()) { return None; }
        if !(0i64..=16).contains(f4) || *ts<LO || *ts>hi { return None; }
        let t=title.trim();
        if t.is_empty()||t.starts_with("http")||t.starts_with('@')||t.starts_with('{')||t.chars().count()>40 { return None; }
        Some((cid.clone(), t.to_string()))
    };
    let mut s=store::Store::open(&format!("{}/imstore.db",data()))?;
    // idmap: 统一的 id->名称 表(人名 + 群标题, 都按 id 存, 历史累积)
    let mut idmap:std::collections::HashMap<String,String>=store::load_names(&s,IM);
    let mut myid:Option<String>=None;
    for v in &recs {
        // 会话标题行 -> cid->群名(覆盖式, 优先级高于人名)
        if let Some((cid,title))=title_of(v){ idmap.insert(cid,title); continue; }
        // NIT 行 [Null, uid, 名称]: 直接的 uid->名字 / cid->群名 表
        if v.len()==3 { if let Val::Null=v[0] {
            let uid=as_uid(&v[1]);
            if let Val::Text(name)=&v[2] {
                let innum=uid.parse::<i64>().map(|n|n>=LO&&n<=hi).unwrap_or(false); // 排除 uid 实为时间戳
                if !uid.is_empty() && !innum {
                    let nm=clean_name(name);
                    if is_person_name(&nm){ idmap.entry(uid).or_insert(nm); }
                }
            }
        }}
        for x in v { if let Val::Text(t)=x {
            if t.contains("\"nick\"")&&(t.contains("\"uId\"")||t.contains("\"uid\"")) { parse_roster_json(t,&mut idmap); }
            else if t.starts_with("{\"") && t.contains("\":\"") { parse_mentions(t,&mut idmap); } // @提及 {uid:name}
            if myid.is_none() && t.contains("\"user_id\"") { if let Some(u)=json_val(t,"user_id"){ if u.bytes().all(|b|b.is_ascii_digit())&&u.len()>=5 {myid=Some(u);} } }
        }}
    }
    let mut newn=0i64;
    let mut chats:std::collections::HashSet<(String,String,i64)>=std::collections::HashSet::new();
    // 本次扫描内先按 (ts,内容) 去重, 保留解析质量最高一份(有发送者 > 有会话名)
    struct Cand{ cid:String, suid:String, sender:String, name:String, grp:i64, src:String }
    let qual=|c:&Cand| -> i32 { (if !c.sender.is_empty(){2}else{0}) + (if c.name!=c.cid {1} else {0}) };
    let mut best:std::collections::HashMap<(i64,String),Cand>=std::collections::HashMap::new();
    for v in &recs {
        if title_of(v).is_some(){ continue; } // 会话标题行不是聊天消息, 跳过
        // ts: 落在 [LO,hi] 的 13 位毫秒整数
        let mut ts=None;
        for x in v { if let Val::Int(i)=x { if *i>=LO && *i<=hi { ts=Some(*i); } } }
        let ts=match ts{Some(t)=>t/1000,None=>continue};
        // cid: 第一个"纯数字(群) 或 uidA:uidB(单聊)"文本字段; 记录其位置
        let mut cid_idx=None; let mut cid=String::new();
        for (k,x) in v.iter().enumerate(){ if let Val::Text(t)=x { if is_cid_text(t){ cid=t.clone(); cid_idx=Some(k); break; } } }
        // content: cid 之后第一个合格正文; 回退为全记录里最长合格正文
        let mut content:Option<&str>=None;
        if let Some(ci)=cid_idx { for x in &v[ci+1..] { if let Val::Text(t)=x { if good_content(t){ content=Some(t); break; } } } }
        if content.is_none() { for x in v { if let Val::Text(t)=x { if good_content(t){ if content.map_or(true,|c|t.len()>c.len()){content=Some(t);} } } } }
        let content=match content{Some(c)=>c,None=>continue};
        // 发送者 uid: cid 紧邻的前一字段; 回退为记录里在 idmap 中的 uid
        let mut suid=String::new();
        if let Some(ci)=cid_idx { if ci>=1 { suid=as_uid(&v[ci-1]); } }
        if suid.is_empty(){ for x in v { let u=as_uid(x); if !u.is_empty() && u!=cid && idmap.contains_key(&u){ suid=u; break; } } }
        // 会话类型 + 名称: 含':'=单聊(取对方名), 纯数字=群聊(取群标题)
        let grp = if cid.contains(':'){0}else{1};
        let name = if grp==0 {
            let other=cid.split(':').find(|p|Some(*p)!=myid.as_deref()).unwrap_or("");
            idmap.get(other).cloned().unwrap_or_else(||cid.clone())
        } else {
            idmap.get(&cid).cloned().unwrap_or_else(||cid.clone())
        };
        let sender=idmap.get(&suid).cloned().unwrap_or_default();
        let content=rewrite_mentions(content,&idmap);
        // src_id: cid:msgid 文本优先, 否则 ts-hash
        let mut src=format!("{}-{:x}",ts,djb2(&content));
        for x in v { if let Val::Text(t)=x { if is_cidmsgid(t){ src=t.clone(); break; } } }
        let cid=if cid.is_empty(){"unknown".into()}else{cid};
        let cand=Cand{cid,suid,sender,name,grp,src};
        let key=(ts, content);
        match best.get(&key){ Some(old) if qual(old)>=qual(&cand)=>{}, _=>{ best.insert(key,cand); } }
    }
    {
        let tx=s.begin();
        for ((ts,content),c) in &best {
            if store::ins_msg_uid(&tx,IM,&c.cid,&c.name,*ts,&fmt_time(*ts),&c.sender,"文本",content,&c.src,&c.suid){ newn+=1; }
            chats.insert((c.cid.clone(),c.name.clone(),c.grp));
        }
        // 持久化 id->名称(人名+群名), 回填历史空 sender 与会话名
        for (id,nm) in &idmap {
            store::save_name(&tx,IM,id,nm);
            store::backfill_sender(&tx,IM,id,nm);
            let _=tx.execute("UPDATE messages SET chat_name=?3 WHERE im=?1 AND chat_id=?2 AND (chat_name=?2 OR chat_name='')",rusqlite::params![IM,id,nm]);
        }
        for (cid,name,grp) in &chats { store::upsert_chat(&tx,IM,cid,name,*grp); }
        tx.commit().map_err(|e|e.to_string())?;
    }
    let total:i64=s.c.query_row("SELECT count(*) FROM messages WHERE im=?1",rusqlite::params![IM],|r|r.get(0)).unwrap_or(0);
    eprintln!("[dingtalk] 扫描记录 {}, +{newn} 新消息, 累计 {total}",recs.len());
    Ok(())
}

fn main(){
    let a:Vec<String>=std::env::args().collect();
    match a.get(1).map(|s|s.as_str()).unwrap_or("help"){
        "collect"=>{ if let Err(e)=collect(){eprintln!("collect 失败: {e}");std::process::exit(1);} }
        "watch"=>{ let iv:u64=a.iter().position(|x|x=="--interval").and_then(|i|a.get(i+1)).and_then(|x|x.parse().ok()).unwrap_or(60);
            eprintln!("[dingtalk] watch 每 {iv}s 采集一次 (Ctrl+C 停止)");
            loop{ if let Err(e)=collect(){eprintln!("[dingtalk] {e}");} std::thread::sleep(std::time::Duration::from_secs(iv)); } }
        _=>{ eprintln!("imc-dingtalk <collect|watch [--interval N]>\n  (需钉钉登录运行 + ptrace_scope=0)"); std::process::exit(2); }
    }
}
