#ifndef CODE_WASM_SHIM_H
#define CODE_WASM_SHIM_H

/* Small libc-shaped surface used by runtime.c when building for
 * wasm32-unknown-unknown. The host supplies only the clock and error sink;
 * everything else is local to the wasm module. */
#include <stdarg.h>

typedef unsigned int size_t;
typedef long long time_t;

#define NULL ((void *)0)

extern void code_host_error(const char *ptr, unsigned int len);
extern double code_host_now(void);
/* Minutes to add to UTC to get the page's own clock. The host flips
 * JavaScript's sign so every backend answers this the same way round. */
extern double code_host_tz_offset(void);

/* Turning a double back into text needs two things this environment cannot
 * compute for itself: the *exact* decimal expansion of a double, and reading
 * a decimal string back to the nearest double. Both are the hard parts of a
 * C library's float formatting — exact arithmetic over hundreds of digits —
 * and both are one built-in call in any plausible host.
 *
 * The algorithm that uses them (`text_push_number` in runtime.c) is the same
 * on every target; only where these two answers come from differs. That is
 * deliberate: the rounding rule that has to match Rust's `Display` lives in
 * one place rather than being reimplemented per host.
 *
 * `code_host_number_exact` writes what `printf("%.40e", value)` writes and
 * returns its length; in JavaScript that is `value.toExponential(40)` (the
 * exponent's zero-padding does not matter, it is parsed as a number).
 * `code_host_number_parse` is `strtod`; in JavaScript, `Number(text)`. */
extern int code_host_number_exact(double value, char *out, unsigned int cap);
extern double code_host_number_parse(const char *ptr, unsigned int len);

/* ---- The heap ------------------------------------------------------------
 *
 * A freestanding build brings its own allocator, and the first one was a
 * bump pointer over a fixed 16 MB array whose `free` did nothing. Every
 * redraw, every answer, every keystroke took a little of it for good, and a
 * photograph took a few megabytes in one go — so a page died of "out of
 * wasm memory" after a while, or at once when handed something large. The
 * runtime releases what it makes (`code_release`, and a leak check in its
 * tests), so it only needed a `free` that meant it.
 *
 * This one is a first-fit free list over memory the module grows as it
 * needs: blocks in address order, each with a header, split when a fit is
 * larger than asked, joined with a free neighbour on either side when let
 * go. Not fast, and not meant to be — a page's allocations are a handler's
 * worth at a time — but nothing is lost and the heap is as big as the
 * browser allows, which is what a photograph needs. */

typedef struct CodeWasmBlock {
    size_t size;                  /* payload bytes, a multiple of 8 */
    int free;
    struct CodeWasmBlock *prev;   /* by address */
    struct CodeWasmBlock *next;
} CodeWasmBlock;

#define CODE_WASM_PAGE 65536u
#define CODE_WASM_HEADER ((sizeof(CodeWasmBlock) + 7u) & ~7u)

static CodeWasmBlock *code_wasm_first;   /* lowest block, or NULL */
static CodeWasmBlock *code_wasm_last;    /* highest block, or NULL */
static unsigned char *code_wasm_heap_end; /* one past the memory grown */

/* Grows the module's memory by at least `bytes` and answers where the new
 * region starts, or NULL when the browser refused. */
static unsigned char *code_wasm_grow(size_t bytes) {
    size_t pages = (bytes + CODE_WASM_PAGE - 1) / CODE_WASM_PAGE;
    if (pages < 16) {
        pages = 16;   /* a megabyte at a time, so a small program grows twice, not two hundred times */
    }
    long before = __builtin_wasm_memory_grow(0, pages);
    if (before < 0) {
        return NULL;
    }
    return (unsigned char *)((size_t)before * CODE_WASM_PAGE);
}

static void code_wasm_split(CodeWasmBlock *block, size_t size) {
    if (block->size < size + CODE_WASM_HEADER + 8u) {
        return;   /* the remainder could not hold a block of its own */
    }
    CodeWasmBlock *rest = (CodeWasmBlock *)((unsigned char *)(block + 0) + CODE_WASM_HEADER + size);
    rest->size = block->size - size - CODE_WASM_HEADER;
    rest->free = 1;
    rest->prev = block;
    rest->next = block->next;
    if (rest->next) {
        rest->next->prev = rest;
    } else {
        code_wasm_last = rest;
    }
    block->next = rest;
    block->size = size;
}

