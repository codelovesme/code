/* Binary trees: a complete tree of depth 12 built bottom-up, walked with an
 * explicit stack to count its nodes, ten times — the same steps as
 * binary_trees.code. */
#include <stdio.h>
#include <stdlib.h>
typedef struct Node { struct Node *l, *r; } Node;
static void release(Node *n) {
    Node **stack = malloc(64 * sizeof *stack); int top = 0;
    stack[top++] = n;
    while (top) { Node *x = stack[--top]; if (x->l) { stack[top++] = x->l; stack[top++] = x->r; } free(x); }
    free(stack);
}
int main(void) {
    const int depth = 12, reps = 10;
    long leaves = 1L << depth, total = 0;
    for (int rep = 0; rep < reps; rep++) {
        Node **level = malloc(leaves * sizeof *level);
        long width = leaves;
        for (long i = 0; i < width; i++) { level[i] = malloc(sizeof(Node)); level[i]->l = level[i]->r = NULL; }
        while (width > 1) {
            for (long j = 0; j < width; j += 2) {
                Node *n = malloc(sizeof(Node)); n->l = level[j]; n->r = level[j + 1]; level[j / 2] = n;
            }
            width /= 2;
        }
        Node *root = level[0];
        free(level);
        Node **stack = malloc(64 * sizeof *stack); int top = 0;
        stack[top++] = root;
        while (top) { Node *n = stack[--top]; total++; if (n->l) { stack[top++] = n->l; stack[top++] = n->r; } }
        free(stack);
        release(root);
    }
    if (total != reps * (leaves * 2 - 1)) return 1;
    printf("%ld\n", total);
    return 0;
}
