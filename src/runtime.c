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
    /* 1 when this value owns one reference to a refcounted heap block (see
     * `heap_block` for which field points at it). A string *literal* points
     * into the program's read-only data instead, and an empty array/object
     * owns nothing at all — both are `heap = 0`, making retain/release a
     * no-op for them. */
    int heap;
    double number;
    const char *str;
    int boolean;
    /* CODE_ARRAY: element buffer; CODE_OBJECT: value buffer. Deliberately
     * `void *`, not `CodeValue *` — codegen.rs packs each element/value at a
     * fixed CODE_VALUE_SLOT_SIZE-byte stride (its VALUE_SIZE), NOT at
     * `sizeof(CodeValue)` (compiler/platform-dependent). Indexing this as a
     * real `CodeValue[]` would silently read the wrong slot the moment those
     * two numbers differ — always go through `slot_at()` below instead of
     * `[]`. */
    void *items;
    const char **keys; /* CODE_OBJECT only — a genuine char* array, stride sizeof(char*) */
    long long len;     /* CODE_ARRAY/CODE_OBJECT element count */
} CodeValue;

/* Must match codegen.rs's VALUE_SIZE exactly. The assert below is the only
 * thing standing between a struct that outgrows the stride and codegen
 * silently reading the wrong slot, so keep it. */
#define CODE_VALUE_SLOT_SIZE 80
_Static_assert(sizeof(CodeValue) <= CODE_VALUE_SLOT_SIZE,
               "CodeValue outgrew codegen.rs's VALUE_SIZE stride");

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

/* ---- Reference counting -------------------------------------------------
 *
 * Compound values (non-empty arrays/objects, concatenated strings) live in
 * refcounted heap blocks; every `CodeValue` slot that names one owns exactly
 * one reference to it. Plain refcounting with NO cycle collector is enough
 * here, and always will be: a cycle can only be built by mutating an
 * already-constructed value to point back at something that reaches it, and
 * this language has no mutation at all — values are only ever built bottom
 * up and read afterwards (see memory `new-code-memory-management`).
 *
 * Every reference is created and destroyed inside this file, never by
 * codegen: each constructor below releases whatever its `out` slot held
 * before overwriting it, so a slot reused across loop iterations drops the
 * previous iteration's value automatically. That is what lets codegen.rs
 * hoist all of its allocas into the entry block and reuse them — see
 * `gen_loop`'s comment for why that in turn is what keeps a long loop's
 * memory bounded by program size rather than by iteration count.
 *
 * Codegen then releases every slot as the program's last act, so a finished
 * program owns nothing at all — not because the OS wouldn't reclaim it
 * anyway, but because "owns nothing" is a property `code_check_leaks` can
 * actually test. */

typedef struct {
    long long rc;
    long long padding; /* keeps the payload 16-byte aligned, like malloc's */
} CodeHeader;

/* Blocks currently allocated. Exists only so `code_check_leaks` can turn
 * "the refcounting is correct" into something a test can actually observe —
 * without it, a missing release and a correct release produce identical
 * program output. */
static long long live_blocks = 0;

static void *heap_alloc(size_t bytes) {
    CodeHeader *h = malloc(sizeof(CodeHeader) + bytes);
    if (!h) {
        code_runtime_error("out of memory");
    }
    h->rc = 1;
    live_blocks++;
    return (char *)h + sizeof(CodeHeader);
}

static CodeHeader *header_of(const void *payload) {
    return (CodeHeader *)((char *)payload - sizeof(CodeHeader));
}

/* The single block a heap-owning value refers to. An object packs its keys
 * array and its value slots into one allocation — `keys` is the base, and
 * `items` points partway into it — so one refcount covers both. */
static void *heap_block(const CodeValue *v) {
    switch (v->tag) {
    case CODE_STR:
        return (void *)v->str;
    case CODE_ARRAY:
        return v->items;
    case CODE_OBJECT:
        return (void *)v->keys;
    default:
        return NULL;
    }
}

void code_retain(const CodeValue *v) {
    if (v->heap) {
        header_of(heap_block(v))->rc++;
    }
}

/* Does NOT clear `v->heap` afterwards: every caller overwrites the slot
 * immediately, and leaving the field alone is what makes `code_copy`'s
 * self-assignment case (`x = x`) work — see its comment. */
void code_release(CodeValue *v) {
    if (!v->heap) {
        return;
    }
    CodeHeader *h = header_of(heap_block(v));
    if (--h->rc == 0) {
        if (v->tag == CODE_ARRAY || v->tag == CODE_OBJECT) {
            for (long long i = 0; i < v->len; i++) {
                code_release(slot_at(v->items, i));
            }
        }
        free(h);
        live_blocks--;
    }
}

/* The last thing a compiled program does, after codegen has released every
 * slot it allocated. Silent unless CODE_CHECK_LEAKS is set, so it costs a
 * normal run one getenv and never changes its behaviour — the test harness
 * sets it for every fixture, which is what makes a lost reference a *failing
 * test* rather than an invisible difference. */
void code_check_leaks(void) {
    if (!getenv("CODE_CHECK_LEAKS")) {
        return;
    }
    if (live_blocks != 0) {
        char msg[96];
        snprintf(msg, sizeof msg, "%lld heap block(s) leaked", live_blocks);
        code_runtime_error(msg);
    }
}

void code_number(CodeValue *out, double n) {
    code_release(out);
    out->tag = CODE_NUMBER;
    out->heap = 0;
    out->number = n;
}

