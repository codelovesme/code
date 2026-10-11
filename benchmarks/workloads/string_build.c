/* String building: 20,000 numbers joined with commas, one at a time. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
    size_t cap = 16, len = 0; char *s = malloc(cap);
    for (int i = 0; i < 20000; i++) {
        char part[16]; int n = snprintf(part, sizeof part, "%d,", i);
        if (len + n + 1 > cap) { cap *= 2; s = realloc(s, cap); }
        memcpy(s + len, part, n); len += n;
    }
    s[len] = 0;
    if (strlen(s) != 108890) return 1;
    printf("%zu\n", len);
    free(s);
    return 0;
}