static void *malloc(size_t bytes) {
    size_t size = (bytes + 7u) & ~7u;
    if (size == 0) {
        size = 8;
    }
    for (CodeWasmBlock *b = code_wasm_first; b; b = b->next) {
        if (b->free && b->size >= size) {
            code_wasm_split(b, size);
            b->free = 0;
            return (unsigned char *)b + CODE_WASM_HEADER;
        }
    }
    /* Nothing fits: more memory, as one new block on the end — joined to
     * the last block when that one is free and adjacent, so a heap grown
     * in steps does not end in a row of pieces. */
    unsigned char *at = code_wasm_grow(size + CODE_WASM_HEADER);
    if (!at) {
        code_host_error("out of wasm memory", 18);
        __builtin_trap();
    }
    size_t got = (size_t)(__builtin_wasm_memory_size(0)) * CODE_WASM_PAGE - (size_t)at;
    if (code_wasm_last && code_wasm_last->free && code_wasm_heap_end == at) {
        code_wasm_last->size += got;
        code_wasm_heap_end = at + got;
        CodeWasmBlock *b = code_wasm_last;
        code_wasm_split(b, size);
        b->free = 0;
        return (unsigned char *)b + CODE_WASM_HEADER;
    }
    CodeWasmBlock *b = (CodeWasmBlock *)at;
    b->size = got - CODE_WASM_HEADER;
    b->free = 0;
    b->prev = code_wasm_last;
    b->next = NULL;
    if (code_wasm_last) {
        code_wasm_last->next = b;
    } else {
        code_wasm_first = b;
    }
    code_wasm_last = b;
    code_wasm_heap_end = at + got;
    code_wasm_split(b, size);
    return (unsigned char *)b + CODE_WASM_HEADER;
}

static void *calloc(size_t count, size_t bytes) {
    size_t total = count * bytes;
    unsigned char *result = malloc(total);
    for (size_t i = 0; i < total; i++) {
        result[i] = 0;
    }
    return result;
}

static void free(void *ptr) {
    if (!ptr) {
        return;
    }
    CodeWasmBlock *b = (CodeWasmBlock *)((unsigned char *)ptr - CODE_WASM_HEADER);
    b->free = 1;
    /* Join with the next block when it is free and touches this one. */
    CodeWasmBlock *n = b->next;
    if (n && n->free && (unsigned char *)b + CODE_WASM_HEADER + b->size == (unsigned char *)n) {
        b->size += CODE_WASM_HEADER + n->size;
        b->next = n->next;
        if (b->next) {
            b->next->prev = b;
        } else {
            code_wasm_last = b;
        }
    }
    /* And with the one before, the same way. */
    CodeWasmBlock *p = b->prev;
    if (p && p->free && (unsigned char *)p + CODE_WASM_HEADER + p->size == (unsigned char *)b) {
        p->size += CODE_WASM_HEADER + b->size;
        p->next = b->next;
        if (p->next) {
            p->next->prev = p;
        } else {
            code_wasm_last = p;
        }
    }
}

static void *realloc(void *old, size_t bytes) {
    if (!old) {
        return malloc(bytes);
    }
    CodeWasmBlock *b = (CodeWasmBlock *)((unsigned char *)old - CODE_WASM_HEADER);
    if (b->size >= bytes) {
        return old;
    }
    unsigned char *result = malloc(bytes);
    unsigned char *source = old;
    for (size_t i = 0; i < b->size; i++) {
        result[i] = source[i];
    }
    free(old);
    return result;
}

static void *memcpy(void *dest, const void *source, size_t count) {
    unsigned char *d = dest;
    const unsigned char *s = source;
    for (size_t i = 0; i < count; i++) {
        d[i] = s[i];
    }
    return dest;
}

