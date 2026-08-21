/* code_abi.h — the native module ABI.
 *
 * A native module is a `.so` compiled from any language that can produce a
 * C-ABI shared library. It must:
 *
 *   1. `#include` this header (or reproduce it exactly — the layout below,
 *      once a module is compiled against it, is a wire format: changing it
 *      silently breaks every module built against the old one).
 *   2. Export `uint32_t code_module_abi_version(void)`, returning
 *      `CODE_ABI_VERSION`.
 *   3. Export `void code_module_dispatch(CodeValue *out, const CodeValue
 *      *particle)`, doing its own `_class`-field dispatch exactly like
 *      `runtime.c`'s `code_core_dispatch` — see that function for the
 *      pattern to copy.
 *   4. Export `void code_release(CodeValue *v)`, so the host can free
 *      whatever `code_module_dispatch` allocated for its result. The
 *      simplest way to get a correct one is to `#include "runtime.c"`
 *      itself (its constructors/refcounting are exactly what a handler
 *      needs to build a result) — see `tests/native_modules/` for an
 *      example.
 *
 * Why a module needs its own `code_release`, not the host's: values never
 * cross this boundary by shared ownership. Whatever a module allocates for
 * its result is deep-copied into the host's own heap immediately after the
 * call (see `code_native_dispatch` in runtime.c) using the host's own
 * refcount bookkeeping, and then the module's copy of `code_release` frees
 * what the module itself allocated. Each side's allocator only ever frees
 * blocks it itself allocated — that's what keeps `CODE_CHECK_LEAKS`
 * meaningful on both sides of a dlopen boundary, where two copies of this
 * runtime have entirely separate static state.
 *
 * Loading is dlopen/dlsym-based, in both `code run` and a `code build`
 * binary — never `cc`-time static linking. Every module exports the same
 * three symbol names, and that's fine: dlsym resolves within one module's
 * own handle, so two linked modules never collide, no matter how many
 * handlers each defines internally.
 *
 * A fatal error inside a module (its own `code_runtime_error`, a segfault,
 * anything that isn't a normal return) takes down the *host* process —
 * `code run` included, not just a `code build` binary. Unlike `core`
 * (`code_core_dispatch`), which the interpreter has its own independent
 * Rust implementation of specifically so a bad handler call there is a
 * clean `Result::Err`, a native module is real native code the interpreter
 * runs in-process via dlopen — there is no reimplementation to fall back
 * on, and no sandboxing here (out of scope for Phase 1). This is the same
 * tradeoff any native-extension mechanism makes (a Python C extension can
 * just as easily crash the interpreter that loaded it); it is not
 * considered a bug, and `docs/todo/native-module-linking.md`'s fixture
 * suite works around it by never provoking a module's fatal path in-process.
 */
#ifndef CODE_ABI_H
#define CODE_ABI_H

#include <stdint.h>

#define CODE_ABI_VERSION 1

typedef enum { CODE_NUMBER, CODE_STR, CODE_BOOL, CODE_NULL, CODE_ARRAY, CODE_OBJECT } CodeTag;

typedef struct CodeValue {
    CodeTag tag;
    int heap;
    double number;
    const char *str;
    int boolean;
    /* CODE_ARRAY: element buffer; CODE_OBJECT: value buffer — both strided
     * at CODE_VALUE_SLOT_SIZE bytes, never sizeof(CodeValue). Always address
     * through a `slot_at`-style helper, never `[]`. */
    void *items;
    const char **keys; /* CODE_OBJECT only, parallel to `items`, sizeof(char*) stride */
    long long len;      /* CODE_ARRAY/CODE_OBJECT element count */
} CodeValue;

#define CODE_VALUE_SLOT_SIZE 80

#endif