/* `s` is a string literal in the program's read-only data, so the value
 * borrows it rather than owning a block — only `code_add`'s concatenation
 * produces an owned string. */
void code_str(CodeValue *out, const char *s) {
    code_release(out);
    out->tag = CODE_STR;
    out->heap = 0;
    out->str = s;
}

void code_bool(CodeValue *out, int b) {
    code_release(out);
    out->tag = CODE_BOOL;
    out->heap = 0;
    out->boolean = b;
}

void code_null(CodeValue *out) {
    code_release(out);
    out->tag = CODE_NULL;
    out->heap = 0;
}

/* `items` is codegen's scratch buffer, not the array's storage: the elements
 * are copied (and retained) into a fresh heap block here, so the scratch
 * slots are free to be rewritten by the next iteration. An empty array owns
 * no block at all. */
void code_array(CodeValue *out, void *items, long long len) {
    void *buf = NULL;
    if (len > 0) {
        buf = heap_alloc((size_t)len * CODE_VALUE_SLOT_SIZE);
        for (long long i = 0; i < len; i++) {
            const CodeValue *src = slot_at(items, i);
            code_retain(src);
            *slot_at(buf, i) = *src;
        }
    }
    code_release(out);
    out->tag = CODE_ARRAY;
    out->heap = len > 0;
    out->items = buf;
    out->len = len;
}

/* One allocation for both arrays: `[keys...][values...]`. The key pointers
 * themselves are string literals in read-only data, so only the array of
 * pointers is copied, never the characters. */
void code_object(CodeValue *out, const char **keys, void *values, long long len) {
    const char **key_buf = NULL;
    void *value_buf = NULL;
    if (len > 0) {
        size_t keys_bytes = (size_t)len * sizeof(const char *);
        key_buf = heap_alloc(keys_bytes + (size_t)len * CODE_VALUE_SLOT_SIZE);
        value_buf = (char *)key_buf + keys_bytes;
        for (long long i = 0; i < len; i++) {
            key_buf[i] = keys[i];
            const CodeValue *src = slot_at(values, i);
            code_retain(src);
            *slot_at(value_buf, i) = *src;
        }
    }
    code_release(out);
    out->tag = CODE_OBJECT;
    out->heap = len > 0;
    out->keys = key_buf;
    out->items = value_buf;
    out->len = len;
}

/* Retain before release, never the other way round. The two can name the
 * same block — `x = x`, or overwriting a loop variable with the next element
 * of the very array the previous element came from — and releasing first
 * would drop the last reference and free the block this is about to read. */
void code_copy(CodeValue *out, const CodeValue *src) {
    code_retain(src);
    code_release(out);
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
                /* `code_copy`, not a bare struct assignment: the extracted
                 * value now lives in a second slot and so needs its own
                 * reference — otherwise `let inner = obj.k` would dangle the
                 * moment `obj` was overwritten. */
                code_copy(out, slot_at(obj->items, i));
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
            code_copy(out, slot_at(arr->items, i));
            return;
        }
    }
    code_null(out);
}

/* `loop x over <expr>` support. Two calls instead of one combined "iterate"
 * entry point because the loop's control flow lives in the generated IR, not
 * here: codegen emits the counter, the bounds check and the back-edge itself
 * (see codegen.rs's `gen_loop`), and only calls into the runtime for the two
 * things that need to inspect a `CodeValue`. Must match interpreter.rs's
 * `Stmt::Loop` eval rule: the iterable must be an array — anything else
 * aborts rather than iterating zero times. */
long long code_iter_len(const CodeValue *v) {
    if (v->tag != CODE_ARRAY) {
        code_runtime_error("loop requires an array");
    }
    return v->len;
}

/* `i` is always in range: the only caller is the loop header codegen emits,
 * which already compared it against `code_iter_len`'s result. */
void code_iter_at(CodeValue *out, const CodeValue *arr, long long i) {
    code_copy(out, slot_at(arr->items, i));
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
        /* Unlike `code_str`'s literal, a concatenation result is a value
         * this runtime owns, so it gets a refcounted block. Built before
         * `out` is released, because `out` may be one of the operands
         * (`s = s + s`). */
        size_t la = strlen(a->str);
        size_t lb = strlen(b->str);
        char *buf = heap_alloc(la + lb + 1);
        memcpy(buf, a->str, la);
        memcpy(buf + la, b->str, lb);
        buf[la + lb] = '\0';
        code_release(out);
        out->tag = CODE_STR;
        out->heap = 1;
        out->str = buf;
        return;
    }
    if (a->tag == CODE_ARRAY && b->tag == CODE_ARRAY) {
        long long na = a->len, nb = b->len;
        long long total = na + nb;
        void *buf = NULL;
        if (total > 0) {
            buf = heap_alloc((size_t)total * CODE_VALUE_SLOT_SIZE);
            for (long long i = 0; i < na; i++) {
                const CodeValue *src = slot_at(a->items, i);
                code_retain(src);
                *slot_at(buf, i) = *src;
            }
            for (long long i = 0; i < nb; i++) {
                const CodeValue *src = slot_at(b->items, i);
                code_retain(src);
                *slot_at(buf, na + i) = *src;
            }
        }
        /* Same ordering point as the string case: the elements are already
         * retained, so releasing `out` here can't free anything `buf` now
         * refers to even when `out` was `a` or `b` (`x = x + x`). */
        code_release(out);
        out->tag = CODE_ARRAY;
        out->heap = total > 0;
        out->items = buf;
        out->len = total;
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
