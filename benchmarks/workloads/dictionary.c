/* Dictionary: 500 counters under computed names ("k0" … "k499"), each
 * incremented 50 times — an open-addressing table of string keys. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define SLOTS 1024
static char *names[SLOTS]; static double values[SLOTS];
static unsigned long hash(const char *s) { unsigned long h = 5381; while (*s) h = h * 33 + (unsigned char)*s++; return h; }
static double *slot(const char *key) {
    unsigned long i = hash(key) & (SLOTS - 1);
    while (names[i] && strcmp(names[i], key)) i = (i + 1) & (SLOTS - 1);
    if (!names[i]) { names[i] = strdup(key); values[i] = 0; }
    return &values[i];
}
int main(void) {
    const int keys = 500, passes = 50; char name[16];
    for (int k = 0; k < keys; k++) { snprintf(name, sizeof name, "k%d", k); *slot(name) = 0; }
    for (int p = 0; p < passes; p++)
        for (int k = 0; k < keys; k++) { snprintf(name, sizeof name, "k%d", k); *slot(name) += 1; }
    double total = 0;
    for (int i = 0; i < SLOTS; i++) if (names[i]) { total += values[i]; free(names[i]); }
    if (total != keys * passes) return 1;
    printf("%.0f\n", total);
    return 0;
}
