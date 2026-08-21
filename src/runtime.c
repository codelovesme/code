/* Runtime support linked into every compiled program. Mirrors src/value.rs's
 * `Value` and its `Display` impl exactly, so `code build foo.code && ./foo`
 * prints byte-for-byte what `code run foo.code` prints.
 *
 * Every constructor writes into a caller-owned `CodeValue*` (rather than
 * returning by value) specifically to sidestep C-struct-by-value calling-
 * convention/ABI matching between this file and the LLVM IR that calls it —
 * codegen.rs only ever passes opaque pointers, never inspects the struct's
 * layout itself. See codegen.rs's VALUE_SIZE comment for the size contract.
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef enum { CODE_NUMBER, CODE_STR, CODE_BOOL, CODE_NULL, CODE_ARRAY, CODE_OBJECT } CodeTag;

typedef struct CodeValue {
    CodeTag tag;
    double number;
    const char *str;
    int boolean;
    /* CODE_ARRAY: element buffer; CODE_OBJECT: value buffer. Deliberately
     * `void *`, not `CodeValue *` — codegen.rs packs each element/value at a
     * fixed CODE_VALUE_SLOT_SIZE-byte stride (its VALUE_SIZE, currently 64),
     * NOT at `sizeof(CodeValue)` (56 here, but compiler/platform-dependent).
     * Indexing this as a real `CodeValue[]` would silently read the wrong
     * slot the moment those two numbers differ — always go through
     * `slot_at()` below instead of `[]`. */
    void *items;
    const char **keys; /* CODE_OBJECT only — a genuine char* array, stride sizeof(char*) */
    long long len;     /* CODE_ARRAY/CODE_OBJECT element count */
} CodeValue;

/* Must match codegen.rs's VALUE_SIZE exactly. */
#define CODE_VALUE_SLOT_SIZE 64

static CodeValue *slot_at(void *base, long long index) {
    return (CodeValue *)((char *)base + index * CODE_VALUE_SLOT_SIZE);
}

void code_number(CodeValue *out, double n) {
    out->tag = CODE_NUMBER;
    out->number = n;
}

void code_str(CodeValue *out, const char *s) {
    out->tag = CODE_STR;
    out->str = s;
}

void code_bool(CodeValue *out, int b) {
    out->tag = CODE_BOOL;
    out->boolean = b;
}

void code_null(CodeValue *out) {
    out->tag = CODE_NULL;
}

void code_array(CodeValue *out, void *items, long long len) {
    out->tag = CODE_ARRAY;
    out->items = items;
    out->len = len;
}

void code_object(CodeValue *out, const char **keys, void *values, long long len) {
    out->tag = CODE_OBJECT;
    out->keys = keys;
    out->items = values;
    out->len = len;
}

/* Shallow copy: fine because nothing in the language mutates a value in
 * place (see memory `new-code-memory-management`) — copying an Array's
 * `items`/`len` header still leaves both copies pointing at the *same*
 * element storage, which is safe precisely because that storage is never
 * written to again after construction. */
void code_copy(CodeValue *out, const CodeValue *src) {
    *out = *src;
}

/* Shortest decimal that round-trips back to `n` — matches Rust's f64
 * Display (e.g. 42.0 -> "42", 2.5 -> "2.5"), not printf's fixed-precision
 * default. */
static void format_number(double n, char *buf, size_t bufsize) {
    if (n == (double)(long long)n && fabs(n) < 1e15) {
        snprintf(buf, bufsize, "%lld", (long long)n);
        return;
    }
    for (int prec = 1; prec <= 17; prec++) {
        snprintf(buf, bufsize, "%.*g", prec, n);
        if (strtod(buf, NULL) == n) {
            return;
        }
    }
}

static void print_json_string(const char *s) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        switch (*p) {
        case '"':
            fputs("\\\"", stdout);
            break;
        case '\\':
            fputs("\\\\", stdout);
            break;
        case '\n':
            fputs("\\n", stdout);
            break;
        case '\t':
            fputs("\\t", stdout);
            break;
        default:
            putchar(*p);
        }
    }
    putchar('"');
}

static void print_json(const CodeValue *v) {
    char buf[64];
    switch (v->tag) {
    case CODE_NUMBER:
        format_number(v->number, buf, sizeof buf);
        fputs(buf, stdout);
        break;
    case CODE_STR:
        print_json_string(v->str);
        break;
    case CODE_BOOL:
        fputs(v->boolean ? "true" : "false", stdout);
        break;
    case CODE_NULL:
        fputs("null", stdout);
        break;
    case CODE_ARRAY:
        putchar('[');
        for (long long i = 0; i < v->len; i++) {
            if (i > 0) {
                putchar(',');
            }
            print_json(slot_at(v->items, i));
        }
        putchar(']');
        break;
    case CODE_OBJECT:
        putchar('{');
        for (long long i = 0; i < v->len; i++) {
            if (i > 0) {
                putchar(',');
            }
            print_json_string(v->keys[i]);
            putchar(':');
            print_json(slot_at(v->items, i));
        }
        putchar('}');
        break;
    }
}

/* Prints "name = value\n" per binding, in first-assignment order — matches
 * src/main.rs's `run` dump exactly (see memory: this is a temporary
 * observability hack, not a language design decision, on both sides). */
void code_dump_bindings(const char **names, void *values, long long count) {
    for (long long i = 0; i < count; i++) {
        fputs(names[i], stdout);
        fputs(" = ", stdout);
        print_json(slot_at(values, i));
        putchar('\n');
    }
}
