//! 多 IM 采集共享库: 存储 / SQLCipher 解密 / 进程内存扫描 / SQLite 叶子页解析。
use std::fs::File;
use std::io::{Read, Write};

// ============ 工具 ============
pub fn md5hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new(); h.update(s.as_bytes()); hex::encode(h.finalize())
}
pub fn unzstd(b: &[u8]) -> String {
    if b.len() >= 4 && b[0]==0x28 && b[1]==0xb5 && b[2]==0x2f && b[3]==0xfd {
        if let Ok(o) = zstd::decode_all(b) { return String::from_utf8_lossy(&o).into_owned(); }
    }
    String::from_utf8_lossy(b).into_owned()
}
pub fn type_label(t: i64) -> String {
    match t { 1=>"文本",3=>"图片",34=>"语音",43=>"视频",47=>"表情",49=>"链接/文件/引用",42=>"名片",48=>"位置",10000|10002=>"系统",_=>return format!("类型{t}") }.into()
}
/// Unix 秒 -> 本地(UTC+8) "YYYY-MM-DD HH:MM:SS"
pub fn fmt_time(ts: i64) -> String {
    let secs = ts + 8*3600; let days = secs.div_euclid(86400); let rem = secs.rem_euclid(86400);
    let (h,mi,s)=(rem/3600,(rem%3600)/60,rem%60);
    let z=days+719468; let era=if z>=0 {z} else {z-146096}/146097; let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365; let y0=yoe+era*400;
    let doy=doe-(365*yoe+yoe/4-yoe/100); let mp=(5*doy+2)/153; let d=doy-(153*mp+2)/5+1;
    let m=if mp<10 {mp+3} else {mp-9}; let y=if m<=2 {y0+1} else {y0};
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}",y,m,d,h,mi,s)
}

