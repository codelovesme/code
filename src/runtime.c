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

/* Mirrors what `code run` does on an interpreter `Err(String)`
 * (src/main.rs: `eprintln!("error: {e}"); ExitCode::FAILURE`) — operand
 * types are only known once the program is actually running, so a type
 * mismatch/division-by-zero can only ever be caught here, not at compile
 * time (unlike `verify_defined`'s undefined-variable check). */
_Noreturn static void code_runtime_error(const char *message) {
    fprintf(stderr, "error: %s\n", message);
    exit(1);
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

/* Invalid access (non-object, missing field / non-array, bad index) writes
 * CODE_NULL into `out` rather than the caller ever seeing an error —
 * decided 2026-08-21, permissive like JS. Must match
 * interpreter.rs's `Expr::Field`/`Expr::Index` eval rules exactly. */
void code_field(CodeValue *out, const CodeValue *obj, const char *field) {
    if (obj->tag == CODE_OBJECT) {
        for (long long i = 0; i < obj->len; i++) {
            if (strcmp(obj->keys[i], field) == 0) {
                *out = *slot_at(obj->items, i);
                return;
            }
        }
    }
    code_null(out);
}

void code_index(CodeValue *out, const CodeValue *arr, const CodeValue *index) {
    if (arr->tag == CODE_ARRAY && index->tag == CODE_NUMBER) {
        double n = index->number;
        long long i = (long long)n;
        if ((double)i == n && i >= 0 && i < arr->len) {
            *out = *slot_at(arr->items, i);
            return;
        }
    }
    code_null(out);
}

/* Operand-type rules below must match ast.rs's `BinOp`/`UnOp` doc comment
 * and interpreter.rs's `apply_binop`/`eval` exactly — this is the compiled
 * side of the same decisions, not an independent design. */

void code_add(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        code_number(out, a->number + b->number);
        return;
    }
    if (a->tag == CODE_STR && b->tag == CODE_STR) {
        /* First heap allocation in this runtime — string concatenation
         * produces a new, dynamically-sized value that can't live on
         * `main`'s stack the way every other slot does (see codegen.rs's
         * VALUE_SIZE comment: those are all statically bounded by program
         * size; this isn't). Never freed, same "cosmetic for a short-lived
         * exe, OS reclaims at exit" reasoning as the rest of this runtime
         * — see memory `new-code-memory-management`. */
        size_t la = strlen(a->str);
        size_t lb = strlen(b->str);
        char *buf = malloc(la + lb + 1);
        if (!buf) {
            code_runtime_error("out of memory");
        }
        memcpy(buf, a->str, la);
        memcpy(buf + la, b->str, lb);
        buf[la + lb] = '\0';
        code_str(out, buf);
        return;
    }
    if (a->tag == CODE_ARRAY && b->tag == CODE_ARRAY) {
        long long na = a->len, nb = b->len;
        void *buf = malloc((size_t)(na + nb) * CODE_VALUE_SLOT_SIZE);
        if (!buf) {
            code_runtime_error("out of memory");
        }
        for (long long i = 0; i < na; i++) {
            *slot_at(buf, i) = *slot_at(a->items, i);
        }
        for (long long i = 0; i < nb; i++) {
            *slot_at(buf, na + i) = *slot_at(b->items, i);
        }
        code_array(out, buf, na + nb);
        return;
    }
    code_runtime_error("cannot apply '+' to these values");
}

void code_sub(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        code_number(out, a->number - b->number);
        return;
    }
    code_runtime_error("cannot apply '-' to these values");
}

void code_mul(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        code_number(out, a->number * b->number);
        return;
    }
    code_runtime_error("cannot apply '*' to these values");
}

void code_div(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        if (b->number == 0.0) {
            /* Not Infinity: the value model is JSON, which has no way to
             * represent that (see ast.rs's BinOp doc comment). */
            code_runtime_error("division by zero");
        }
        code_number(out, a->number / b->number);
        return;
    }
    code_runtime_error("cannot apply '/' to these values");
}

/* -1/0/1 for orderable pairs (Number-Number, Str-Str); aborts for anything
 * else. codegen.rs turns the result into `<`/`>`/`<=`/`>=` with a plain
 * LLVM icmp against 0 — one runtime function instead of four. */
long long code_compare(const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        if (a->number < b->number) {
            return -1;
        }
        return a->number > b->number ? 1 : 0;
    }
    if (a->tag == CODE_STR && b->tag == CODE_STR) {
        int c = strcmp(a->str, b->str);
        return c < 0 ? -1 : (c > 0 ? 1 : 0);
    }
    code_runtime_error("cannot order these values");
}

void code_neg(CodeValue *out, const CodeValue *a) {
    if (a->tag == CODE_NUMBER) {
        code_number(out, -a->number);
        return;
    }
    code_runtime_error("cannot negate this value");
}

void code_not(CodeValue *out, const CodeValue *a) {
    if (a->tag == CODE_BOOL) {
        code_bool(out, !a->boolean);
        return;
    }
    code_runtime_error("'not' requires a boolean");
}

/* Used by `and`/`or` codegen to check each operand is actually a bool
 * before branching on it. */
int code_bool_value(const CodeValue *v, const char *op) {
    if (v->tag != CODE_BOOL) {
        char msg[64];
        snprintf(msg, sizeof msg, "'%s' requires booleans", op);
        code_runtime_error(msg);
    }
    return v->boolean;
}

/* Deep structural equality, matching Rust's derived `PartialEq` on `Value`
 * exactly — including that it's positional for CODE_OBJECT (same keys in
 * the same order), not a same-set-of-pairs comparison. Used for `==`/`!=`,
 * which (unlike every other operator here) are well-defined for *any* two
 * values, including mismatched kinds — never calls code_runtime_error. */
int code_values_equal(const CodeValue *a, const CodeValue *b) {
    if (a->tag != b->tag) {
        return 0;
    }
    switch (a->tag) {
    case CODE_NUMBER:
        return a->number == b->number;
    case CODE_STR:
        return strcmp(a->str, b->str) == 0;
    case CODE_BOOL:
        return a->boolean == b->boolean;
    case CODE_NULL:
        return 1;
    case CODE_ARRAY:
        if (a->len != b->len) {
            return 0;
        }
        for (long long i = 0; i < a->len; i++) {
            if (!code_values_equal(slot_at(a->items, i), slot_at(b->items, i))) {
                return 0;
            }
        }
        return 1;
    case CODE_OBJECT:
        if (a->len != b->len) {
            return 0;
        }
        for (long long i = 0; i < a->len; i++) {
            if (strcmp(a->keys[i], b->keys[i]) != 0) {
                return 0;
            }
            if (!code_values_equal(slot_at(a->items, i), slot_at(b->items, i))) {
                return 0;
            }
        }
        return 1;
    }
    return 0;
}

/* Silent on success (no output, no return value). Must match
 * interpreter.rs's `Stmt::Assert` eval rule exactly: `v` must be
 * CODE_BOOL, and its value must be true — anything else aborts via
 * code_runtime_error, same as every other operator error here. */
void code_assert(const CodeValue *v) {
    if (v->tag != CODE_BOOL) {
        code_runtime_error("assert requires a boolean");
    }
    if (!v->boolean) {
        code_runtime_error("assertion failed");
    }
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
