// LD_PRELOAD 拦 SEE 的 sqlite3_key / sqlite3_key_v2, 捕获数据库密钥。
#include <dlfcn.h>
#include <cstdio>
#include <cstdlib>
#include <pthread.h>
#include <unistd.h>

static pthread_mutex_t mu=PTHREAD_MUTEX_INITIALIZER;
static const char* dir(){ const char* d=getenv("DT_HOOK_DIR"); return d?d:"/tmp/dt_capture"; }
static FILE* lf(){ static FILE* f=nullptr; if(!f){char p[1024];snprintf(p,sizeof p,"%s/keys.log",dir());f=fopen(p,"a");} return f; }

static void logkey(const char* fn,const char* dbname,const void* pKey,int nKey){
    pthread_mutex_lock(&mu);
    FILE* f=lf();
    if(f){
        fprintf(f,"[KEY] %s db=%s nKey=%d ",fn,dbname?dbname:"-",nKey);
        const unsigned char* k=(const unsigned char*)pKey;
        if(k && nKey>0 && nKey<=256){
            fprintf(f,"hex="); for(int i=0;i<nKey;i++) fprintf(f,"%02x",k[i]);
            fprintf(f," ascii=\""); for(int i=0;i<nKey;i++){unsigned char c=k[i];fputc((c>=32&&c<127)?c:'.',f);} fputc('"',f);
        }
        fputc('\n',f); fflush(f);
    }
    pthread_mutex_unlock(&mu);
}

extern "C" int sqlite3_key(void* db,const void* pKey,int nKey){
    typedef int(*fn)(void*,const void*,int);
    static fn real=nullptr; if(!real) real=(fn)dlsym(RTLD_NEXT,"sqlite3_key");
    logkey("sqlite3_key",nullptr,pKey,nKey);
    return real?real(db,pKey,nKey):0;
}
extern "C" int sqlite3_key_v2(void* db,const char* zDb,const void* pKey,int nKey){
    typedef int(*fn)(void*,const char*,const void*,int);
    static fn real=nullptr; if(!real) real=(fn)dlsym(RTLD_NEXT,"sqlite3_key_v2");
    logkey("sqlite3_key_v2",zDb,pKey,nKey);
    return real?real(db,zDb,pKey,nKey):0;
}
__attribute__((constructor)) static void on_load(){ FILE* f=lf(); if(f){fprintf(f,"=== keyhook loaded pid=%d ===\n",getpid());fflush(f);} }
