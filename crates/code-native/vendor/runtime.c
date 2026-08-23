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
#define _GNU_SOURCE
#include <dlfcn.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

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

/* ---- Iterative traversal --------------------------------------------------
 *
 * `code_release`, `code_values_equal` and `print_json` all walk a value's
 * children, and all three used to recurse. Nesting depth is bounded only by a
 * loop's iteration count (`loop x over xs { a = [a] }`), not by how many
 * brackets the source contains, so one stack frame per level segfaults at
 * around 131k deep — see `tests/stress_deep_nesting.code`, and `value.rs` for the
 * interpreter's three equivalents, which have the same shape for the same
 * reason. Each keeps an explicit work stack in heap memory instead.
 *
 * The stacks are file-static and grow on demand, never shrinking: this is a
 * single-threaded runtime and none of the three can re-enter itself now that
 * they don't recurse, so one buffer each is enough. */

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
        code_number(&ts, (double)time(NULL));
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
            code_number(&count, (double)strlen(value->str));
            code_make_result(out, "LengthResult", &count);
            return;
        }
        code_runtime_error("Length requires an array or string 'value'");
    }

    char msg[96];
    snprintf(msg, sizeof msg, "unknown core handler '%s'", class_val->str);
    code_runtime_error(msg);
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

void *code_native_open(const char *path) {
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
    return nh;
}

/* Builds a fresh heap-owned string value by copying `s`'s bytes — unlike
 * `code_str`, whose caller always passes a program literal it doesn't own.
 * Needed here because a module's own string may become dangling the moment
 * its `code_release` runs. */
static void code_str_owned(CodeValue *out, const char *s) {
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

/* One step of print_json's traversal. Closers and separators go on the same
 * stack as the values they follow, which is what removes the need for a
 * recursive call to come back and finish a container. Mirrors value.rs's
 * `Step` enum exactly. */
typedef struct {
    const CodeValue *value; /* NULL when this step is punctuation */
    const char *punct;      /* "," / "]" / "}" , or an object key */
    int is_key;             /* punct is a key: print quoted, then ':' */
} Step;

static Step *steps = NULL;
static size_t steps_cap = 0;

static void push_step(size_t *len, const CodeValue *value, const char *punct, int is_key) {
    steps = grow(steps, &steps_cap, *len + 1, sizeof(Step));
    steps[*len].value = value;
    steps[*len].punct = punct;
    steps[*len].is_key = is_key;
    (*len)++;
}

static void print_json(const CodeValue *v) {
    char buf[64];
    size_t len = 0;
    push_step(&len, v, NULL, 0);

    while (len > 0) {
        Step step = steps[--len];
        if (!step.value) {
            if (step.is_key) {
                print_json_string(step.punct);
                putchar(':');
            } else {
                fputs(step.punct, stdout);
            }
            continue;
        }
        const CodeValue *current = step.value;
        switch (current->tag) {
        case CODE_NUMBER:
            format_number(current->number, buf, sizeof buf);
            fputs(buf, stdout);
            break;
        case CODE_STR:
            print_json_string(current->str);
            break;
        case CODE_BOOL:
            fputs(current->boolean ? "true" : "false", stdout);
            break;
        case CODE_NULL:
            fputs("null", stdout);
            break;
        /* Pushed in reverse so they pop in source order, with the closing
         * bracket pushed first and therefore popped last. */
        case CODE_ARRAY:
            putchar('[');
            push_step(&len, NULL, "]", 0);
            for (long long i = current->len - 1; i >= 0; i--) {
                push_step(&len, slot_at(current->items, i), NULL, 0);
                if (i > 0) {
                    push_step(&len, NULL, ",", 0);
                }
            }
            break;
        case CODE_OBJECT:
            putchar('{');
            push_step(&len, NULL, "}", 0);
            for (long long i = current->len - 1; i >= 0; i--) {
                push_step(&len, slot_at(current->items, i), NULL, 0);
                push_step(&len, NULL, current->keys[i], 1);
                if (i > 0) {
                    push_step(&len, NULL, ",", 0);
                }
            }
            break;
        }
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
