use imc_common::{crypto, store, md5hex, unzstd, fmt_time, type_label, mem};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

const IM:&str="wechat";
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

fn find_src()->Option<String>{
    let base=format!("{}/Documents/xwechat_files",home());
    for e in std::fs::read_dir(&base).ok()?{
        let e=e.ok()?; let p=format!("{}/db_storage",e.path().display());
        if Path::new(&p).is_dir(){return Some(p);}
    }
    None
}
fn wechat_pid()->Option<String>{ mem::pid_by_cmd("wechat") }

fn get_keys(force:bool)->Result<Vec<Vec<u8>>,String>{
    let kf=format!("{}/wechat/keys.txt",data());
    if !force { if let Ok(k)=crypto::load_keys(&kf){ if !k.is_empty(){return Ok(k);} } }
    let pid=wechat_pid().ok_or("微信未运行")?;
    let keys=crypto::scan_raw_keys(&pid)?;
    std::fs::create_dir_all(format!("{}/wechat",data())).ok();
    let mut s=String::new(); for k in &keys{ s.push_str(&hex::encode(k)); s.push('\n'); }
    std::fs::write(&kf,s).ok();
    Ok(keys.into_iter().map(|a|a.to_vec()).collect())
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
        keys=get_keys(true)?; dec_one(&keys,"message/message_0.db",&mo_out)?;
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
        "decrypt"=>{ if a.len()!=5{eprintln!("usage: imc-wechat decrypt <keyfile|hex> <in.db> <out.db>");std::process::exit(2);}
            match crypto::load_keys(&a[2]).and_then(|k|crypto::decrypt_db(&k,&a[3],&a[4])){Ok(_)=>eprintln!("OK {}",a[4]),Err(e)=>{eprintln!("{e}");std::process::exit(1);}} }
        "collect"=>{ if let Err(e)=collect(){eprintln!("collect 失败: {e}");std::process::exit(1);} }
        "watch"=>{ let iv:u64=a.iter().position(|x|x=="--interval").and_then(|i|a.get(i+1)).and_then(|x|x.parse().ok()).unwrap_or(30);
            eprintln!("[wechat] watch 每 {iv}s 采集一次 (Ctrl+C 停止)");
            loop{ if let Err(e)=collect(){eprintln!("[wechat] {e}");} std::thread::sleep(std::time::Duration::from_secs(iv)); } }
        _=>{ eprintln!("imc-wechat <keys|decrypt|collect|watch [--interval N]>"); std::process::exit(2); }
    }
}
