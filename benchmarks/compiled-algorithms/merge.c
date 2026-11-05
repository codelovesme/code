#include <assert.h>
#include <stdlib.h>
int main(void) {
    const size_t n=__SIZE__;
    double *values=malloc(n*sizeof(*values)), *scratch=malloc(n*sizeof(*scratch));
    assert(values && scratch);
    size_t seed=17;
    for (size_t i=0; i<n; i++) {
        values[i]=seed; scratch[i]=0;
        seed+=8191; if (seed>=n) seed-=n;
    }
    for (size_t width=1; width<n; width*=2) {
        for (size_t left=0; left<n; left+=2*width) {
            size_t mid=left+width; if (mid>n) mid=n;
            size_t right=left+2*width; if (right>n) right=n;
            size_t a=left, b=mid, out=left;
            while (a<mid && b<right) {
                if (values[a]<=values[b]) scratch[out++]=values[a++];
                else scratch[out++]=values[b++];
            }
            while (a<mid) scratch[out++]=values[a++];
            while (b<right) scratch[out++]=values[b++];
        }
        double *swap=values; values=scratch; scratch=swap;
    }
    for (size_t i=0; i<n; i++) assert(values[i]==(double)i);
    free(scratch); free(values);
}