// ============ SQLCipher (微信 v4) ============
pub mod crypto {
    use super::*;
    use hmac::{Hmac, Mac}; use sha2::Sha512;
    pub const KEY:usize=32; pub const SALT:usize=16; pub const IV:usize=16;
    pub const PAGE:usize=4096; pub const HMAC:usize=64; pub const RESERVE:usize=80;
    type H=Hmac<Sha512>;
    pub fn pbkdf2_sha512(pw:&[u8],salt:&[u8],rounds:u32,out:usize)->Vec<u8>{let mut o=vec![0u8;out];pbkdf2::pbkdf2_hmac::<Sha512>(pw,salt,rounds,&mut o);o}
    fn hmac512(key:&[u8],parts:&[&[u8]])->Vec<u8>{let mut m=H::new_from_slice(key).unwrap();for p in parts{m.update(p);}m.finalize().into_bytes().to_vec()}
    fn mackey(enc:&[u8],salt:&[u8])->Vec<u8>{let ms:Vec<u8>=salt.iter().map(|b|b^0x3a).collect();pbkdf2_sha512(enc,&ms,2,KEY)}
    pub fn validate_raw(page1:&[u8],enc:&[u8])->bool{
        if enc.len()!=KEY||page1.len()<PAGE{return false;}
        let salt=&page1[..SALT]; let mk=mackey(enc,salt); let de=PAGE-RESERVE+IV;
        hmac512(&mk,&[&page1[SALT..de],&1u32.to_le_bytes()])==page1[de..de+HMAC]
    }
    fn cbc_dec(key:&[u8],iv:&[u8],data:&[u8])->Vec<u8>{
        use aes::cipher::{BlockDecrypt,KeyInit,generic_array::GenericArray};
        let c=aes::Aes256::new(GenericArray::from_slice(key));
        let mut prev=[0u8;16]; prev.copy_from_slice(iv); let mut out=Vec::with_capacity(data.len());
        for ch in data.chunks(16){let mut blk=[0u8;16];blk.copy_from_slice(ch);let ct=blk;let mut ga=GenericArray::clone_from_slice(&blk);c.decrypt_block(&mut ga);for i in 0..16{out.push(ga[i]^prev[i]);}prev=ct;}
        out
    }
    pub fn load_keys(arg:&str)->Result<Vec<Vec<u8>>,String>{
        if arg.len()==64 { if let Ok(k)=hex::decode(arg){return Ok(vec![k]);} }
        let data=std::fs::read_to_string(arg).map_err(|_|"key 非64hex且非keyfile".to_string())?;
        Ok(data.lines().filter_map(|l|{let l=l.trim();if l.len()==64{hex::decode(l).ok()}else{None}}).collect())
    }
    pub fn decrypt_db(keys:&[Vec<u8>],inp:&str,outp:&str)->Result<(),String>{
        let mut f=File::open(inp).map_err(|e|format!("open {inp}: {e}"))?;
        if f.metadata().map_err(|e|e.to_string())?.len()<PAGE as u64{return Err("too small".into());}
        let mut p1=vec![0u8;PAGE]; f.read_exact(&mut p1).map_err(|e|e.to_string())?;
        if &p1[..15]==b"SQLite format 3"{return Err("already decrypted".into());}
        let enc=keys.iter().find(|k|validate_raw(&p1,k)).ok_or_else(||format!("no key validates {inp}"))?;
        let salt=p1[..SALT].to_vec(); let mk=mackey(enc,&salt);
        use aes::cipher::{BlockDecrypt,KeyInit,generic_array::GenericArray};
        let _=aes::Aes256::new(GenericArray::from_slice(enc)); // ensure key len ok
        let all=std::fs::read(inp).map_err(|e|e.to_string())?;
        let mut out=File::create(outp).map_err(|e|e.to_string())?;
        out.write_all(b"SQLite format 3\x00").map_err(|e|e.to_string())?;
        let de=PAGE-RESERVE+IV; let total=all.len()/PAGE;
        for pn in 0..total{
            let pg=&all[pn*PAGE..(pn+1)*PAGE];
            if pg.iter().all(|&b|b==0){out.write_all(pg).map_err(|e|e.to_string())?;continue;}
            let off=if pn==0{SALT}else{0};
            let calc=hmac512(&mk,&[&pg[off..de],&((pn as u32)+1).to_le_bytes()]);
            if calc!=pg[de..de+HMAC]{return Err(format!("HMAC fail page {pn}"));}
            let iv=&pg[PAGE-RESERVE..PAGE-RESERVE+IV];
            let dec=cbc_dec(enc,iv,&pg[off..PAGE-RESERVE]);
            out.write_all(&dec).map_err(|e|e.to_string())?;
            out.write_all(&pg[PAGE-RESERVE..PAGE]).map_err(|e|e.to_string())?;
        }
        Ok(())
    }
    /// 扫描 /proc/<pid>/mem 里 x'<64hex>...' 原始密钥字面量
    pub fn scan_raw_keys(pid:&str)->Result<Vec<[u8;KEY]>,String>{
        let mut seen=std::collections::HashSet::new(); let mut out=Vec::new();
        super::mem::for_regions(pid,|buf|{
            let n=buf.len(); let mut i=0;
            while i+2<n {
                if buf[i]==b'x'&&buf[i+1]==b'\''{
                    let mut j=i+2; while j<n && is_hex(buf[j]){j+=1;}
                    if j-(i+2)>=64 { if let Ok(raw)=hex::decode(&buf[i+2..i+2+64]){ if raw.len()==KEY{let mut a=[0u8;KEY];a.copy_from_slice(&raw); if seen.insert(a){out.push(a);}}}}
                    i=j;
                } else { i+=1; }
            }
        })?;
        Ok(out)
    }
    fn is_hex(b:u8)->bool{ b.is_ascii_digit()||(b'a'..=b'f').contains(&b)||(b'A'..=b'F').contains(&b) }
    /// 跨平台兜底: 在进程内存里按 step 对齐滑窗, 找能通过数据库首页校验的 32 字节原始密钥
    /// (不依赖 x'..' 字面量; Windows 微信密钥为原始字节时用这个)。step 越小越全越慢(1 最全)。
    use std::sync::atomic::{AtomicBool,Ordering};
    use std::sync::Mutex;
    // 在单个内存区里多线程校验候选 32 字节密钥
    fn scan_buf(buf:&[u8], page1:&[u8], step:usize, nthreads:usize, found:&Mutex<Option<[u8;KEY]>>, stop:&AtomicBool){
        if stop.load(Ordering::Relaxed) || buf.len()<KEY { return; }
        let last=buf.len()-KEY;
        std::thread::scope(|sc|{
            for t in 0..nthreads {
                sc.spawn(move ||{
                    let mut i=t*step;
                    while i<=last {
                        if (i & 0x3ffff)==0 && stop.load(Ordering::Relaxed) { return; }
                        if validate_raw(page1,&buf[i..i+KEY]){
                            let mut a=[0u8;KEY]; a.copy_from_slice(&buf[i..i+KEY]);
                            *found.lock().unwrap()=Some(a); stop.store(true,Ordering::Relaxed); return;
                        }
                        i+=step*nthreads;
                    }
                });
            }
        });
    }
    /// 跨平台兜底取密钥: 逐候选校验数据库首页, 命中即真密钥。
    /// 作用域逐步放宽(先私有可写堆, 再全部可写, 最后全部可读), 以免密钥在被过滤掉的区里。
    pub fn find_key_validate(pid:&str, page1:&[u8], step:usize)->Result<Option<[u8;KEY]>,String>{
        let step=step.max(1);
        let nthreads=std::thread::available_parallelism().map(|n|n.get()).unwrap_or(4);
        let found:Mutex<Option<[u8;KEY]>>=Mutex::new(None);
        let stop=AtomicBool::new(false);
        let got=||found.lock().unwrap().is_some();
        // 1) 私有可写堆(最省, 多数命中在此)
        super::mem::for_regions_wp(pid,|b| scan_buf(b,page1,step,nthreads,&found,&stop))?;
        // 2) 全部可写(含映射的可写区, 覆盖"密钥不在私有堆"的情况)
        if !got() { super::mem::for_regions_w(pid,|b| scan_buf(b,page1,step,nthreads,&found,&stop))?; }
        // 3) 全部可读(最后兜底, 较慢)
        if !got() { super::mem::for_regions(pid,|b| scan_buf(b,page1,step,nthreads,&found,&stop))?; }
        Ok(found.into_inner().unwrap())
    }
}