static void *memmove(void *dest, const void *source, size_t count) {
    unsigned char *d = dest;
    const unsigned char *s = source;
    if (d < s) {
        for (size_t i = 0; i < count; i++) {
            d[i] = s[i];
        }
    } else {
        for (size_t i = count; i > 0; i--) {
            d[i - 1] = s[i - 1];
        }
    }
    return dest;
}

void *memset(void *dest, int value, size_t count) {
    unsigned char *d = dest;
    for (size_t i = 0; i < count; i++) {
        d[i] = (unsigned char)value;
    }
    return dest;
}

static size_t strlen(const char *text) {
    size_t length = 0;
    while (text[length]) {
        length++;
    }
    return length;
}

static int strcmp(const char *left, const char *right) {
    while (*left && *left == *right) {
        left++;
        right++;
    }
    return (unsigned char)*left - (unsigned char)*right;
}

/* Base 10 only, which is the only base runtime.c asks for — it reads the
 * exponent out of an expansion this shim's own `snprintf` never produced. */
static long strtol(const char *text, char **end, int base) {
    (void)base;
    const char *p = text;
    while (*p == ' ' || *p == '\t' || *p == '\n') {
        p++;
    }
    int negative = 0;
    if (*p == '+' || *p == '-') {
        negative = (*p == '-');
        p++;
    }
    long value = 0;
    while (*p >= '0' && *p <= '9') {
        value = value * 10 + (*p - '0');
        p++;
    }
    if (end) {
        *end = (char *)p;
    }
    return negative ? -value : value;
}

static void code_wasm_append(char *out, size_t limit, size_t *used, char ch) {
    if (*used + 1 < limit) {
        out[*used] = ch;
    }
    (*used)++;
}

static void code_wasm_append_text(char *out, size_t limit, size_t *used,
                                  const char *text) {
    while (*text) {
        code_wasm_append(out, limit, used, *text++);
    }
}

static void code_wasm_append_unsigned(char *out, size_t limit, size_t *used,
                                      unsigned long long value) {
    char digits[32];
    size_t count = 0;
    do {
        digits[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value);
    while (count) {
        code_wasm_append(out, limit, used, digits[--count]);
    }
}

static int snprintf(char *out, size_t limit, const char *format, ...) {
    va_list args;
    size_t used = 0;
    va_start(args, format);
    while (*format) {
        if (*format != '%') {
            code_wasm_append(out, limit, &used, *format++);
            continue;
        }
        format++;
        if (*format == '%') {
            code_wasm_append(out, limit, &used, *format++);
        } else if (*format == 's') {
            code_wasm_append_text(out, limit, &used, va_arg(args, const char *));
            format++;
        } else if (*format == 'u') {
            code_wasm_append_unsigned(out, limit, &used,
                                      (unsigned long long)va_arg(args, unsigned int));
            format++;
        } else if (*format == 'd') {
            int value = va_arg(args, int);
            unsigned long long magnitude = (unsigned long long)value;
            if (value < 0) {
                code_wasm_append(out, limit, &used, '-');
                /* Through unsigned, so the most negative int has no negation
                 * to overflow. */
                magnitude = -(unsigned long long)value;
            }
            code_wasm_append_unsigned(out, limit, &used, magnitude);
            format++;
        } else if (*format == 'l' && format[1] == 'l' && format[2] == 'd') {
            long long value = va_arg(args, long long);
            if (value < 0) {
                code_wasm_append(out, limit, &used, '-');
                value = -value;
            }
            code_wasm_append_unsigned(out, limit, &used, (unsigned long long)value);
            format += 3;
        } else {
            code_wasm_append_text(out, limit, &used, "<format>");
            while (*format && *format != 's' && *format != 'u' && *format != 'd') {
                format++;
            }
        }
    }
    if (limit) {
        out[used < limit ? used : limit - 1] = '\0';
    }
    va_end(args);
    return (int)used;
}

static char *getenv(const char *name) {
    (void)name;
    return NULL;
}

static time_t time(time_t *result) {
    double now = code_host_now();
    time_t seconds = (time_t)now;
    if (result) {
        *result = seconds;
    }
    return seconds;
}

#endif