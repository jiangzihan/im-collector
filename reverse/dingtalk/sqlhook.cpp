// LD_PRELOAD 拦截 libsqlite3: 捕获 app 读到的明文行 (绕过文件解密)。
#include <dlfcn.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <pthread.h>
#include <unistd.h>

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static long g_rows=0, g_cap=0;
static const char* dir(){ const char* d=getenv("DT_HOOK_DIR"); return d?d:"/tmp/dt_capture"; }
static FILE* rf(){ static FILE* f=nullptr; if(!f){char p[1024];snprintf(p,sizeof p,"%s/rows.tsv",dir());f=fopen(p,"a");} return f; }
static FILE* lf(){ static FILE* f=nullptr; if(!f){char p[1024];snprintf(p,sizeof p,"%s/sql.log",dir());f=fopen(p,"a");} return f; }
static void* S(const char* n){ return dlsym(RTLD_NEXT,n); }

typedef long long i64;
static int  (*p_step)(void*)=0;
static int  (*p_ccount)(void*)=0;
static int  (*p_ctype)(void*,int)=0;
static const unsigned char* (*p_ctext)(void*,int)=0;
static int  (*p_cbytes)(void*,int)=0;
static i64  (*p_cint)(void*,int)=0;
static double (*p_cdouble)(void*,int)=0;
static const char* (*p_cname)(void*,int)=0;
static const void* (*p_cblob)(void*,int)=0;
static const char* (*p_sql)(void*)=0;

static void init_syms(){
  if(p_step) return;
  p_step=(int(*)(void*))S("sqlite3_step");
  p_ccount=(int(*)(void*))S("sqlite3_column_count");
  p_ctype=(int(*)(void*,int))S("sqlite3_column_type");
  p_ctext=(const unsigned char*(*)(void*,int))S("sqlite3_column_text");
  p_cbytes=(int(*)(void*,int))S("sqlite3_column_bytes");
  p_cint=(i64(*)(void*,int))S("sqlite3_column_int64");
  p_cdouble=(double(*)(void*,int))S("sqlite3_column_double");
  p_cname=(const char*(*)(void*,int))S("sqlite3_column_name");
  p_cblob=(const void*(*)(void*,int))S("sqlite3_column_blob");
  p_sql=(const char*(*)(void*))S("sqlite3_sql");
  g_cap = getenv("DT_MAX_ROWS")? atol(getenv("DT_MAX_ROWS")) : 500000;
}
static void emit(FILE* f,const char* s,int n){ for(int i=0;i<n;i++){unsigned char c=s[i]; if(c=='\t'||c=='\n')fputc(' ',f); else fputc(c,f);} }

extern "C" int sqlite3_step(void* stmt){
  init_syms();
  int r = p_step? p_step(stmt): 101;
  if(r==100 && stmt){                     // SQLITE_ROW
    pthread_mutex_lock(&mu);
    if(g_rows < g_cap){
      g_rows++;
      FILE* f=rf();
      if(f){
        const char* sql = p_sql? p_sql(stmt): "";
        // SQL 指纹(前60字符, 去表别名噪音)
        fputc('\t',f);
        if(sql){ int L=0; while(sql[L]&&L<60)L++; emit(f,sql,L);} 
        int nc = p_ccount? p_ccount(stmt):0;
        for(int i=0;i<nc && i<40;i++){
          fputc('\t',f);
          const char* nm = p_cname? p_cname(stmt,i): "";
          if(nm){ int L=0;while(nm[L]&&L<40)L++; emit(f,nm,L);} 
          fputc('=',f);
          int ty = p_ctype? p_ctype(stmt,i):5;
          if(ty==1){ fprintf(f,"%lld", p_cint? p_cint(stmt,i):0); }
          else if(ty==2){ fprintf(f,"%g", p_cdouble? p_cdouble(stmt,i):0); }
          else if(ty==3){ const unsigned char* t=p_ctext?p_ctext(stmt,i):0; int b=p_cbytes?p_cbytes(stmt,i):0; if(t){int L=b>400?400:b; emit(f,(const char*)t,L);} }
          else if(ty==4){ int b=p_cbytes?p_cbytes(stmt,i):0; fprintf(f,"<blob %d>",b); }
          else fprintf(f,"NULL");
        }
        fputc('\n',f); 
        if((g_rows & 0x3ff)==0) fflush(f);
      }
    }
    pthread_mutex_unlock(&mu);
  }
  return r;
}

extern "C" int sqlite3_open_v2(const char* fn,void** db,int flags,const char* vfs){
  init_syms();
  typedef int(*fn_t)(const char*,void**,int,const char*);
  static fn_t real=(fn_t)S("sqlite3_open_v2");
  int r = real? real(fn,db,flags,vfs): 1;
  FILE* f=lf(); if(f&&fn){ fprintf(f,"[open] rc=%d %s\n",r,fn); fflush(f);}
  return r;
}
__attribute__((constructor)) static void on_load(){ FILE* f=lf(); if(f){fprintf(f,"=== sqlhook loaded pid=%d ===\n",getpid());fflush(f);} }