// ============ 进程内存(跨平台) ============
// 统一 API: pid_by_cmd(needle)->pid, for_regions(pid, f) 遍历可读内存区。
// Linux: /proc/<pid>/{cmdline,status,maps,mem}; Windows: ToolHelp + VirtualQueryEx + ReadProcessMemory。
// 注意 needle 语义: Linux 匹配 cmdline 子串; Windows 匹配进程映像名(不区分大小写)子串。
pub mod mem {
    #[cfg(unix)]
    mod imp {
        use std::fs::File;
        use std::os::unix::fs::FileExt;
        pub fn pid_by_cmd(needle:&str)->Option<String>{
            let mut best:(u64,String)=(0,String::new());
            for e in std::fs::read_dir("/proc").ok()?{
                let e=e.ok()?; let p=e.file_name().into_string().unwrap_or_default();
                if !p.chars().all(|c|c.is_ascii_digit()){continue;}
                if let Ok(cl)=std::fs::read(format!("/proc/{p}/cmdline")){
                    if cl.windows(needle.len()).any(|w|w==needle.as_bytes()){
                        let rss=rss_kb(&p); if rss>best.0{best=(rss,p);}
                    }
                }
            }
            if best.1.is_empty(){None}else{Some(best.1)}
        }
        fn rss_kb(pid:&str)->u64{
            if let Ok(s)=std::fs::read_to_string(format!("/proc/{pid}/status")){
                for l in s.lines(){ if let Some(r)=l.strip_prefix("VmRSS:"){ return r.split_whitespace().next().and_then(|x|x.parse().ok()).unwrap_or(0);}}
            }
            0
        }
        /// 对每个可读内存区调用 f(&buf)
        pub fn for_regions<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,false,false,f) }
        /// 只遍历可写内存区(堆; 找运行时密钥用, 范围小得多)
        pub fn for_regions_w<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,true,false,f) }
        /// 只遍历可写 + 私有/匿名内存区(堆; 排除文件映射, 找密钥最省)
        pub fn for_regions_wp<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,true,true,f) }
        fn for_regions_impl<F:FnMut(&[u8])>(pid:&str,wonly:bool,priv_only:bool,mut f:F)->Result<(),String>{
            let maps=std::fs::read_to_string(format!("/proc/{pid}/maps")).map_err(|e|format!("maps: {e}"))?;
            let mf=File::open(format!("/proc/{pid}/mem")).map_err(|e|format!("mem(需 ptrace_scope=0?): {e}"))?;
            for line in maps.lines(){
                let mut it=line.split_whitespace();
                let range=match it.next(){Some(r)=>r,None=>continue};
                let perms=it.next().unwrap_or(""); if !perms.starts_with('r'){continue;}
                if wonly && perms.as_bytes().get(1)!=Some(&b'w'){continue;}
                if priv_only {
                    // 字段: range perms offset dev inode [pathname]; 私有=匿名(无 pathname)或 [heap]/[anon*]
                    let (_off,_dev,_ino)=(it.next(),it.next(),it.next());
                    let path=it.next().unwrap_or("");
                    let is_priv = path.is_empty() || path=="[heap]" || path.starts_with("[anon");
                    if !is_priv { continue; }
                }
                let mut rb=range.split('-');
                let (a,b)=match(rb.next(),rb.next()){(Some(a),Some(b))=>(a,b),_=>continue};
                let (s,e)=match(u64::from_str_radix(a,16),u64::from_str_radix(b,16)){(Ok(s),Ok(e)) if e>s=>(s,e),_=>continue};
                let sz=(e-s) as usize; if sz>800*1024*1024{continue;}
                let mut buf=vec![0u8;sz];
                if let Ok(n)=mf.read_at(&mut buf,s){ if n>0 { f(&buf[..n]); } }
            }
            Ok(())
        }
    }

    #[cfg(windows)]
    mod imp {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::Memory::{
            VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE,
            PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
            PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
        };
        use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};

        /// 按进程映像名(不区分大小写)子串匹配, 取工作集最大的进程
        pub fn pid_by_cmd(needle:&str)->Option<String>{
            let needle=needle.to_lowercase();
            let mut best:(usize,u32)=(0,0);
            unsafe{
                let snap=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0).ok()?;
                let mut pe=PROCESSENTRY32W{ dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
                if Process32FirstW(snap,&mut pe).is_ok(){
                    loop{
                        let len=pe.szExeFile.iter().position(|&c|c==0).unwrap_or(pe.szExeFile.len());
                        let name=String::from_utf16_lossy(&pe.szExeFile[..len]);
                        if name.to_lowercase().contains(&needle){
                            let ws=working_set(pe.th32ProcessID);
                            if ws>=best.0{ best=(ws,pe.th32ProcessID); }
                        }
                        if Process32NextW(snap,&mut pe).is_err(){break;}
                    }
                }
                let _=CloseHandle(snap);
            }
            if best.1==0{None}else{Some(best.1.to_string())}
        }

        fn working_set(pid:u32)->usize{
            unsafe{
                let h=match OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ,false,pid){Ok(h)=>h,Err(_)=>return 0};
                let mut pmc=PROCESS_MEMORY_COUNTERS{ cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                let r=GetProcessMemoryInfo(h,&mut pmc,pmc.cb);
                let _=CloseHandle(h);
                if r.is_ok(){ pmc.WorkingSetSize } else {0}
            }
        }

        /// 对每个已提交且可读的内存区调用 f(&buf)
        pub fn for_regions<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,false,false,f) }
        /// 只遍历可写内存区(堆; 找运行时密钥用)
        pub fn for_regions_w<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,true,false,f) }
        /// 只遍历可写 + 私有(MEM_PRIVATE)内存区(堆; 排除映像/文件映射, 找密钥最省)
        pub fn for_regions_wp<F:FnMut(&[u8])>(pid:&str,f:F)->Result<(),String>{ for_regions_impl(pid,true,true,f) }
        fn for_regions_impl<F:FnMut(&[u8])>(pid:&str,wonly:bool,priv_only:bool,mut f:F)->Result<(),String>{
            let pid:u32=pid.parse().map_err(|_|"bad pid".to_string())?;
            unsafe{
                let h:HANDLE=OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ,false,pid)
                    .map_err(|e|format!("OpenProcess(需管理员/SeDebugPrivilege?): {e}"))?;
                let mut addr:usize=0;
                let read_mask=PAGE_READONLY.0|PAGE_READWRITE.0|PAGE_WRITECOPY.0
                    |PAGE_EXECUTE_READ.0|PAGE_EXECUTE_READWRITE.0|PAGE_EXECUTE_WRITECOPY.0;
                let write_mask=PAGE_READWRITE.0|PAGE_WRITECOPY.0|PAGE_EXECUTE_READWRITE.0|PAGE_EXECUTE_WRITECOPY.0;
                let mask=if wonly{write_mask}else{read_mask};
                loop{
                    let mut mbi=MEMORY_BASIC_INFORMATION::default();
                    let got=VirtualQueryEx(h,Some(addr as *const _),&mut mbi,std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
                    if got==0{break;}
                    let base=mbi.BaseAddress as usize;
                    let size=mbi.RegionSize;
                    let prot=mbi.Protect.0;
                    let ok_region=mbi.State==MEM_COMMIT
                        && (prot&mask)!=0
                        && (prot&PAGE_GUARD.0)==0
                        && (prot&PAGE_NOACCESS.0)==0
                        && (!priv_only || mbi.Type==MEM_PRIVATE);
                    if ok_region && size>0 && size<=800*1024*1024 {
                        let mut buf=vec![0u8;size];
                        let mut read:usize=0;
                        let ok=ReadProcessMemory(h,base as *const _,buf.as_mut_ptr() as *mut _,size,Some(&mut read)).is_ok();
                        if ok && read>0 { f(&buf[..read]); }
                    }
                    match base.checked_add(size){ Some(n) if n>addr=>addr=n, _=>break }
                }
                let _=CloseHandle(h);
            }
            Ok(())
        }
    }

    pub use imp::{pid_by_cmd, for_regions, for_regions_w, for_regions_wp};
}

