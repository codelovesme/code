#include <stdlib.h>
#include <stddef.h>
int main(void) {
    const size_t n = __SIZE__;
    double *values = malloc(n * sizeof(*values));
    if (!values) return 2;
    double state = 17.0;
    for (size_t i = 0; i < n; ++i) {
        values[i] = state;
        state += 17.0;
        if (state >= 1024.0) state -= 1024.0;
    }
    double result = 0.0;
    for (size_t round = 0; round < 16; ++round) {
        double answer = 0.0;
        for (size_t i = 0; i < n; ++i) {
            double v = values[i];
            if (v > 512.0 + (double)round) answer += v;
        }
        result += answer;
    }
    free(values);
    return result == __EXPECTED__.0 ? 0 : 1;
}
