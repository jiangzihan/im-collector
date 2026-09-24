// LD_PRELOAD 只拦 Util::Encry::decryptDataAES(密文,密钥)->明文, 透明转发+捕获。
#include <dlfcn.h>
#include <string>
#include <cstdio>
#include <cstdlib>
#include <pthread.h>
#include <unistd.h>

static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static long g_n = 0;
static const char* dir(){ const char* d=getenv("DT_HOOK_DIR"); return d?d:"/tmp/dt_capture"; }
static FILE* logf(){ static FILE* f=nullptr; if(!f){char p[1024];snprintf(p,sizeof p,"%s/hook.log",dir());f=fopen(p,"a");} return f; }
static void dump(const char* suff,long n,const void* buf,size_t len){ if(!buf||!len||len>32*1024*1024)return; char p[1024];snprintf(p,sizeof p,"%s/c%06ld_%s.bin",dir(),n,suff);FILE* f=fopen(p,"wb");if(f){fwrite(buf,1,len,f);fclose(f);} }

namespace Util { namespace Encry {
  std::string decryptDataAES(const std::string& in, const std::string& key){
    typedef std::string (*fn)(const std::string&, const std::string&);
    static fn real=nullptr;
    if(!real) real=(fn)dlsym(RTLD_NEXT,"_ZN4Util5Encry14decryptDataAESERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEES8_");
    std::string out = real ? real(in,key) : std::string();
    pthread_mutex_lock(&g_mu);
    long n=g_n++;
    FILE* f=logf();
    if(f){
      fprintf(f,"[%ld] decryptDataAES inlen=%zu outlen=%zu keylen=%zu key=",n,in.size(),out.size(),key.size());
      for(size_t i=0;i<key.size()&&i<64;i++) fprintf(f,"%02x",(unsigned char)key[i]);
      fputc('\n',f); fflush(f);
    }
    dump("in", n, in.data(), in.size());
    dump("out",n, out.data(), out.size());
    pthread_mutex_unlock(&g_mu);
    return out;
  }
}}

__attribute__((constructor)) static void on_load(){ FILE* f=logf(); if(f){fprintf(f,"=== ahhook(decryptDataAES) loaded pid=%d ===\n",getpid());fflush(f);} }
