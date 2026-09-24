use imc_common::{store, fmt_time, mem, leaf};
use imc_common::leaf::Val;

const IM:&str="dingtalk";
const LO:i64=1577836800000; const HI:i64=1790900000000;
fn home()->String{ std::env::var("HOME").unwrap_or("/root".into()) }
fn root()->String{ std::env::var("IMC_HOME").unwrap_or(format!("{}/im-collector",home())) }
fn data()->String{ format!("{}/data",root()) }

fn cjk(s:&str)->usize{ s.chars().filter(|c|('\u{4e00}'..='\u{9fff}').contains(c)).count() }
fn djb2(s:&str)->u64{ let mut h:u64=5381; for b in s.bytes(){h=h.wrapping_mul(33).wrapping_add(b as u64);} h }

fn good_content(s:&str)->bool{
    if s.len()<2 {return false;}
    if s.starts_with("@lA")||s.starts_with("dingtalk://")||s.starts_with('{')||s.starts_with('['){return false;}
    if s.chars().all(|c|c.is_ascii_digit()){return false;}
    cjk(s)>=2 || s.contains("http") || (s.contains(' ') && s.len()>6)
}
fn is_cidmsgid(s:&str)->bool{
    let p:Vec<&str>=s.splitn(2,':').collect();
    p.len()==2 && !p[0].is_empty() && !p[1].is_empty() && p[0].bytes().all(|b|b.is_ascii_digit()) && p[1].bytes().all(|b|b.is_ascii_digit())
}

fn collect()->Result<(),String>{
    let pid=mem::pid_by_cmd("com.alibabainc.dingtalk").ok_or("钉钉未运行")?;
    let mut recs:Vec<Vec<Val>>=Vec::new();
    mem::for_regions(&pid,|buf|{ leaf::scan(buf,&mut recs); })?;
    let mut s=store::Store::open(&format!("{}/imstore.db",data()))?;
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
            // chat_id: 第一个较大的 Int(非ts)
            let mut cid=String::from("unknown");
            for x in v { if let Val::Int(i)=x { if *i!=ts && *i>1_000_000 { cid=i.to_string(); break; } } }
            // src_id: cid:msgid 文本优先, 否则 ts-hash
            let mut src=format!("{}-{:x}",ts,djb2(content));
            for x in v { if let Val::Text(t)=x { if is_cidmsgid(t){ src=t.clone(); break; } } }
            if store::ins_msg(&tx,IM,&cid,&cid,ts,&fmt_time(ts),"","文本",content,&src){ newn+=1; }
            chats.insert(cid);
        }
        for c in &chats { store::upsert_chat(&tx,IM,c,c,1); }
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
