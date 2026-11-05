#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
int main(void) {
    const size_t n=__SIZE__;
    bool *prime=malloc((n+1)*sizeof(*prime));
    assert(prime);
    for (size_t i=0; i<=n; i++) prime[i]=true;
    prime[0]=prime[1]=false;
    for (size_t p=2; p*p<=n; p++) if (prime[p])
        for (size_t multiple=p*p; multiple<=n; multiple+=p) prime[multiple]=false;
    uint64_t count=0, sum=0;
    for (size_t i=2; i<=n; i++) if (prime[i]) {count++; sum+=i;}
    assert(count==__COUNT__ && sum==__SUM__ULL);
    free(prime);
}