// ============ SQLite 叶子页解析(钉钉内存取证) ============
pub mod leaf {
    #[derive(Clone,Debug)]
    pub enum Val{ Null, Int(i64), Real(f64), Text(String), Blob(usize) }
    fn varint(b:&[u8],mut o:usize)->(i64,usize){
        let mut v:i64=0;
        for i in 0..9 {
            if o>=b.len(){return (v,o);}
            let c=b[o]; o+=1;
            if i==8 { v=(v<<8)|c as i64; return (v,o);}
            v=(v<<7)|(c&0x7f) as i64;
            if c&0x80==0 { return (v,o);}
        }
        (v,o)
    }
    fn parse_record(pl:&[u8])->Option<Vec<Val>>{
        let (hlen,mut o)=varint(pl,0); let hlen=hlen as usize;
        if hlen==0||hlen>pl.len(){return None;}
        let mut types=Vec::new();
        while o<hlen { let (t,no)=varint(pl,o); if no<=o{return None;} o=no; types.push(t); }
        let mut vals=Vec::new(); let mut p=hlen;
        for t in types {
            match t {
                0=>vals.push(Val::Null),
                1=>{if p+1>pl.len(){break;} vals.push(Val::Int(i8::from_be_bytes([pl[p]]) as i64));p+=1;}
                2=>{if p+2>pl.len(){break;} vals.push(Val::Int(i16::from_be_bytes([pl[p],pl[p+1]]) as i64));p+=2;}
                3=>{if p+3>pl.len(){break;} let mut x=((pl[p] as i64)<<16)|((pl[p+1] as i64)<<8)|pl[p+2] as i64; if x&0x800000!=0{x-=1<<24;} vals.push(Val::Int(x));p+=3;}
                4=>{if p+4>pl.len(){break;} vals.push(Val::Int(i32::from_be_bytes([pl[p],pl[p+1],pl[p+2],pl[p+3]]) as i64));p+=4;}
                5=>{if p+6>pl.len(){break;} let mut x=0i64;for k in 0..6{x=(x<<8)|pl[p+k] as i64;} if x&0x800000000000!=0{x-=1<<48;} vals.push(Val::Int(x));p+=6;}
                6=>{if p+8>pl.len(){break;} vals.push(Val::Int(i64::from_be_bytes(pl[p..p+8].try_into().unwrap())));p+=8;}
                7=>{if p+8>pl.len(){break;} vals.push(Val::Real(f64::from_be_bytes(pl[p..p+8].try_into().unwrap())));p+=8;}
                8=>vals.push(Val::Int(0)), 9=>vals.push(Val::Int(1)),
                x if x>=12 && x%2==0 => { let n=((x-12)/2) as usize; p+=n; vals.push(Val::Blob(n)); }
                x if x>=13 => { let n=((x-13)/2) as usize; if p+n>pl.len(){break;} vals.push(Val::Text(String::from_utf8_lossy(&pl[p..p+n]).into_owned())); p+=n; }
                _=>vals.push(Val::Null),
            }
        }
        Some(vals)
    }
    /// 在 buf 里扫描叶子表页(0x0D), 返回所有记录
    pub fn scan(buf:&[u8], out:&mut Vec<Vec<Val>>){
        let n=buf.len(); let mut i=0;
        while i<n {
            if buf[i]!=0x0d { i+=1; continue; }
            let off=i; i+=1;
            if off+8>n { continue; }
            let nc=((buf[off+3] as usize)<<8)|buf[off+4] as usize;
            if nc<1||nc>300 { continue; }
            for c in 0..nc {
                let po=off+8+c*2; if po+2>n{break;}
                let co=((buf[po] as usize)<<8)|buf[po+1] as usize;
                if co<8||off+co+2>n{continue;}
                let (plen,o)=varint(buf,off+co); let plen=plen as usize;
                if plen<2||plen>3900{continue;}
                let (_rowid,o2)=varint(buf,o);
                if o2+plen>n{continue;}
                if let Some(r)=parse_record(&buf[o2..o2+plen]){ out.push(r); }
            }
        }
    }
}

