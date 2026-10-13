use imc_common::{store, fmt_time, mem, leaf};
use imc_common::leaf::Val;

const IM:&str="dingtalk";
const LO:i64=1577836800000; const HI:i64=1790900000000;
fn home()->String{ std::env::var("HOME").unwrap_or("/root".into()) }
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
// 解析成员名册 {"nick":"名","uId":"号",...} -> uid->name
fn parse_roster(s:&str, map:&mut std::collections::HashMap<String,String>){
    let b=s.as_bytes(); let mut i=0; let mut last=String::new();
    while i<b.len(){
        if b[i]==b'"'{
            let ks=i+1; let mut j=ks; while j<b.len()&&b[j]!=b'"'{j+=1;} if j>=b.len(){break;}
            let key=&s[ks..j];
            if j+2<b.len()&&b[j+1]==b':'&&b[j+2]==b'"'{
                let vs=j+3; let mut k=vs; while k<b.len()&&b[k]!=b'"'{k+=1;}
                let val=&s[vs..k.min(s.len())];
                match key {
                    "nick"|"name"|"userNick"|"remark"|"orgName"|"showName" => { if !val.is_empty(){ last=clean_name(val); } }
                    "uId"|"uid"|"userId"|"emplId"|"staffId" => { if !val.is_empty() && val.bytes().all(|c|c.is_ascii_digit()) && !last.is_empty(){ map.entry(val.to_string()).or_insert_with(||last.clone()); } }
                    _=>{}
                }
                i=k+1; continue;
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
fn is_cid(t:&str)->bool{
    !t.is_empty() && t.len()<=40 && (t.bytes().all(|b|b.is_ascii_digit()) || is_cidmsgid(t))
}
// tbconversation: [pk INT, cid TEXT, tag INT, type INT, title TEXT, ...] -> (cid,title)
fn conv_entry(v:&[Val])->Option<(String,String)>{
    if v.len()<5 { return None; }
    if !matches!(v[0],Val::Int(_)) { return None; }
    let cid=match &v[1]{Val::Text(t) if is_cid(t)=>t.clone(),_=>return None};
    if !matches!(v[3],Val::Int(_)) { return None; }
    let title=match &v[4]{Val::Text(t)=>t.clone(),_=>return None};
    let tt=title.trim();
    if tt.is_empty()||tt.starts_with('{')||tt.starts_with('[')||tt.starts_with("@lA")||tt.starts_with("http")||tt.bytes().all(|b|b.is_ascii_digit()){return None;}
    Some((cid,title))
}
fn is_cidmsgid(s:&str)->bool{
    let p:Vec<&str>=s.splitn(2,':').collect();
    p.len()==2 && !p[0].is_empty() && !p[1].is_empty() && p[0].bytes().all(|b|b.is_ascii_digit()) && p[1].bytes().all(|b|b.is_ascii_digit())
}

fn collect()->Result<(),String>{
    let pid=mem::pid_by_cmd("com.alibabainc.dingtalk").ok_or("钉钉未运行")?;
    let mut recs:Vec<Vec<Val>>=Vec::new();
    mem::for_regions(&pid,|buf|{ leaf::scan(buf,&mut recs); })?;
    // 构建 cid->群名 映射
    let mut convmap:std::collections::HashMap<String,String>=std::collections::HashMap::new();
    let mut s=store::Store::open(&format!("{}/imstore.db",data()))?;
    let mut uidmap:std::collections::HashMap<String,String>=store::load_names(&s,IM); // 历史累积
    for v in &recs { for x in v { if let Val::Text(t)=x {
        if t.contains("\"uId\"")||t.contains("\"nick\"") { parse_roster(t,&mut uidmap); }
        else if t.starts_with("{\"") && t.contains("\":\"") { parse_mentions(t,&mut uidmap); }
    } } }
    for v in &recs { if let Some((cid,title))=conv_entry(v){ convmap.entry(cid).and_modify(|e|{if title.len()>e.len(){*e=title.clone();}}).or_insert(title); } }
    let mut newn=0i64; let mut chats=std::collections::HashSet::new();
    {
        let tx=s.begin();
        for v in &recs {
            // ts
            let mut ts=None;
            for x in v { if let Val::Int(i)=x { if *i>=LO && *i<=HI { ts=Some(*i); } } }
            let ts=match ts{Some(t)=>t/1000,None=>continue};
            // content
            let mut content:Option<&str>=None;
            for x in v { if let Val::Text(t)=x { if good_content(t) { if content.map_or(true,|c|t.len()>c.len()){content=Some(t);} } } }
            let content=match content{Some(c)=>c,None=>continue};
            // 数值字段(有序): [0]=cid, 找发送者uid
            let mut nums:Vec<String>=Vec::new();
            for x in v { match x { Val::Int(n)=>nums.push(n.to_string()), Val::Text(t) if !t.is_empty()&&t.bytes().all(|c|c.is_ascii_digit())=>nums.push(t.clone()), _=>{} } }
            let mut cid=String::from("unknown");
            for x in v { if let Val::Int(i)=x { if *i!=ts && *i>1_000_000 { cid=i.to_string(); break; } } }
            // 发送者uid: 优先在 uidmap 里、且非cid非13位ts的数值
            let ts_ms=(ts*1000).to_string();
            let suid=nums.iter().find(|n| **n!=cid && n.len()!=13 && *n!=&ts_ms && uidmap.contains_key(*n))
                .or_else(||nums.iter().find(|n| **n!=cid && n.len()>=8 && n.len()!=13 && *n!=&ts_ms))
                .cloned().unwrap_or_default();
            let sender=uidmap.get(&suid).cloned().unwrap_or_default();
            let content=rewrite_mentions(content,&uidmap);
            let content=content.as_str();
            // src_id: cid:msgid 文本优先, 否则 ts-hash
            let mut src=format!("{}-{:x}",ts,djb2(content));
            for x in v { if let Val::Text(t)=x { if is_cidmsgid(t){ src=t.clone(); break; } } }
            let name=convmap.get(&cid).cloned().unwrap_or_else(||cid.clone());
            if store::ins_msg_uid(&tx,IM,&cid,&name,ts,&fmt_time(ts),&sender,"文本",content,&src,&suid){ newn+=1; }
            chats.insert(cid);
        }
        // 持久化 uid->name, 并回填历史空 sender
        for (uid,nm) in &uidmap { store::save_name(&tx,IM,uid,nm); store::backfill_sender(&tx,IM,uid,nm); }
        // 回填历史消息的群名 + 更新会话名
        for (cid,title) in &convmap {
            let _=tx.execute("UPDATE messages SET chat_name=?3 WHERE im=?1 AND chat_id=?2",rusqlite::params![IM,cid,title]);
        }
        for c in &chats {
            let name=convmap.get(c).cloned().unwrap_or_else(||c.clone());
            store::upsert_chat(&tx,IM,c,&name,1);
        }
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
