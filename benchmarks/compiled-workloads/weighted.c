#include <stdlib.h>
#include <stddef.h>
typedef struct { double x, y, z; } Record;
int main(void) {
    const size_t n = __SIZE__;
    Record *values = malloc(n * sizeof(*values));
    if (!values) return 2;
    double state = 17.0;
    for (size_t i = 0; i < n; ++i) {
        values[i] = (Record){state, 1024.0 - state, state * 2.0};
        state += 17.0;
        if (state >= 1024.0) state -= 1024.0;
    }
    double result = 0.0;
    for (size_t round = 0; round < 16; ++round) {
        double answer = 0.0;
        for (size_t i = 0; i < n; ++i) {
            Record v = values[i];
            answer = answer + v.x * ((double)round + 1.0) + v.y * 3.0 - v.z * 2.0;
        }
        result += answer;
    }
    free(values);
    return result == __EXPECTED__.0 ? 0 : 1;
}
