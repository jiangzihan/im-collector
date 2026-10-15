#!/usr/bin/env python3
"""多 IM 聊天记录 MCP 服务: 读统一库(home 共享目录), 供 Claude 按 IM/会话/时间分析。"""
import os,sqlite3,subprocess,datetime
from mcp.server.fastmcp import FastMCP
ROOT=os.environ.get("IMC_HOME", os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# 运行时数据目录: 不在源码树内, 默认 home 共享目录 ~/.local/share/im-collector
def _data_dir():
    d=os.environ.get("IMC_DATA")
    if not d:
        xdg=os.environ.get("XDG_DATA_HOME")
        d=os.path.join(xdg,"im-collector") if xdg else os.path.join(os.path.expanduser("~"),".local","share","im-collector")
    return d
DATA=_data_dir()
DB=os.path.join(DATA,"imstore.db")
BIN=os.path.join(ROOT,"bin")
mcp=FastMCP("im-collector")

def conn():
    c=sqlite3.connect(DB); c.row_factory=sqlite3.Row; return c
def to_ts(s):
    if not s: return None
    s=s.strip()
    for f in ("%Y-%m-%d %H:%M:%S","%Y-%m-%d %H:%M","%Y-%m-%d","%Y/%m/%d"):
        try: return int(datetime.datetime.strptime(s,f).timestamp())
        except ValueError: continue
    return int(s) if s.isdigit() else None

@mcp.tool()
def list_ims() -> str:
    """列出已采集的 IM 及消息数。"""
    with conn() as c:
        rows=c.execute("SELECT im,count(*) n,count(DISTINCT chat_id) chats FROM messages GROUP BY im").fetchall()
    return "\n".join(f"{r['im']}: {r['n']} 条消息, {r['chats']} 个会话" for r in rows) or "空(先 refresh)"

@mcp.tool()
def list_chats(im:str, with_text_only:bool=True, limit:int=80) -> str:
    """列出某 IM 的会话。im='wechat'|'dingtalk'。"""
    q="SELECT name,chat_id,is_group,text_count,msg_count,last_time FROM chats WHERE im=?"
    if with_text_only: q+=" AND text_count>0"
    q+=" ORDER BY text_count DESC LIMIT ?"
    with conn() as c: rows=c.execute(q,(im,limit)).fetchall()
    if not rows: return f"{im} 无会话数据"
    return "文本/总 | 群? | 名称 | 最后时间 | chat_id\n"+"\n".join(
        f"{r['text_count']}/{r['msg_count']} | {'群' if r['is_group'] else '个人'} | {r['name']} | {r['last_time']} | {r['chat_id']}" for r in rows)

@mcp.tool()
def get_messages(im:str, chat:str="", since:str="", until:str="", limit:int=500, only_text:bool=True) -> str:
    """取消息。im 必填; chat=会话名(模糊)或 chat_id; since/until 支持 'YYYY-MM-DD'。"""
    q="SELECT time,chat_name,sender,type,content FROM messages WHERE im=?"; p=[im]
    if chat: q+=" AND (chat_id=? OR chat_name LIKE ?)"; p+= [chat,f"%{chat}%"]
    t0,t1=to_ts(since),to_ts(until)
    if t0: q+=" AND ts>=?"; p.append(t0)
    if t1: q+=" AND ts<=?"; p.append(t1)
    if only_text: q+=" AND type='文本'"
    q+=" ORDER BY ts LIMIT ?"; p.append(limit)
    with conn() as c: rows=c.execute(q,p).fetchall()
    if not rows: return "无匹配消息"
    out=[]
    for r in rows:
        c_=(r["content"] or "").replace("\n"," ")[:800]
        who=r["sender"] or ""
        out.append(f"[{r['time']}] «{r['chat_name']}» {who}: {c_}" if not chat else f"[{r['time']}] {who}: {c_}")
    return "\n".join(out)

@mcp.tool()
def search(keyword:str, im:str="", since:str="", until:str="", limit:int=200) -> str:
    """全局/按 IM 搜索消息内容。"""
    q="SELECT im,time,chat_name,sender,content FROM messages WHERE content LIKE ?"; p=[f"%{keyword}%"]
    if im: q+=" AND im=?"; p.append(im)
    t0,t1=to_ts(since),to_ts(until)
    if t0: q+=" AND ts>=?"; p.append(t0)
    if t1: q+=" AND ts<=?"; p.append(t1)
    q+=" ORDER BY ts DESC LIMIT ?"; p.append(limit)
    with conn() as c: rows=c.execute(q,p).fetchall()
    if not rows: return f"未找到含 '{keyword}' 的消息"
    return f"命中 {len(rows)} 条:\n"+"\n".join(f"[{r['time']}] {r['im']}«{r['chat_name']}» {r['sender'] or ''}: {(r['content'] or '').replace(chr(10),' ')[:300]}" for r in rows)

@mcp.tool()
def refresh(im:str) -> str:
    """立即采集某 IM 最新消息 (im='wechat'|'dingtalk')。需对应客户端登录运行 + ptrace_scope=0。"""
    exe=os.path.join(BIN,f"imc-{im}")
    if not os.path.exists(exe): return f"未知 im: {im}"
    r=subprocess.run([exe,"collect"],capture_output=True,text=True,timeout=240)
    with conn() as c: n=c.execute("SELECT count(*) FROM messages WHERE im=?",(im,)).fetchone()[0]
    return (r.stdout+r.stderr).strip()+f"\n{im} 当前 {n} 条。"

if __name__=="__main__":
    # 传输方式: 默认 stdio(各 agent 自行拉起, 最通用);
    # 设 IMC_MCP_TRANSPORT=sse|http 则作为常驻服务监听(多 agent 按 URL 共享一个实例)。
    transport=os.environ.get("IMC_MCP_TRANSPORT","stdio").lower()
    if transport in ("sse","http","streamable-http"):
        host=os.environ.get("IMC_MCP_HOST","127.0.0.1")
        port=int(os.environ.get("IMC_MCP_PORT","8900"))
        try:
            mcp.settings.host=host; mcp.settings.port=port
        except Exception:
            pass
        mcp.run(transport="sse" if transport=="sse" else "streamable-http")
    else:
        mcp.run()
