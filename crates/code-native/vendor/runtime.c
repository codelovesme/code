/* Runtime support linked into every compiled program. Mirrors src/value.rs's
 * `Value` (the six JSON-shaped kinds) so a compiled program manipulates values
 * identically to the interpreter. Programs themselves are silent unless they
 * emit through a linked module (such as `terminal`, which writes straight to
 * stdout) — there is no bindings dump anymore, so nothing here renders values
 * for display; the only text this file produces is error messages on stderr.
 *
 * Every constructor writes into a caller-owned `CodeValue*` (rather than
 * returning by value) specifically to sidestep C-struct-by-value calling-
 * convention/ABI matching between this file and the LLVM IR that calls it —
 * codegen.rs only ever passes opaque pointers, never inspects the struct's
 * layout itself. See codegen.rs's VALUE_SIZE comment for the size contract.
 */
#define _GNU_SOURCE
#ifdef CODE_WASM
#include "wasm_shim.h"
#else
#include <dlfcn.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#endif

#include "code_abi.h"

/* `CodeTag`/`CodeValue`/`CODE_VALUE_SLOT_SIZE` now live in code_abi.h — it's
 * the native-module ABI, so runtime.c and every module built against it (see
 * that header) share one definition instead of two that could drift apart.
 *
 * Must match codegen.rs's VALUE_SIZE exactly. The assert below is the only
 * thing standing between a struct that outgrew the stride and codegen
 * silently reading the wrong slot, so keep it. */
_Static_assert(sizeof(CodeValue) <= CODE_VALUE_SLOT_SIZE,
               "CodeValue outgrew codegen.rs's VALUE_SIZE stride");

static CodeValue *slot_at(void *base, long long index) {
    return (CodeValue *)((char *)base + index * CODE_VALUE_SLOT_SIZE);
}

/* Mirrors what `code run` does on an interpreter `Err(String)`
 * (src/main.rs: `eprintln!("error: {e}"); ExitCode::FAILURE`) — operand
 * types are only known once the program is actually running, so a type
 * mismatch/division-by-zero can only ever be caught here, not at compile
 * time (unlike `verify_defined`'s undefined-variable check).
 *
 * Not `static`: a `.a` static module (see the "Native modules" section
 * below) links directly against the host's own copy of this runtime rather
 * than bringing its own, so it needs this externally visible to raise its
 * own fatal errors the same way `core`'s handlers do. */
