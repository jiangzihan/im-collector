#!/usr/bin/env python3
"""im-collector 本地 Web: 只监听 127.0.0.1, 读 data/imstore.db, 网页轮询实现实时。"""
import os,json,sqlite3,subprocess,datetime,urllib.parse
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
ROOT=os.environ.get("IMC_HOME",os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DB=os.path.join(ROOT,"data","imstore.db"); BIN=os.path.join(ROOT,"bin")
HTML=os.path.join(os.path.dirname(os.path.abspath(__file__)),"index.html")
def db():
    c=sqlite3.connect(f"file:{DB}?mode=ro",uri=True); c.row_factory=sqlite3.Row; return c
def to_ts(s):
    if not s:return None
    for f in ("%Y-%m-%d %H:%M:%S","%Y-%m-%d %H:%M","%Y-%m-%d"):
        try:return int(datetime.datetime.strptime(s.strip(),f).timestamp())
        except:pass
    return int(s) if s.isdigit() else None
class H(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def _send(self,code,body,ctype="application/json"):
        b=body if isinstance(body,bytes) else body.encode()
        self.send_response(code);self.send_header("Content-Type",ctype);self.send_header("Content-Length",str(len(b)));self.end_headers();self.wfile.write(b)
    def do_GET(self):
        u=urllib.parse.urlparse(self.path);q=urllib.parse.parse_qs(u.query)
        g=lambda k,d="":(q.get(k,[d])[0])
        try:
            if u.path=="/" or u.path=="/index.html":
                return self._send(200,open(HTML,'rb').read(),"text/html; charset=utf-8")
            if u.path=="/api/ims":
                with db() as c: rows=c.execute("SELECT im,count(*) n,count(DISTINCT chat_id) chats,MAX(time) last FROM messages GROUP BY im").fetchall()
                return self._send(200,json.dumps([dict(r) for r in rows],ensure_ascii=False))
            if u.path=="/api/chats":
                im=g("im");sql="SELECT name,chat_id,is_group,text_count,msg_count,last_time FROM chats WHERE im=? AND text_count>0 ORDER BY last_time DESC LIMIT 200"
                with db() as c: rows=c.execute(sql,(im,)).fetchall()
                return self._send(200,json.dumps([dict(r) for r in rows],ensure_ascii=False))
            if u.path=="/api/messages":
                im=g("im");chat=g("chat");kw=g("q");lim=min(int(g("limit","200")),1000);order=g("order","desc")
                sql="SELECT im,time,ts,chat_name,sender,type,content FROM messages WHERE 1=1";p=[]
                if im:sql+=" AND im=?";p.append(im)
                if chat:sql+=" AND (chat_id=? OR chat_name LIKE ?)";p+=[chat,f"%{chat}%"]
                if kw:sql+=" AND content LIKE ?";p.append(f"%{kw}%")
                t0=to_ts(g("since"));t1=to_ts(g("until"))
                if t0:sql+=" AND ts>=?";p.append(t0)
                if t1:sql+=" AND ts<=?";p.append(t1)
                sql+=f" ORDER BY ts {'ASC' if order=='asc' else 'DESC'} LIMIT ?";p.append(lim)
                with db() as c: rows=c.execute(sql,p).fetchall()
                return self._send(200,json.dumps([dict(r) for r in rows],ensure_ascii=False))
            if u.path=="/api/refresh":
                im=g("im");exe=os.path.join(BIN,f"imc-{im}")
                if not os.path.exists(exe):return self._send(400,json.dumps({"err":"unknown im"}))
                r=subprocess.run([exe,"collect"],capture_output=True,text=True,timeout=240)
                return self._send(200,json.dumps({"out":(r.stdout+r.stderr).strip()},ensure_ascii=False))
            self._send(404,json.dumps({"err":"not found"}))
        except Exception as e:
            self._send(500,json.dumps({"err":str(e)},ensure_ascii=False))
if __name__=="__main__":
    port=int(os.environ.get("IMC_WEB_PORT","8899"))
    print(f"im-collector web: http://127.0.0.1:{port}  (Ctrl+C 停止)")
    ThreadingHTTPServer(("127.0.0.1",port),H).serve_forever()