// ============ 统一存储 ============
pub mod store {
    use rusqlite::Connection;
    pub struct Store{ pub c:Connection }
    impl Store{
        pub fn open(path:&str)->Result<Self,String>{
            let c=Connection::open(path).map_err(|e|e.to_string())?;
            c.execute_batch(
              "CREATE TABLE IF NOT EXISTS chats(im TEXT,chat_id TEXT,name TEXT,is_group INT,msg_count INT,text_count INT,last_time TEXT,PRIMARY KEY(im,chat_id));
               CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY AUTOINCREMENT,im TEXT,chat_id TEXT,chat_name TEXT,ts INTEGER,time TEXT,sender TEXT,type TEXT,content TEXT,src_id TEXT,sender_uid TEXT);
               CREATE UNIQUE INDEX IF NOT EXISTS ux_msg ON messages(im,chat_id,src_id);
               CREATE INDEX IF NOT EXISTS ix_msg_ts ON messages(im,ts);
               CREATE TABLE IF NOT EXISTS sync_state(im TEXT,k TEXT,v INTEGER,PRIMARY KEY(im,k));
               CREATE TABLE IF NOT EXISTS names(im TEXT,uid TEXT,name TEXT,PRIMARY KEY(im,uid));"
            ).map_err(|e|e.to_string())?;
            let _=c.execute("ALTER TABLE messages ADD COLUMN sender_uid TEXT",[]); // 老库迁移(存在则忽略)
            Ok(Store{c})
        }
        pub fn cursor(&self,im:&str,k:&str)->i64{
            self.c.query_row("SELECT v FROM sync_state WHERE im=?1 AND k=?2",rusqlite::params![im,k],|r|r.get(0)).unwrap_or(0)
        }
        pub fn set_cursor(&self,im:&str,k:&str,v:i64){
            let _=self.c.execute("INSERT OR REPLACE INTO sync_state(im,k,v) VALUES(?1,?2,?3)",rusqlite::params![im,k,v]);
        }
        pub fn begin(&mut self)->rusqlite::Transaction{ self.c.transaction().unwrap() }
    }
    /// 在事务里插一条消息(去重), 返回是否新插入
    pub fn ins_msg(tx:&rusqlite::Transaction,im:&str,chat_id:&str,chat_name:&str,ts:i64,time:&str,sender:&str,ty:&str,content:&str,src_id:&str)->bool{
        ins_msg_uid(tx,im,chat_id,chat_name,ts,time,sender,ty,content,src_id,"")
    }
    pub fn ins_msg_uid(tx:&rusqlite::Transaction,im:&str,chat_id:&str,chat_name:&str,ts:i64,time:&str,sender:&str,ty:&str,content:&str,src_id:&str,sender_uid:&str)->bool{
        tx.execute("INSERT OR IGNORE INTO messages(im,chat_id,chat_name,ts,time,sender,type,content,src_id,sender_uid) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![im,chat_id,chat_name,ts,time,sender,ty,content,src_id,sender_uid]).unwrap_or(0)>0
    }
    /// 载入 im 的全部 uid->name
    pub fn load_names(st:&Store,im:&str)->std::collections::HashMap<String,String>{
        let mut m=std::collections::HashMap::new();
        if let Ok(mut q)=st.c.prepare("SELECT uid,name FROM names WHERE im=?1"){
            if let Ok(rows)=q.query_map(rusqlite::params![im],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))){
                for row in rows.flatten(){ m.insert(row.0,row.1); }
            }
        }
        m
    }
    pub fn save_name(tx:&rusqlite::Transaction,im:&str,uid:&str,name:&str){
        let _=tx.execute("INSERT OR IGNORE INTO names(im,uid,name) VALUES(?1,?2,?3)",rusqlite::params![im,uid,name]);
    }
    /// 用已知 uid->name 回填历史消息里空的 sender
    pub fn backfill_sender(tx:&rusqlite::Transaction,im:&str,uid:&str,name:&str){
        let _=tx.execute("UPDATE messages SET sender=?3 WHERE im=?1 AND sender_uid=?2 AND (sender='' OR sender IS NULL)",rusqlite::params![im,uid,name]);
    }
    pub fn upsert_chat(tx:&rusqlite::Transaction,im:&str,chat_id:&str,name:&str,is_group:i64){
        let _=tx.execute("INSERT OR REPLACE INTO chats(im,chat_id,name,is_group,msg_count,text_count,last_time)
            VALUES(?1,?2,?3,?4,
                (SELECT count(*) FROM messages WHERE im=?1 AND chat_id=?2),
                (SELECT COALESCE(SUM(type='文本'),0) FROM messages WHERE im=?1 AND chat_id=?2),
                (SELECT COALESCE(MAX(time),'') FROM messages WHERE im=?1 AND chat_id=?2))",
            rusqlite::params![im,chat_id,name,is_group]);
    }
}