_Noreturn void code_runtime_error(const char *message) {
#ifdef CODE_WASM
    code_host_error(message, (unsigned int)strlen(message));
    __builtin_trap();
#else
    fprintf(stderr, "error: %s\n", message);
    exit(1);
#endif
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

/* ---- Iterative traversal --------------------------------------------------
 *
 * `code_release` and `code_values_equal` both walk a value's children, and
 * both used to recurse. Nesting depth is bounded only by a loop's iteration
 * count (`loop x over xs { a = [a] }`), not by how many brackets the source
 * contains, so one stack frame per level segfaults at around 131k deep — see
 * `tests/stress_deep_nesting.code`, and `value.rs` for the interpreter's
 * equivalents, which have the same shape for the same reason. Each keeps an
 * explicit work stack in heap memory instead.
 *
 * The stacks are file-static and grow on demand, never shrinking: this is a
 * single-threaded runtime and neither can re-enter itself now that they
 * don't recurse, so one buffer each is enough. */

static void *grow(void *buf, size_t *cap, size_t needed, size_t item_size) {
    if (*cap >= needed) {
        return buf;
    }
    size_t next = *cap ? *cap * 2 : 64;
    while (next < needed) {
        next *= 2;
    }
    void *bigger = realloc(buf, next * item_size);
    if (!bigger) {
        code_runtime_error("out of memory");
    }
    *cap = next;
    return bigger;
}

static CodeValue *dead = NULL; /* values whose block is owed a free() */
static size_t dead_cap = 0;

/* Does NOT clear `v->heap` afterwards: every caller overwrites the slot
 * immediately, and leaving the field alone is what makes `code_copy`'s
 * self-assignment case (`x = x`) work — see its comment. */
void code_release(CodeValue *v) {
    if (!v->heap) {
        return;
    }
    if (--header_of(heap_block(v))->rc != 0) {
        return;
    }

    size_t len = 0;
    dead = grow(dead, &dead_cap, len + 1, sizeof(CodeValue));
    dead[len++] = *v;

    while (len > 0) {
        CodeValue current = dead[--len];
        /* Children are read out *before* the block is freed, and only the
         * ones whose own count reaches zero are queued. */
        if (current.tag == CODE_ARRAY || current.tag == CODE_OBJECT) {
            for (long long i = 0; i < current.len; i++) {
                const CodeValue *child = slot_at(current.items, i);
                if (child->heap && --header_of(heap_block(child))->rc == 0) {
                    dead = grow(dead, &dead_cap, len + 1, sizeof(CodeValue));
                    dead[len++] = *child;
                }
            }
        }
        free(header_of(heap_block(&current)));
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

/* The wrong *kind* of operand for `.`/`[]` is a runtime error; a member
 * that simply isn't there is still null. Must match interpreter.rs's
 * `Expr::Field`/`Expr::Index` eval rules — and their message text — exactly. */
/* Mirrors interpreter.rs's `type_name` exactly — the two backends' error
 * messages are meant to read identically, not merely to both fail. */
static const char *article_for(const CodeValue *v) {
    return (v->tag == CODE_ARRAY || v->tag == CODE_OBJECT) ? "an" : "a";
}

static const char *type_name(const CodeValue *v) {
    switch (v->tag) {
    case CODE_NUMBER: return "number";
    case CODE_STR:    return "string";
    case CODE_BOOL:   return "boolean";
    case CODE_NULL:   return "null";
    case CODE_ARRAY:  return "array";
    case CODE_OBJECT: return "object";
    }
    return "value";
}

void code_field(CodeValue *out, const CodeValue *obj, const char *field) {
    if (obj->tag != CODE_OBJECT) {
        char msg[128];
        snprintf(msg, sizeof msg,
                 "cannot read field '%s' of %s %s — '.' requires an object", field,
                 article_for(obj), type_name(obj));
        code_runtime_error(msg);
    }
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
    /* A *missing* field is still null: only the wrong operand kind errors
     * (see interpreter.rs's `Expr::Field`). */
    code_null(out);
}

/* `obj[key]` — a *computed* field read, the thing `code_field` can never
 * offer since its `field` argument is always a literal baked in at the call
 * site. Same absent-is-null rule as `code_field`; a non-`CODE_STR` key is
 * also just null, not an error, matching the array branch's non-`CODE_NUMBER`
 * case below. See interpreter.rs's `Expr::Index` — this must match it
 * exactly. */
void code_index(CodeValue *out, const CodeValue *arr, const CodeValue *index) {
    if (arr->tag == CODE_ARRAY) {
        if (index->tag == CODE_NUMBER) {
            double n = index->number;
            long long i = (long long)n;
            if ((double)i == n && i >= 0 && i < arr->len) {
                code_copy(out, slot_at(arr->items, i));
                return;
            }
        }
        /* An out-of-range or non-integer index is still null, for the same
         * reason a missing field is. */
        code_null(out);
        return;
    }
    if (arr->tag == CODE_OBJECT) {
        if (index->tag == CODE_STR) {
            for (long long i = 0; i < arr->len; i++) {
                if (strcmp(arr->keys[i], index->str) == 0) {
                    code_copy(out, slot_at(arr->items, i));
                    return;
                }
            }
        }
        code_null(out);
        return;
    }
    char msg[96];
    snprintf(msg, sizeof msg, "cannot index %s %s — '[]' requires an array or object",
             article_for(arr), type_name(arr));
    code_runtime_error(msg);
}

/* `emit <particle> to core [get <name>]`. `class_name` is read from the
 * particle's own "_class" field at runtime, never resolved to a fixed call
 * at compile time — even when the particle is a literal `ClassName { ... }`
 * right at the call site, because it can just as easily be a value that was
 * built earlier, stored, and passed around (see memory `new-code-particle`
 * for why particles carry `_class` with them at all). Must match
 * interpreter.rs's `dispatch_core` exactly — same handler set, same
 * operand-type rules.
 *
 * A future handler that returns *part of* its input (rather than a fresh
 * Number/Str/Array/Object, as `Length` always does) would need to
 * `code_retain` that piece before it can safely end up in `out` — nothing
 * here does that today, so this is a note for whoever adds the next one,
 * not a currently-exercised path. */
static const CodeValue *find_field(const CodeValue *obj, const char *key) {
    for (long long i = 0; i < obj->len; i++) {
        if (strcmp(obj->keys[i], key) == 0) {
            return slot_at(obj->items, i);
        }
    }
    return NULL;
}

/* Builds `{ "_class": class_name, "value": *value }` — the shape every core
 * handler's result takes, matching the old language's `<Name>Result`
 * convention: what goes into `emit` is a particle, so what comes back out
 * is one too, not a bare scalar.
 *
 * `slots` is a scratch buffer shaped exactly like the ones codegen.rs builds
 * for an object literal (`CODE_VALUE_SLOT_SIZE`-strided, addressed only via
 * `slot_at`) — zero-initialized before anything writes into it, which
 * matters here specifically: `code_str`/`code_copy` both call
 * `code_release` on their `out` first, and `code_release` reads `out->heap`
 * — on an *uninitialized* local that's garbage, not a real flag, so it has
 * to start at all-zero (reading as a payload-less number, `heap = 0`) for
 * that first release to be the no-op it's supposed to be. `code_copy`
 * rather than a raw struct copy for `value` for the same reason the doc
 * comment above `find_field` flags: a future handler whose result owns a
 * heap block needs it retained, and `code_copy` does that for free even
 * though `Length`'s `value` here never does. */
static void code_make_result(CodeValue *out, const char *class_name, const CodeValue *value) {
    const char *keys[2] = {"_class", "value"};
    _Alignas(8) char slots[2 * CODE_VALUE_SLOT_SIZE] = {0};
    code_str(slot_at(slots, 0), class_name);
    code_copy(slot_at(slots, 1), value);
    code_object(out, keys, slots, 2);
    /* `code_object` retained its own copy of each slot; these scratch ones
     * are done being needed the moment it returns. Harmless no-ops for
     * `Length` (`slots[0]` is a literal, `slots[1]` a Number — neither ever
     * `heap`), but load-bearing the moment a handler's `value` argument (see
     * this function's doc comment above `code_core_dispatch`) is itself
     * heap-owned: without this, that caller's own reference to `value`
     * would double-count against the fresh copy `code_object` just made. */
    code_release(slot_at(slots, 0));
    code_release(slot_at(slots, 1));
}

/* Builds `Exception { source, message, innerException }` — how a module (and,
 * once the C runtime has an error channel, the language itself) reports that
 * it could not do the work. `inner` may be NULL for the common case of a
 * failure with nothing beneath it.
 *
 * `message` is copied, not borrowed: callers build it into a stack buffer.
 * See docs/todo/errors-as-particles.md for the model. */
void code_make_exception(CodeValue *out, const char *source, const char *message,
                         const CodeValue *inner) {
    const char *keys[4] = {"_class", "source", "message", "innerException"};
    _Alignas(8) char slots[4 * CODE_VALUE_SLOT_SIZE] = {0};
    code_str(slot_at(slots, 0), "Exception");
    code_str_owned(slot_at(slots, 1), source);
    code_str_owned(slot_at(slots, 2), message);
    if (inner) {
        code_copy(slot_at(slots, 3), inner);
    } else {
        code_null(slot_at(slots, 3));
    }
    code_object(out, keys, slots, 4);
    for (int i = 0; i < 4; i++) {
        code_release(slot_at(slots, i));
    }
}

void code_core_dispatch(CodeValue *out, const CodeValue *particle) {
    if (particle->tag != CODE_OBJECT) {
        code_runtime_error("emit requires a particle (an object with a \"_class\" field)");
    }
    const CodeValue *class_val = find_field(particle, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        code_runtime_error("emit requires a particle (an object with a \"_class\" field)");
    }

    if (strcmp(class_val->str, "Timestamp") == 0) {
        /* Whole seconds since the Unix epoch — must match
         * interpreter.rs's `dispatch_core` exactly. Takes no operands,
         * so there is nothing to validate beyond the particle shape.
         * Zero-initialized for the same reason `code_make_result`'s
         * `slots` is: `code_number` releases `out` before setting it. */
        CodeValue ts = {0};
    #ifdef CODE_WASM
        code_number(&ts, code_host_now());
    #else
        code_number(&ts, (double)time(NULL));
    #endif
        code_make_result(out, "TimestampResult", &ts);
        return;
    }

    if (strcmp(class_val->str, "Length") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value) {
            code_runtime_error("Length { \"value\": ... } requires a 'value' field");
        }
        /* Zero-initialized for the same reason `code_make_result`'s `slots`
         * is: `code_number` releases `out` before setting it. */
        CodeValue count = {0};
        if (value->tag == CODE_ARRAY) {
            code_number(&count, (double)value->len);
            code_make_result(out, "LengthResult", &count);
            return;
        }
        if (value->tag == CODE_STR) {
            /* Characters, not bytes: `strlen` reported 6 for "héllo".
             * Counting the bytes that are not UTF-8 continuation bytes
             * (0b10xxxxxx) counts codepoints, which is what `chars().count()`
             * gives on the interpreter side — the two must agree. */
            long long chars = 0;
            for (const char *p = value->str; *p; p++) {
                if (((unsigned char)*p & 0xC0) != 0x80) {
                    chars++;
                }
            }
            code_number(&count, (double)chars);
            code_make_result(out, "LengthResult", &count);
            return;
        }
        code_runtime_error("Length requires an array or string 'value'");
    }

    /* Not a core class. Null rather than an error: sending a particle is not
     * a demand, and whether to act on one is the recipient's business — the
     * same answer `to this` and a native module give (decided 2026-08-28,
     * see docs/todo/errors-as-particles.md). */
    code_null(out);
}

/* ---- Native modules (`link "x.so" as x`, `emit ... to x [get n]`) --------
 *
 * See code_abi.h for the contract every module implements. Loading is
 * dlopen/dlsym-based here, exactly as in the interpreter (native.rs) — never
 * cc-time static linking, so multiple linked modules that all export the
 * identically-named `code_module_dispatch` never collide: dlsym resolves
 * within one module's own handle, never the whole process.
 *
 * A module's result is never adopted directly — `code_native_dispatch`
 * always deep-copies it into a fresh, host-allocated value
 * (`code_native_copy_in`), then calls the module's *own* copy of
 * `code_release` (looked up from the same handle) to free whatever it
 * allocated. That is what keeps `CODE_CHECK_LEAKS` meaningful on both sides
 * of a dlopen boundary: two separate copies of this runtime, two separate
 * static `live_blocks` counters, each only ever freeing blocks it itself
 * allocated.
 *
 * A `.a` static module (`link "x.a" as x`, `code build` only — see
 * `docs/todo/native-module-linking.md`) is a different story, handled
 * entirely by codegen.rs rather than by a `NativeHandle` here: it is linked
 * straight into the same binary as this very runtime, so it calls
 * `code_number`/`code_array`/... directly rather than bringing its own copy,
 * and its result needs no deep copy — it was built with the host's own
 * allocator to begin with. `code_static_module_check` and
 * `code_static_vars_object` below are the two bits of that path still
 * shared here rather than duplicated in generated IR. */

typedef struct {
    void (*dispatch)(CodeValue *out, const CodeValue *particle);
    void (*release)(CodeValue *v);
    /* Optional: the module's exported variables (constants). NULL when the
     * module doesn't export `code_module_vars` (a Phase 1, handlers-only
     * module) — in which case `code_native_vars_object` binds an empty
     * object. Unlike the two required symbols above, a missing one is not an
     * error. */
    const CodeVarList *(*vars)(void);
    /* Particles this module has pushed and the program hasn't handled yet —
     * see `code_emit_inbound`. A bounded ring: a module that runs away must
     * not grow the host's memory without bound, so the oldest entry is
     * dropped rather than the allocation growing. */
    CodeValue inbound[CODE_INBOUND_CAPACITY];
    int inbound_head;
    int inbound_count;
} NativeHandle;

/* Shared by both native-module paths (`.so` here, `.a` in codegen's direct
 * calls — see `code_static_vars_object` below): aborts with a consistent
 * message if `version` (whatever a module's `code_module_abi_version`
 * reported) doesn't match this runtime's `CODE_ABI_VERSION`. `what` names
 * the module in the error (a path for `.so`, the module's chosen prefix for
 * `.a`). */
void code_static_module_check(uint32_t version, const char *what) {
    if (version != CODE_ABI_VERSION) {
        char msg[256];
        snprintf(msg, sizeof msg, "native module '%s' has ABI version %u (expected %u)", what,
                 (unsigned)version, (unsigned)CODE_ABI_VERSION);
        code_runtime_error(msg);
    }
}

/* Defined below, next to `code_poll_inbound` — forward-declared so
 * `code_native_open` can hand its address to a module. */
void code_emit_inbound(void *queue, const CodeValue *value);

void *code_native_open(const char *path) {
#ifdef CODE_WASM
    (void)path;
    code_runtime_error("native modules are not available in a wasm build");
    return NULL;
#else
    void *handle = dlopen(path, RTLD_NOW);
    if (!handle) {
        char msg[256];
        snprintf(msg, sizeof msg, "cannot load native module '%s': %s", path, dlerror());
        code_runtime_error(msg);
    }

    uint32_t (*version_fn)(void) = (uint32_t (*)(void))dlsym(handle, "code_module_abi_version");
    if (!version_fn) {
        char msg[256];
        snprintf(msg, sizeof msg, "native module '%s' missing 'code_module_abi_version'", path);
        code_runtime_error(msg);
    }
    code_static_module_check(version_fn(), path);

    NativeHandle *nh = malloc(sizeof(NativeHandle));
    if (!nh) {
        code_runtime_error("out of memory");
    }
    nh->dispatch = (void (*)(CodeValue *, const CodeValue *))dlsym(handle, "code_module_dispatch");
    nh->release = (void (*)(CodeValue *))dlsym(handle, "code_release");
    if (!nh->dispatch || !nh->release) {
        char msg[256];
        snprintf(msg, sizeof msg,
                 "native module '%s' missing 'code_module_dispatch' or 'code_release'", path);
        code_runtime_error(msg);
    }
    /* Optional — a module without it simply has no exported variables. */
    nh->vars = (const CodeVarList *(*)(void))dlsym(handle, "code_module_vars");

    /* Also optional: a module that never speaks first doesn't export it.
     * The pusher handed across is *this* runtime's, not the module's own
     * copy — see code_abi.h for why that distinction matters. */
    memset(nh->inbound, 0, sizeof nh->inbound);
    nh->inbound_head = 0;
    nh->inbound_count = 0;
    void (*set_inbound)(void *, CodeEmitFn) =
        (void (*)(void *, CodeEmitFn))dlsym(handle, "code_module_set_inbound");
    if (set_inbound) {
        set_inbound(nh, code_emit_inbound);
    }
    return nh;
#endif
}

/* Builds a fresh heap-owned string value by copying `s`'s bytes — unlike
 * `code_str`, whose caller always passes a program literal it doesn't own.
 * Needed here because a module's own string may become dangling the moment
 * its `code_release` runs.
 *
 * Part of the module-facing ABI since 2026-08-28, when modules started
 * returning `Exception` particles: an exception message is built at runtime,
 * usually into a stack buffer, and handing that to `code_str` — which only
 * borrows the pointer — leaves a dangling read the moment the handler
 * returns. See code_abi.h. */
void code_str_owned(CodeValue *out, const char *s) {
    size_t n = strlen(s);
    char *buf = heap_alloc(n + 1);
    memcpy(buf, s, n + 1);
    code_release(out);
    out->tag = CODE_STR;
    out->heap = 1;
    out->str = buf;
}

/* Deep-copies a value produced by a *different* copy of this runtime (a
 * dlopen'd module) into a fresh, host-owned value — see the section comment
 * above for why this can never be a plain assignment or retain. */
static void code_native_copy_in(CodeValue *out, const CodeValue *from) {
    switch (from->tag) {
    case CODE_NUMBER:
        code_number(out, from->number);
        return;
    case CODE_STR:
        code_str_owned(out, from->str);
        return;
    case CODE_BOOL:
        code_bool(out, from->boolean);
        return;
    case CODE_NULL:
        code_null(out);
        return;
    case CODE_ARRAY: {
        // Zero-initialized (calloc, not malloc): each recursive
        // code_native_copy_in call below may write a CODE_STR/CODE_ARRAY/
        // CODE_OBJECT result via a constructor that calls code_release(out)
        // *first* (see code_str_owned) — that reads out->heap, which has to
        // start real rather than garbage, same hazard code_make_result's
        // doc comment already flags.
        void *slots = from->len > 0 ? calloc((size_t)from->len, CODE_VALUE_SLOT_SIZE) : NULL;
        for (long long i = 0; i < from->len; i++) {
            code_native_copy_in(slot_at(slots, i), slot_at(from->items, i));
        }
        code_array(out, slots, from->len);
        for (long long i = 0; i < from->len; i++) {
            code_release(slot_at(slots, i));
        }
        free(slots);
        return;
    }
    case CODE_OBJECT: {
        const char **keys = from->len > 0 ? malloc((size_t)from->len * sizeof(const char *)) : NULL;
        // Zero-initialized for the same reason the CODE_ARRAY case above is.
        void *slots = from->len > 0 ? calloc((size_t)from->len, CODE_VALUE_SLOT_SIZE) : NULL;
        for (long long i = 0; i < from->len; i++) {
            keys[i] = from->keys[i];
            code_native_copy_in(slot_at(slots, i), slot_at(from->items, i));
        }
        code_object(out, keys, slots, from->len);
        for (long long i = 0; i < from->len; i++) {
            code_release(slot_at(slots, i));
        }
        free(keys);
        free(slots);
        return;
    }
    }
}

/* ---- Inbound: a module speaking first --------------------------------------
 *
 * The other direction across the boundary. `code_module_dispatch` answers a
 * question; this lets a module raise one — a `terminal` pushing `Key`
 * particles as they arrive, say — which is what an event loop is made of.
 *
 * Deep-copied on the way in, exactly like a dispatch result: the value
 * belongs to the module's allocator until this returns, so nothing may be
 * retained. See `code_native_copy_in`.
 *
 * Deliberately lock-free, because this phase is synchronous: a module pushes
 * from inside a dispatch call it is already on the program's thread for. A
 * module pushing from a thread of its own needs a lock here and a keep-alive
 * loop in the generated `main` — the next phase, see
 * docs/todo/inbound-emissions-from-native-modules.md. */

void code_emit_inbound(void *queue, const CodeValue *value) {
    if (!queue || !value) {
        return;
    }
    NativeHandle *nh = (NativeHandle *)queue;
    int slot;
    if (nh->inbound_count == CODE_INBOUND_CAPACITY) {
        /* Full: drop the oldest so a runaway module costs bounded memory
         * rather than unbounded. */
        slot = nh->inbound_head;
        code_release(&nh->inbound[slot]);
        memset(&nh->inbound[slot], 0, sizeof(CodeValue));
        nh->inbound_head = (nh->inbound_head + 1) % CODE_INBOUND_CAPACITY;
    } else {
        slot = (nh->inbound_head + nh->inbound_count) % CODE_INBOUND_CAPACITY;
        nh->inbound_count++;
    }
    code_native_copy_in(&nh->inbound[slot], value);
}

/* Pops the oldest queued particle into `out`, or returns 0 when the queue is
 * empty (including for a module that never pushes at all, and for a handle
 * that hasn't been linked yet — the generated drain loop runs over every
 * module global, some of which may still be null). */
int code_poll_inbound(void *queue, CodeValue *out) {
    if (!queue) {
        return 0;
    }
    NativeHandle *nh = (NativeHandle *)queue;
    if (nh->inbound_count == 0) {
        return 0;
    }
    int slot = nh->inbound_head;
    code_copy(out, &nh->inbound[slot]);
    code_release(&nh->inbound[slot]);
    memset(&nh->inbound[slot], 0, sizeof(CodeValue));
    nh->inbound_head = (nh->inbound_head + 1) % CODE_INBOUND_CAPACITY;
    nh->inbound_count--;
    return 1;
}

/* Frees the small `NativeHandle` `code_native_open` allocated — called once
 * per linked module as part of the program's end-of-run cleanup (see
 * codegen.rs's `emit_cleanup`), the same "owns nothing when it exits" rule
 * `code_check_leaks` already holds every `CodeValue` slot to. Does not
 * `dlclose` the module itself: nothing depends on unloading it before the
 * process exits anyway, and dlclose has its own sharp edges (a module with
 * `__attribute__((destructor))` running at an unexpected time, symbols still
 * live on a stack frame mid-unwind) that aren't worth taking on for no
 * actual benefit here. */
void code_native_close(void *handle) {
    if (handle) {
        /* Anything still queued at exit is this runtime's to free — the
         * "owns nothing when it exits" rule `code_check_leaks` enforces. */
        NativeHandle *nh = (NativeHandle *)handle;
        for (int i = 0; i < nh->inbound_count; i++) {
            code_release(&nh->inbound[(nh->inbound_head + i) % CODE_INBOUND_CAPACITY]);
        }
    }
    free(handle);
}

/* `emit <particle> to <alias> [get <name>]` for a linked native module.
 * `handle` is whatever `code_native_open` returned for that alias. */
void code_native_dispatch(void *handle, CodeValue *out, const CodeValue *particle) {
    NativeHandle *nh = (NativeHandle *)handle;
    CodeValue result = {0};
    nh->dispatch(&result, particle);
    code_native_copy_in(out, &result);
    nh->release(&result);
}

/* `link "x.so" as x` — build the object of the module's exported variables
 * (constants), bound under `alias` so `alias.name` is ordinary field access.
 * Reads the module's optional `code_module_vars` export and deep-copies each
 * value out (the same boundary rule as `code_native_dispatch`), then calls
 * the module's own `code_release` on each. A module with no such export
 * yields an empty object. The key *strings* are borrowed from the module
 * (like every object's keys in this runtime — `code_object` copies the
 * pointers, never the characters); that is safe because the module owns them
 * for its whole lifetime and `code_native_close` never `dlclose`s it, so they
 * outlive the object. `handle` is whatever `code_native_open` returned. */
void code_native_vars_object(void *handle, CodeValue *out) {
    NativeHandle *nh = (NativeHandle *)handle;
    const CodeVarList *list = nh->vars ? nh->vars() : NULL;
    long long count = list ? list->count : 0;
    if (count < 0) {
        code_runtime_error("native module reports a negative variable count");
    }
    const char **keys = NULL;
    void *values = NULL;
    if (count > 0) {
        keys = (const char **)malloc((size_t)count * sizeof(const char *));
        // Zero-initialized (calloc, not malloc): each code_native_copy_in
        // below may write a result via a constructor that calls
        // code_release(out) first (see code_str_owned) — that reads
        // out->heap, which has to start real rather than garbage.
        values = calloc((size_t)count, CODE_VALUE_SLOT_SIZE);
        for (long long i = 0; i < count; i++) {
            keys[i] = list->names[i];
            code_native_copy_in(slot_at(values, i), slot_at(list->values, i));
        }
    }
    code_object(out, keys, values, count);
    // code_object retained each scratch value into the fresh object block;
    // drop the scratch copies now (and the module's own copies are the
    // module's to keep — we never release its name strings, only the values
    // we copied out of its buffer).
    if (count > 0) {
        for (long long i = 0; i < count; i++) {
            code_release(slot_at(values, i));
        }
        free(values);
    }
    free(keys);
}

/* `link "x.a" as x`'s equivalent of `code_native_vars_object` above, for a
 * module whose `code_module_vars` (if it exports one — `list` is NULL
 * otherwise) already returns host-allocated values: no `code_native_copy_in`
 * needed, just `code_retain` into a fresh object, exactly like building an
 * object literal from existing bindings. Key strings are borrowed exactly
 * as `code_native_vars_object` borrows them — the module's static storage
 * outlives the program, there being no `.a` equivalent of `dlclose` to worry
 * about at all. */
void code_static_vars_object(const CodeVarList *list, CodeValue *out) {
    long long count = list ? list->count : 0;
    if (count < 0) {
        code_runtime_error("native module reports a negative variable count");
    }
    const char **keys = NULL;
    void *values = NULL;
    if (count > 0) {
        keys = (const char **)malloc((size_t)count * sizeof(const char *));
        values = malloc((size_t)count * CODE_VALUE_SLOT_SIZE);
        for (long long i = 0; i < count; i++) {
            keys[i] = list->names[i];
            CodeValue *slot = slot_at(values, i);
            *slot = *slot_at(list->values, i);
            code_retain(slot);
        }
    }
    code_object(out, keys, values, count);
    if (count > 0) {
        for (long long i = 0; i < count; i++) {
            code_release(slot_at(values, i));
        }
        free(values);
    }
    free(keys);
}

/* `loop [k,] v over <expr>` support. Three calls instead of one combined
 * "iterate" entry point because the loop's control flow lives in the
 * generated IR, not here: codegen emits the counter, the bounds check and
 * the back-edge itself (see codegen.rs's `gen_loop`), and only calls into
 * the runtime for the things that need to inspect a `CodeValue`. Must match
 * interpreter.rs's `Stmt::Loop` eval rule: the iterable must be an array or
 * object — anything else aborts rather than iterating zero times. An
 * object's `items` is laid out parallel to its `keys` (see `code_object`),
 * which is what lets `code_iter_at` serve both container kinds unchanged. */
long long code_iter_len(const CodeValue *v) {
    if (v->tag != CODE_ARRAY && v->tag != CODE_OBJECT) {
        code_runtime_error("loop requires an array or object");
    }
    return v->len;
}

/* `i` is always in range: the only caller is the loop header codegen emits,
 * which already compared it against `code_iter_len`'s result. */
void code_iter_at(CodeValue *out, const CodeValue *arr, long long i) {
    code_copy(out, slot_at(arr->items, i));
}

/* The `key` half of `loop k, v over <expr>` — see `Stmt::Loop`'s doc comment
 * for the law (`X[k] = v`) this exists to satisfy. `code_str_owned`, not a
 * borrowed pointer into `keys`: a key can outlive the loop (assigned to a
 * `get` accumulator), and for an object built by a *different* copy of this
 * runtime (a dlopen'd module) the key bytes aren't even ours to hand back a
 * pointer into. `i` is always in range, same as `code_iter_at`. */
void code_iter_key(CodeValue *out, const CodeValue *v, long long i) {
    if (v->tag == CODE_OBJECT) {
        code_str_owned(out, v->keys[i]);
        return;
    }
    code_number(out, (double)i);
}

/* ---- Handlers written in the language itself -------------------------------
 *
 * The one check the compiled backend needs that nothing else did. Its
 * interpreter counterpart lives in `interpreter.rs`'s `dispatch_handler`, so
 * a handler behaves identically whichever backend runs it. */

/* A handler's result must be a particle, so every `get` binding has a class
 * to test with `is`. Same rule the core handlers follow. */
void code_check_particle(const CodeValue *v) {
    if (v->tag == CODE_OBJECT) {
        for (long long i = 0; i < v->len; i++) {
            if (strcmp(v->keys[i], "_class") == 0) {
                return;
            }
        }
    }
    char msg[128];
    snprintf(msg, sizeof msg,
             "a handler must return a particle — an object with a '_class' field — found %s %s",
             article_for(v), type_name(v));
    code_runtime_error(msg);
}

/* ---- Rendering a value as text -------------------------------------------
 *
 * The compiled side of string interpolation (`"hi $name"`), and the first
 * place either runtime had to turn a value back into characters — so this
 * has to agree with `value.rs`'s `Display` byte for byte, or the same
 * fixture would assert differently under `code run` than under `code build`.
 *
 * Same split as `Expr::Interpolated`'s doc comment: a string at the *top*
 * level renders bare, everything else as compact JSON — which means a string
 * nested inside an array or object does keep its quotes. Iterative, for the
 * reason the traversal section above gives. */

typedef struct {
    char *buf;
    size_t len;
    size_t cap;
} TextBuf;

static void text_push(TextBuf *t, const char *s, size_t n) {
    if (t->len + n + 1 > t->cap) {
        size_t next = t->cap ? t->cap : 64;
        while (next < t->len + n + 1) {
            next *= 2;
        }
        char *bigger = realloc(t->buf, next);
        if (!bigger) {
            code_runtime_error("out of memory");
        }
        t->buf = bigger;
        t->cap = next;
    }
    memcpy(t->buf + t->len, s, n);
    t->len += n;
}

static void text_push_str(TextBuf *t, const char *s) { text_push(t, s, strlen(s)); }

/* Rust's `{}` for f64 is the shortest decimal that round-trips, laid out
 * positionally (never in exponent form). Reproduced here digit by digit,
 * because the obvious shortcut — let `printf("%.*e")` do the rounding and
 * just move the point — disagrees on exact ties: glibc rounds those to even
 * (2181495296738027.25 -> "...27.2") while Rust rounds away from zero
 * ("...27.3"). So `printf` is used only for the *exact* expansion, and the
 * rounding to the shortest round-tripping length happens below. Verified
 * against Rust's own output over 205k values, random bit patterns included.
 *
 * Integral values short-circuit through `%lld`: it is the overwhelmingly
 * common case, it is exact, and it is the only path the wasm shim (which has
 * no float formatting and no `strtod`) can offer at all. A fractional number
 * on wasm is a loud error rather than a string that silently disagrees with
 * the other two modes — see docs/todo/wasm-fractional-number-text.md. */
static void text_push_number(TextBuf *t, double d) {
    char tmp[512];
    if (d == (double)(long long)d && d >= -9007199254740992.0 && d <= 9007199254740992.0) {
        /* `(long long)-0.0` is 0, which would print an unsigned zero — but
         * Rust's `Display` keeps the sign. Tested by dividing rather than
         * with `signbit`, so the wasm build needs no `math.h`. */
        if (d == 0.0 && 1.0 / d < 0.0) {
            text_push_str(t, "-0");
            return;
        }
        snprintf(tmp, sizeof tmp, "%lld", (long long)d);
        text_push_str(t, tmp);
        return;
    }
#ifdef CODE_WASM
    code_runtime_error("rendering a fractional number as text is not supported on wasm yet");
#else
    /* 41 significant digits: more than the 17 any double needs to round-trip,
     * so `full` is the exact expansion as far as the rounding below can care. */
    char exact[80];
    snprintf(exact, sizeof exact, "%.40e", d);
    const char *p = exact;
    int negative = (*p == '-');
    if (negative) {
        p++;
    }
    char full[48];
    size_t nfull = 0;
    for (; *p && *p != 'e'; p++) {
        if (*p != '.') {
            full[nfull++] = *p;
        }
    }
    int fullexp = (int)strtol(p + 1, NULL, 10);

    /* Shortest length whose correctly-rounded form reads back bit-identically.
     * 17 always does, so the loop always terminates with a usable answer. */
    char m[48];
    size_t n = 1;
    int exp10 = fullexp;
    for (int len = 1; len <= 17; len++) {
        n = (size_t)len;
        exp10 = fullexp;
        memcpy(m, full, n);
        if (nfull > n && full[n] >= '5') {
            size_t i = n;
            while (i > 0) {
                if (m[i - 1] == '9') {
                    m[i - 1] = '0';
                    i--;
                } else {
                    m[i - 1]++;
                    break;
                }
            }
            /* Carried off the front (999... -> 1000...): one more digit, one
             * higher power of ten. */
            if (i == 0) {
                memmove(m + 1, m, n);
                m[0] = '1';
                exp10++;
            }
        }
        char sci[64];
        size_t o = 0;
        if (negative) {
            sci[o++] = '-';
        }
        sci[o++] = m[0];
        if (n > 1) {
            sci[o++] = '.';
            memcpy(sci + o, m + 1, n - 1);
            o += n - 1;
        }
        o += (size_t)snprintf(sci + o, sizeof sci - o, "e%d", exp10);
        sci[o] = '\0';
        if (strtod(sci, NULL) == d) {
            break;
        }
    }
    while (n > 1 && m[n - 1] == '0') {
        n--;
    }

    /* Exponent form was only ever the intermediate — lay the digits out
     * positionally, which is the one form Rust's `Display` ever prints. */
    size_t out = 0;
    if (negative) {
        tmp[out++] = '-';
    }
    if (exp10 >= (int)n - 1) {
        /* Whole number: every digit, then zeros out to the decimal point. */
        memcpy(tmp + out, m, n);
        out += n;
        for (int i = 0; i < exp10 - (int)n + 1; i++) {
            tmp[out++] = '0';
        }
    } else if (exp10 >= 0) {
        /* Point falls inside the digit run. */
        memcpy(tmp + out, m, (size_t)exp10 + 1);
        out += (size_t)exp10 + 1;
        tmp[out++] = '.';
        memcpy(tmp + out, m + exp10 + 1, n - (size_t)exp10 - 1);
        out += n - (size_t)exp10 - 1;
    } else {
        /* Leading `0.` and however many zeros before the first digit. */
        tmp[out++] = '0';
        tmp[out++] = '.';
        for (int i = 0; i < -exp10 - 1; i++) {
            tmp[out++] = '0';
        }
        memcpy(tmp + out, m, n);
        out += n;
    }
    text_push(t, tmp, out);
#endif
}

static void text_push_json_string(TextBuf *t, const char *s) {
    text_push(t, "\"", 1);
    for (const char *p = s; *p; p++) {
        switch (*p) {
        case '"':  text_push(t, "\\\"", 2); break;
        case '\\': text_push(t, "\\\\", 2); break;
        case '\n': text_push(t, "\\n", 2); break;
        case '\t': text_push(t, "\\t", 2); break;
        default:   text_push(t, p, 1); break;
        }
    }
    text_push(t, "\"", 1);
}

/* One entry of the render work stack. `value` is a value still to write;
 * otherwise `punct` is literal text to emit (a bracket, a comma, or a key
 * that has already been quoted into the buffer's own storage). */
typedef struct {
    const CodeValue *value;
    const char *punct;
    int is_key;
} TextStep;

static TextStep *steps = NULL;
static size_t steps_cap = 0;

void code_to_text(CodeValue *out, const CodeValue *v) {
    TextBuf t = {NULL, 0, 0};
    size_t len = 0;

    steps = grow(steps, &steps_cap, len + 1, sizeof(TextStep));
    steps[len++] = (TextStep){v, NULL, 0};
    int top_level = 1;

    while (len > 0) {
        TextStep step = steps[--len];
        if (!step.value) {
            if (step.is_key) {
                text_push_json_string(&t, step.punct);
                text_push(&t, ":", 1);
            } else {
                text_push_str(&t, step.punct);
            }
            continue;
        }
        const CodeValue *current = step.value;
        switch (current->tag) {
        case CODE_NUMBER:
            text_push_number(&t, current->number);
            break;
        case CODE_STR:
            if (top_level) {
                text_push_str(&t, current->str);
            } else {
                text_push_json_string(&t, current->str);
            }
            break;
        case CODE_BOOL:
            text_push_str(&t, current->boolean ? "true" : "false");
            break;
        case CODE_NULL:
            text_push_str(&t, "null");
            break;
        /* Pushed in reverse so they pop in source order, with the closing
         * bracket pushed first and therefore popped last — mirroring
         * `value.rs`'s `Display`. */
        case CODE_ARRAY:
            text_push(&t, "[", 1);
            steps = grow(steps, &steps_cap, len + 1, sizeof(TextStep));
            steps[len++] = (TextStep){NULL, "]", 0};
            for (long long i = current->len - 1; i >= 0; i--) {
                steps = grow(steps, &steps_cap, len + 2, sizeof(TextStep));
                steps[len++] = (TextStep){slot_at(current->items, i), NULL, 0};
                if (i > 0) {
                    steps[len++] = (TextStep){NULL, ",", 0};
                }
            }
            break;
        case CODE_OBJECT:
            text_push(&t, "{", 1);
            steps = grow(steps, &steps_cap, len + 1, sizeof(TextStep));
            steps[len++] = (TextStep){NULL, "}", 0};
            for (long long i = current->len - 1; i >= 0; i--) {
                steps = grow(steps, &steps_cap, len + 3, sizeof(TextStep));
                steps[len++] = (TextStep){slot_at(current->items, i), NULL, 0};
                steps[len++] = (TextStep){NULL, current->keys[i], 1};
                if (i > 0) {
                    steps[len++] = (TextStep){NULL, ",", 0};
                }
            }
            break;
        }
        top_level = 0;
    }

    /* `text_push` always keeps one spare byte, but an empty render never
     * called it — this makes the buffer exist either way. */
    text_push(&t, "", 0);
    t.buf[t.len] = '\0';

    /* Rehomed into a refcounted block: `t.buf` came from plain `realloc`, and
     * every owned string in this runtime has to be freeable by `code_release`
     * like any other. Built before `out` is released — `out` may be the very
     * value being rendered. */
    char *owned = heap_alloc(t.len + 1);
    memcpy(owned, t.buf, t.len + 1);
    free(t.buf);
    code_release(out);
    out->tag = CODE_STR;
    out->heap = 1;
    out->str = owned;
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
    /* One array operand is enough: the other is then a single *element* to
     * append or prepend, and only two arrays concatenate. Written as one
     * case rather than three because "how many elements does this operand
     * contribute, and where do they come from" is the only difference —
     * see interpreter.rs's matching arms. */
    if (a->tag == CODE_ARRAY || b->tag == CODE_ARRAY) {
        long long na = (a->tag == CODE_ARRAY) ? a->len : 1;
        long long nb = (b->tag == CODE_ARRAY) ? b->len : 1;
        long long total = na + nb;
        void *buf = NULL;
        if (total > 0) {
            buf = heap_alloc((size_t)total * CODE_VALUE_SLOT_SIZE);
            for (long long i = 0; i < na; i++) {
                const CodeValue *src = (a->tag == CODE_ARRAY) ? slot_at(a->items, i) : a;
                code_retain(src);
                *slot_at(buf, i) = *src;
            }
            for (long long i = 0; i < nb; i++) {
                const CodeValue *src = (b->tag == CODE_ARRAY) ? slot_at(b->items, i) : b;
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
    /* Two objects merge, the way two arrays concatenate — see
     * interpreter.rs's matching arm for the rule this implements. A field
     * both sides name takes b's value in a's position; b's remaining fields
     * follow in b's own order. Checked *after* the array case above, so one
     * array operand still makes the object a single element rather than
     * something to merge into.
     *
     * `find_field` compares key text, never pointers: two literals spelling
     * the same name are separate objects in read-only data, and a module's
     * keys live in its own storage entirely. Layout and key ownership match
     * `code_object` exactly — one allocation holding `[keys...][values...]`,
     * with the key *pointers* copied and the characters borrowed from
     * whichever operand supplied them (both outlive this call, and their
     * storage outlives the program). */
    if (a->tag == CODE_OBJECT && b->tag == CODE_OBJECT) {
        long long total = a->len;
        for (long long j = 0; j < b->len; j++) {
            if (find_field(a, b->keys[j]) == NULL) {
                total++;
            }
        }
        const char **key_buf = NULL;
        void *value_buf = NULL;
        if (total > 0) {
            size_t keys_bytes = (size_t)total * sizeof(const char *);
            key_buf = heap_alloc(keys_bytes + (size_t)total * CODE_VALUE_SLOT_SIZE);
            value_buf = (char *)key_buf + keys_bytes;
            long long n = 0;
            for (long long i = 0; i < a->len; i++) {
                const CodeValue *override_val = find_field(b, a->keys[i]);
                const CodeValue *src = override_val ? override_val : slot_at(a->items, i);
                key_buf[n] = a->keys[i];
                code_retain(src);
                *slot_at(value_buf, n) = *src;
                n++;
            }
            for (long long j = 0; j < b->len; j++) {
                if (find_field(a, b->keys[j]) != NULL) {
                    continue;
                }
                key_buf[n] = b->keys[j];
                const CodeValue *src = slot_at(b->items, j);
                code_retain(src);
                *slot_at(value_buf, n) = *src;
                n++;
            }
        }
        /* Same ordering point as the two cases above: every value is
         * retained already, so releasing `out` here cannot free anything the
         * new block refers to, even when `out` is `a` or `b` (`x = x + x`). */
        code_release(out);
        out->tag = CODE_OBJECT;
        out->heap = total > 0;
        out->keys = key_buf;
        out->items = value_buf;
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

/* -1/0/1 for two Numbers; aborts for anything else, strings included —
 * ordering is Number-only (see ast.rs's BinOp doc comment). codegen.rs turns
 * the result into `<`/`>`/`≤`/`≥` with a plain LLVM icmp against 0 — one
 * runtime function instead of four. */
long long code_compare(const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        if (a->number < b->number) {
            return -1;
        }
        return a->number > b->number ? 1 : 0;
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

/* `expr is ClassName` — the type test (see ast.rs's `Expr::Is`): 1 when
 * `a` is an object whose `"_class"` field holds the string `name`, 0 for
 * everything else. Total by design — a missing `_class` or a non-object
 * operand simply answers 0, mirroring how `find_field` reports absence as
 * null and equality turns that into false. Must match interpreter.rs's
 * `Expr::Is` arm exactly. */
int code_is_particle(const CodeValue *a, const char *name) {
    if (a->tag != CODE_OBJECT) {
        return 0;
    }
    const CodeValue *class_val = find_field(a, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        return 0;
    }
    return strcmp(class_val->str, name) == 0 ? 1 : 0;
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
typedef struct {
    const CodeValue *a;
    const CodeValue *b;
} Pair;

static Pair *pending = NULL; /* value pairs still to compare */
static size_t pending_cap = 0;

int code_values_equal(const CodeValue *a, const CodeValue *b) {
    size_t len = 0;
    pending = grow(pending, &pending_cap, len + 1, sizeof(Pair));
    pending[len].a = a;
    pending[len].b = b;
    len++;

    while (len > 0) {
        Pair pair = pending[--len];
        const CodeValue *x = pair.a;
        const CodeValue *y = pair.b;
        if (x->tag != y->tag) {
            return 0;
        }
        switch (x->tag) {
        case CODE_NUMBER:
            if (x->number != y->number) {
                return 0;
            }
            break;
        case CODE_STR:
            if (strcmp(x->str, y->str) != 0) {
                return 0;
            }
            break;
        case CODE_BOOL:
            if (x->boolean != y->boolean) {
                return 0;
            }
            break;
        case CODE_NULL:
            break;
        case CODE_ARRAY:
        case CODE_OBJECT:
            if (x->len != y->len) {
                return 0;
            }
            pending = grow(pending, &pending_cap, len + (size_t)x->len, sizeof(Pair));
            for (long long i = 0; i < x->len; i++) {
                /* Objects compare positionally — same keys in the same
                 * order — matching value.rs's `PartialEq` exactly. */
                if (x->tag == CODE_OBJECT && strcmp(x->keys[i], y->keys[i]) != 0) {
                    return 0;
                }
                pending[len].a = slot_at(x->items, i);
                pending[len].b = slot_at(y->items, i);
                len++;
            }
            break;
        }
    }
    return 1;
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
