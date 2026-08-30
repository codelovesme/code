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

static unsigned char code_wasm_heap[16 * 1024 * 1024];
static size_t code_wasm_heap_used;

typedef struct {
    size_t size;
} CodeWasmAlloc;

static void *malloc(size_t bytes) {
    size_t aligned = (bytes + 7u) & ~7u;
    size_t start = (code_wasm_heap_used + 7u) & ~7u;
    if (aligned > sizeof(code_wasm_heap) - start - sizeof(CodeWasmAlloc)) {
        code_host_error("out of wasm memory", 18);
        __builtin_trap();
    }
    CodeWasmAlloc *allocation = (CodeWasmAlloc *)(code_wasm_heap + start);
    allocation->size = bytes;
    void *result = allocation + 1;
    code_wasm_heap_used = start + sizeof(CodeWasmAlloc) + aligned;
    return result;
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
    (void)ptr;
}

static void *realloc(void *old, size_t bytes) {
    unsigned char *result = malloc(bytes);
    if (old) {
        CodeWasmAlloc *allocation = ((CodeWasmAlloc *)old) - 1;
        size_t copied = allocation->size < bytes ? allocation->size : bytes;
        unsigned char *source = old;
        for (size_t i = 0; i < copied; i++) {
            result[i] = source[i];
        }
    }
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