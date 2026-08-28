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
 *   5. *Optionally* export `const CodeVarList *code_module_vars(void)`,
 *      returning the module's exported values (constants) — see
 *      `CodeVarList` below. A module that does not export it simply has no
 *      exported variables; `link "x.so" as x` then binds `x` to an empty
 *      object. This is optional (not a required symbol) so that a Phase 1
 *      module — handlers only — keeps working unchanged, and so the ABI
 *      version does not have to bump for it.
 *   6. *Optionally* export `void code_module_set_inbound(void *queue,
 *      CodeEmitFn emit)`. The host calls it once at link time to hand the
 *      module a queue and the function that pushes onto it; the module
 *      keeps both and may call `emit(queue, &particle)` to speak *first*,
 *      rather than only answering a dispatch. Optional for the same reason
 *      as `code_module_vars`: a module that never initiates simply doesn't
 *      export it, and the ABI version doesn't move.
 *
 * Why `emit` is a function *pointer* the host supplies, rather than a
 * `code_emit_inbound` a module could call directly: a `.so` carries its own
 * copy of this runtime (see below), so a direct call would push onto the
 * module's own queue, which the host never reads. The pointer is the host's.
 * A queued particle is deep-copied into the host's heap by that function,
 * the same boundary rule a dispatch result follows, so the module may
 * release its own copy the moment `emit` returns.
 *
 * Queued particles are dispatched to the *program's* handlers — a `.code`
 * `ClassName { ... } => { ... }` — not back into the module. That is what
 * makes an event loop expressible: the module supplies events, the program
 * decides what they mean.
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

/* What `code_module_set_inbound` hands a module: the host's own pusher.
 * `queue` is opaque to the module — it only ever passes it straight back. */
typedef void (*CodeEmitFn)(void *queue, const CodeValue *value);

/* How many pushed particles a module may have outstanding before the oldest
 * starts being dropped. Bounded on purpose: a module that pushes faster than
 * the program drains must cost bounded memory, not unbounded. */
#define CODE_INBOUND_CAPACITY 256

/* A module's exported variables (constants) — what `code_module_vars`
 * returns. `names` and `values` are parallel arrays of `count` entries:
 * `values[i]` is the value exported under `names[i]`. `values` is strided at
 * `CODE_VALUE_SLOT_SIZE` bytes (address it through a `slot_at`-style helper,
 * never `[]`), exactly like a `CodeValue`'s own `items` buffer.
 *
 * The module owns all of this memory — the names, the value buffer, and
 * everything the values point into — and it must stay valid for the module's
 * whole lifetime (the host reads it once at `link` time and deep-copies each
 * value out, the same way it treats a `code_module_dispatch` result). The
 * host never frees any of it; it only ever calls the module's own
 * `code_release` on a *copy* it made. */
typedef struct CodeVarList {
    long long count;
    const char **names;
    CodeValue *values; /* CODE_VALUE_SLOT_SIZE stride, `count` slots */
} CodeVarList;

/* ---- `.a` static modules — a different, simpler contract -----------------
 *
 * Everything above is the `.so` contract: a module `dlopen`s in as a
 * self-contained unit with its own copy of the runtime, so values crossing
 * the boundary need a deep copy and the module needs its own `code_release`.
 *
 * A `.a` module is linked straight into the same binary as the host by `cc`
 * (`code build` only — there is no `dlopen` for a static archive, so `code
 * run` refuses to link one at all). That means there is only ever one copy
 * of the runtime in the final program — the host's — so a `.a` module does
 * NOT bring its own copy of it. It builds its results by calling the host's
 * own constructors directly, declared `extern` below, and needs no
 * `code_release` of its own: nothing it builds is ever a separate
 * allocation to free.
 *
 * The only names a `.a` module must still choose carefully are the ones it
 * defines itself — `code_module_dispatch` and `code_module_abi_version`
 * (required), `code_module_vars` (optional) — because unlike `.so` handles,
 * every `.a` linked into one program shares a single flat symbol table.
 * Convention: pick a prefix unique among every `.a` your program will ever
 * link alongside, and name them `<prefix>_code_module_dispatch`,
 * `<prefix>_code_module_abi_version`, `<prefix>_code_module_vars`. `code
 * build` finds them by running `nm` on the archive at link time (see
 * `loader.rs`), so `link "libfoo.a" as m` needs no syntax to name the
 * prefix — it just has to be unique. */

void code_number(CodeValue *out, double n);
/* Borrows `s` — the value keeps the pointer rather than the bytes, so `s`
 * must outlive it. Correct for a string literal, wrong for anything built at
 * runtime; use `code_str_owned` for those. */
void code_str(CodeValue *out, const char *s);
/* Copies `s`'s bytes into a heap-owned string. What a handler returning a
 * message built into a stack buffer must use — handing that buffer to
 * `code_str` leaves a dangling read the moment the handler returns. */
void code_str_owned(CodeValue *out, const char *s);
void code_bool(CodeValue *out, int b);
void code_null(CodeValue *out);
void code_array(CodeValue *out, void *items, long long len);
void code_object(CodeValue *out, const char **keys, void *values, long long len);
void code_copy(CodeValue *out, const CodeValue *src);
void code_field(CodeValue *out, const CodeValue *obj, const char *field);
void code_index(CodeValue *out, const CodeValue *arr, const CodeValue *index);
void code_retain(const CodeValue *v);
void code_release(CodeValue *v);
int code_values_equal(const CodeValue *a, const CodeValue *b);
int code_bool_value(const CodeValue *v, const char *op);
int code_is_particle(const CodeValue *a, const char *name);
/* Builds `Exception { source, message, innerException }` — how a module
 * reports that it could not do the work. `inner` may be NULL. A module may
 * never end the application, so this replaces `code_runtime_error` for
 * everything a module can encounter; see docs/todo/errors-as-particles.md.
 * `source` and `message` are both copied. */
void code_make_exception(CodeValue *out, const char *source, const char *message,
                         const CodeValue *inner);
void code_assert(const CodeValue *v);
_Noreturn void code_runtime_error(const char *message);

/* Addresses slot `index` of a `CODE_VALUE_SLOT_SIZE`-strided buffer — the
 * same convention `items`/`values` buffers use throughout this header.
 * `static inline` rather than declared `extern`: it's pure pointer
 * arithmetic with no state, so a header-only copy in each translation unit
 * that includes this file (host and every `.a` module alike) is simpler
 * than giving it one more externally-linked name to keep unique. */
static inline CodeValue *code_slot_at(void *base, long long index) {
    return (CodeValue *)((char *)base + index * CODE_VALUE_SLOT_SIZE);
}

#endif
