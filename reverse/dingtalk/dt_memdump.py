#!/usr/bin/env python3
# 钉钉消息内存取证: 从运行中的钉钉进程内存解析 SQLite 叶子页, 提取聊天消息。
# 依赖: 钉钉登录运行中 + ptrace_scope=0。用法: dt_memdump.py [out.tsv]
import glob,sys,struct,datetime,re,os
OUT=sys.argv[1] if len(sys.argv)>1 else os.path.expanduser("~/wxtools/data/dt_capture/dt_messages.tsv")
LO,HI=1577836800000,1790900000000
pids=[int(d.split('/')[2]) for d in glob.glob('/proc/[0-9]*/cmdline') if b'com.alibabainc.dingtalk' in open(d,'rb').read()]
if not pids: print("钉钉未运行"); sys.exit(1)
def rss(p):
    try:
        for l in open(f'/proc/{p}/status'):
            if l.startswith('VmRSS'): return int(l.split()[1])
    except: return 0
    return 0
pid=max(pids,key=rss)
maps=open(f'/proc/{pid}/maps').read(); mem=open(f'/proc/{pid}/mem','rb',0)
def rvarint(b,o):
    v=0
    for i in range(9):
        c=b[o+i]
        if i==8: return (v<<8)|c,o+9
        v=(v<<7)|(c&0x7f)
        if not(c&0x80): return v,o+i+1
    return v,o+9
def parse_record(pl):
    try:
        hlen,o=rvarint(pl,0); ts=[]
        while o<hlen: t,o=rvarint(pl,o); ts.append(t)
        vals=[]; p=hlen
        for t in ts:
            if t==0: vals.append(None)
            elif t<=6: n=[0,1,2,3,4,6,8][t]; vals.append(int.from_bytes(pl[p:p+n],'big',signed=True)); p+=n
            elif t==7: vals.append(struct.unpack('>d',pl[p:p+8])[0]); p+=8
            elif t==8: vals.append(0)
            elif t==9: vals.append(1)
            elif t>=12 and t%2==0: n=(t-12)//2; p+=n; vals.append(None)
            elif t>=13: n=(t-13)//2; vals.append(pl[p:p+n]); p+=n
            else: vals.append(None)
        return vals
    except: return None
def leaf(buf,off):
    nc=int.from_bytes(buf[off+3:off+5],'big')
    if not(1<=nc<=300): return []
    recs=[]
    for i in range(nc):
        po=off+8+i*2
        if po+2>len(buf): break
        co=int.from_bytes(buf[po:po+2],'big')
        if not(8<=co<4096) or off+co+2>len(buf): continue
        try:
            plen,o=rvarint(buf,off+co); rowid,o=rvarint(buf,o)
            if not(2<=plen<=3900): continue
            v=parse_record(buf[o:o+plen])
            if v: recs.append(v)
        except: continue
    return recs
seen=set(); rows=[]
for line in maps.splitlines():
    f=line.split()
    if len(f)<2 or f[1][0]!='r': continue
    a,b=f[0].split('-'); s=int(a,16); e=int(b,16)
    if e-s>800*1024*1024: continue
    try: mem.seek(s); buf=mem.read(e-s)
    except: continue
    idx=0
    while True:
        j=buf.find(b'\x0d',idx)
        if j==-1: break
        idx=j+1
        if j+4096>len(buf): continue
        for v in leaf(buf,j):
            ts=None
            for x in v:
                if isinstance(x,int) and LO<=x<=HI: ts=x
            if not ts: continue
            txt=None
            for x in v:
                if not isinstance(x,(bytes,bytearray)): continue
                try: s2=x.decode('utf8')
                except: continue
                if s2[:3]=='@lA' or s2[:11]=='dingtalk://' or s2[:1] in '{[': continue
                if s2.isdigit(): continue
                if len(s2)>=2 and (re.search(r'[一-鿿]',s2) or 'http' in s2 or (' ' in s2 and len(s2)>6)):
                    if txt is None or len(s2)>len(txt): txt=s2
            if not txt: continue
            k=(ts,txt)
            if k in seen: continue
            seen.add(k); rows.append((ts,txt))
rows.sort()
with open(OUT,'w') as w:
    for ts,txt in rows:
        w.write(datetime.datetime.fromtimestamp(ts/1000).strftime('%Y-%m-%d %H:%M:%S')+'\t'+txt.replace('\n',' ')+'\n')
print(f"pid={pid}  提取聊天消息 {len(rows)} 条 -> {OUT}")
