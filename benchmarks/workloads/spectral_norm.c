/* Spectral norm (Benchmarks Game), N = 1000, ten power iterations.
 * A(i,j) = 1 / ((i+j)(i+j+1)/2 + i + 1); `t` picks the transpose with the
 * same arithmetic the code version uses. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#define N 1000
static void times(const double *v, double *out, double t) {
    for (int i = 0; i < N; i++) {
        double sum = 0;
        for (int j = 0; j < N; j++) {
            double a = 1.0 / ((double)(i + j) * (i + j + 1) / 2 + i + 1 + t * (j - i));
            sum += a * v[j];
        }
        out[i] = sum;
    }
}
int main(void) {
    static double u[N], v[N], w[N];
    for (int i = 0; i < N; i++) u[i] = 1;
    for (int k = 0; k < 10; k++) {
        times(u, w, 0); times(w, v, 1);
        times(v, w, 0); times(w, u, 1);
    }
    double vbv = 0, vv = 0;
    for (int i = 0; i < N; i++) { vbv += u[i] * v[i]; vv += v[i] * v[i]; }
    double r = sqrt(vbv / vv);
    if (r < 1.274224 || r > 1.274225) return 1;
    printf("%.9f\n", r);
    return 0;
}
