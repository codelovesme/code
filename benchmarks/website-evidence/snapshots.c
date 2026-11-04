/* Same checked matrices as snapshots.code. -DCOW=1 shares both container
 * levels with explicit reference counts; default eagerly deep-copies. */
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#ifndef COW
#define COW 0
#endif
#define ROWS 128
#define COLS 4096
#define VERSIONS 64
typedef struct { size_t refs; double *cells; } Row;
typedef struct { size_t refs; Row *rows[ROWS]; } Matrix;
static void *allocate(size_t n) { void *p = malloc(n); if (!p) abort(); return p; }
static Row *row_new(void) {
    Row *r = allocate(sizeof *r); r->refs = 1;
    r->cells = allocate(COLS * sizeof(double)); return r;
}
static void row_release(Row *r) { if (--r->refs == 0) { free(r->cells); free(r); } }
static void release(Matrix *m) {
    if (--m->refs == 0) {
        for (int r = 0; r < ROWS; r++) row_release(m->rows[r]);
        free(m);
    }
}
static Matrix *copy(Matrix *m, int share_rows) {
    Matrix *out = allocate(sizeof *out); out->refs = 1;
    for (int r = 0; r < ROWS; r++) {
        if (share_rows) { out->rows[r] = m->rows[r]; out->rows[r]->refs++; }
        else { out->rows[r] = row_new(); memcpy(out->rows[r]->cells, m->rows[r]->cells, COLS * sizeof(double)); }
    }
    return out;
}
static void update(Matrix **slot, int s) {
    Matrix *m = *slot;
    if (m->refs > 1) { *slot = copy(m, 1); release(m); m = *slot; }
    Row *old = m->rows[s];
    if (old->refs > 1) {
        Row *next = row_new();
        memcpy(next->cells, old->cells, COLS * sizeof(double));
        row_release(old); m->rows[s] = next;
    }
    m->rows[s]->cells[s] += 1.0;
}
int main(void) {
    Matrix *current = allocate(sizeof *current); current->refs = 1;
    for (int r = 0; r < ROWS; r++) {
        current->rows[r] = row_new();
        for (int c = 0; c < COLS; c++) current->rows[r]->cells[c] = r * COLS + c;
    }
    Matrix *snapshots[VERSIONS + 1];
    for (int s = 0; s < VERSIONS; s++) {
        if (COW) {
            snapshots[s] = current; current->refs++;
            update(&current, s);
        } else {
            snapshots[s] = copy(current, 0);
            current->rows[s]->cells[s] += 1.0;
        }
    }
    snapshots[VERSIONS] = current;
    const double n = ROWS * COLS, base = n * (n - 1.0) / 2.0;
    for (int v = 0; v <= VERSIONS; v++) {
        double total = 0;
        for (int r = 0; r < ROWS; r++)
            for (int c = 0; c < COLS; c++) total += snapshots[v]->rows[r]->cells[c];
        assert(total == base + v);
    }
    for (int v = 0; v <= VERSIONS; v++) release(snapshots[v]);
    return 0;
}
