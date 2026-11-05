#include <assert.h>
#include <stdlib.h>
int main(void) {
    const size_t n = __SIZE__;
    double *a = malloc(n*n*sizeof(*a)), *b = malloc(n*n*sizeof(*b));
    double *c = calloc(n*n, sizeof(*c));
    assert(a && b && c);
    for (size_t i=0; i<n; i++) for (size_t j=0; j<n; j++) {
        a[i*n+j] = (double)(i+j+1)/n;
        b[i*n+j] = (double)(i*2+j+1)/n;
    }
    for (size_t i=0; i<n; i++) for (size_t k=0; k<n; k++) {
        double x = a[i*n+k];
        for (size_t j=0; j<n; j++) c[i*n+j] += x*b[k*n+j];
    }
    double sum_k = (double)n*(n-1)/2, sum_k2 = (double)n*(n-1)*(2*n-1)/6;
    for (size_t i=0; i<n; i++) for (size_t j=0; j<n; j++) {
        double u=i+1, v=j+1;
        double expected=(2*sum_k2+(2*u+v)*sum_k+n*u*v)/(n*n);
        double difference=c[i*n+j]-expected;
        assert(difference > -1e-8 && difference < 1e-8);
    }
    free(c); free(b); free(a);
}
