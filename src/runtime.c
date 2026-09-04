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
#include <pthread.h>
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

/* ---- The failure channel -------------------------------------------------
 *
 * `code_runtime_error` is `_Noreturn`: a helper that reaches it cannot tell
 * its caller anything, because there is no caller left. That is the whole
 * reason a `.code` program can never respond to its own runtime errors —
 * `10 / 0` ends the process from inside `code_div`, so no `if r is Exception`
 * downstream ever runs.
 *
 * This is the way back. A helper that cannot do its work calls `fail` and
 * returns normally; `code_failed` is left set, and the generated code checks
 * it after every call that can set it (codegen.rs's `check_failed`, which is
 * the only way those helpers are ever called — see `call_fallible`). The
 * failing operation therefore reaches a landing block the caller chose,
 * instead of taking the process with it.
 *
 * Phase 3 of docs/todo/errors-as-particles.md builds *only* the channel:
 * every landing block still ends in `code_abort_failure`, so behaviour is
 * byte-for-byte what it was. Phases 4 and 5 change what those blocks do —
 * write an Exception into the frame's `out` and branch to its exit — without
 * touching anything here.
 *
 * Deliberately NOT in code_abi.h. A `.so` module carries its own copy of this
 * runtime, so a flag set inside one would be set on the *module's* copy and
 * the host would never look at it — a silently swallowed failure. Modules
 * report trouble by returning `code_make_exception`, which needs no channel
 * because a return value already is one. */
int code_failed = 0;
static char failure_message[256];

/* Where the top-level statement now running came from, as the rendered
 * `--> file:line:col` block `span.rs`'s `location_block` produces — or NULL
 * when the program has no source to point into (a `Program` built by hand,
 * or an entry module the loader kept no text for).
 *
 * Written by generated code before each top-level statement (see codegen.rs)
 * and read only by `code_abort_failure`, which is the single place a
 * compiled program reports anything. That single place is what made this
 * cheap: before phases 3 and 4 an error could leave from any of `runtime.c`'s
 * `_Noreturn` helpers, and giving each of them a location would have meant
 * threading one through every call site in the generated IR. Now a failure
 * inside a handler is a value, so only the top level ever prints. */
const char *code_location = NULL;

/* First failure wins: with a check after every fallible call there is never a
 * second one to lose, but if that ever slips the original cause is the one
 * worth keeping. Copied into a fixed buffer rather than retained by pointer —
 * most callers build their message in a stack buffer. */
static void fail(const char *message) {
    if (!code_failed) {
        snprintf(failure_message, sizeof failure_message, "%s", message);
        code_failed = 1;
    }
}

/* What a landing block ends in at the *top level*, where there is no frame to
 * return into: a failure there ends the program with a non-zero status, which
 * is the same thing `return Exception` from the outermost call means. Routed
 * through `code_runtime_error` rather than duplicating its body so the wasm
 * build (which reports through `code_host_error` instead of stderr) keeps
 * working without this file knowing there are two ways to report. */
_Noreturn void code_abort_failure(void) {
    const char *message = code_failed ? failure_message : "unknown runtime error";
    if (code_location) {
        /* Joined in exactly the order `span::render` joins them, so the two
         * output modes produce byte-identical stderr. Heap rather than a
         * fixed buffer because the block quotes a source line of any length,
         * and a truncated location would be a silent divergence; the
         * allocation is never freed, which is correct for a function that
         * ends the process on its next statement. Not `heap_alloc`: this is
         * not a `CodeValue` block and must not move the leak counter. */
        size_t n = strlen(message) + 1 + strlen(code_location) + 1;
        char *located = malloc(n);
        if (located) {
            snprintf(located, n, "%s\n%s", message, code_location);
            code_runtime_error(located);
        }
    }
    code_runtime_error(message);
}

/* What a landing block ends in *inside a handler*: the frame's result becomes
 * an `Exception`, and the flag is cleared so the caller carries on. The
 * caller is under no obligation to look — a returned Exception is an ordinary
 * value, not a signal that keeps propagating (decided 2026-08-28: "C geriye
 * Exception döner, B bakmazsa kaldığı yerden devam"). Only the frame where
 * the failure actually happened unwinds.
 *
 * `source` is "core" because that is the language's own name for what runs a
 * program's own statements; a module's exceptions name the module instead. */
void code_take_failure(CodeValue *out) {
    code_make_exception(out, "core",
                        code_failed ? failure_message : "unknown runtime error", NULL);
    code_failed = 0;
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
 * program output.
 *
 * Bumped atomically because a module with a thread of its own allocates from
 * that thread — `code_emit_inbound` deep-copies the pushed particle on
 * whichever thread pushed it (see the "Inbound" section). A plain `++` there
 * is a data race, and a lost increment would make `CODE_CHECK_LEAKS` report
 * a leak that never happened, or miss one that did. Relaxed ordering is
 * enough: nothing is published through this counter, it is only read once at
 * exit, after every thread that could touch it has been shut out
 * (`code_native_close`). */
static long long live_blocks = 0;

#define code_blocks_add(n) (void)__atomic_add_fetch(&live_blocks, (n), __ATOMIC_RELAXED)
#define code_blocks_read() __atomic_load_n(&live_blocks, __ATOMIC_RELAXED)

static void *heap_alloc(size_t bytes) {
    CodeHeader *h = malloc(sizeof(CodeHeader) + bytes);
    if (!h) {
        code_runtime_error("out of memory");
    }
    h->rc = 1;
    code_blocks_add(1);
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
 * The stacks grow on demand and never shrink: neither can re-enter itself
 * now that they don't recurse, so one buffer each is enough — per thread.
 *
 * `code_release`'s is thread-local, because the release path is reachable
 * from a module's own thread: `code_emit_inbound` deep-copies onto a ring
 * that is full, and dropping the oldest entry releases it. A shared buffer
 * would then be walked by two threads at once, which is heap corruption
 * rather than a wrong answer. One buffer per thread costs a few KB for the
 * one or two threads a program has, and is never freed — the same bargain
 * the single shared one already made. `code_values_equal`'s stays plain
 * static: comparison is only ever reached from program code, which runs on
 * the program's own thread. */
#ifdef CODE_WASM
/* No threads in a wasm build, and the freestanding shim has no TLS. */
#define CODE_THREAD_LOCAL
#else
#define CODE_THREAD_LOCAL __thread
#endif

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

static CODE_THREAD_LOCAL CodeValue *dead = NULL; /* values whose block is owed a free() */
static CODE_THREAD_LOCAL size_t dead_cap = 0;

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
        code_blocks_add(-1);
    }
}

/* `code_release`, plus blanking the slot afterwards.
 *
 * Every other release is immediately followed by a write to the same slot,
 * which is why `code_release` can leave `heap` alone (see its comment). One
 * caller is different: a compiled statement releases its temporaries the
 * moment the statement ends, and then leaves those slots sitting there —
 * for the next execution of the same statement to overwrite, or for the
 * exit sweep to release again. Either would be a second release of a block
 * already freed. Blanking closes both: an all-zero slot is a payload-less
 * number, exactly what a slot looks like before its first write, so
 * releasing it again is a no-op and writing to it is safe.
 *
 * Not in `code_abi.h`, unlike the constructors: no module ever calls this.
 * It exists for `gen_stmt` in `src/codegen.rs`. */
void code_clear(CodeValue *v) {
    code_release(v);
    memset(v, 0, sizeof *v);
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
    long long leaked = code_blocks_read();
    if (leaked != 0) {
        char msg[96];
        snprintf(msg, sizeof msg, "%lld heap block(s) leaked", leaked);
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

/* Appends `key`'s characters (a NULL key reads as empty, the same answer
 * `code_object` has always given it) to the block's own character run and
 * answers where they landed, advancing the cursor past them. Shared by the
 * two places that build an object: the constructor and `+`'s merge. */
static const char *copy_key(char **chars, const char *key) {
    size_t n = (key ? strlen(key) : 0) + 1;
    if (key) {
        memcpy(*chars, key, n);
    } else {
        (*chars)[0] = '\0';
    }
    const char *placed = *chars;
    *chars += n;
    return placed;
}

/* One allocation for both arrays: `[keys...][values...]`. The key pointers
 * themselves are string literals in read-only data, so only the array of
 * pointers is copied, never the characters. */
/* Owns its key *characters*, not just the pointers, since 2026-08-29.
 *
 * They used to be borrowed, which made every field name in a value something
 * that had to outlive it — fine while keys were only ever program literals,
 * and the reason `code-native`'s `object()` demanded `&'static CStr`. Two
 * things wanted otherwise at once: `{ "$name" = v }` builds a key at run
 * time, and a module that wants to hand back HTTP headers has names that
 * arrived over a socket. Copying is one path instead of two, costs a few
 * bytes and one `memcpy` per field, and deletes the restriction rather than
 * documenting an exception to it.
 *
 * The bytes live in the same allocation as the key pointers and the value
 * slots — [pointers][slots][characters] — so an object is still one block,
 * one refcount, one free. */
void code_object(CodeValue *out, const char **keys, void *values, long long len) {
    const char **key_buf = NULL;
    void *value_buf = NULL;
    if (len > 0) {
        size_t keys_bytes = (size_t)len * sizeof(const char *);
        size_t slots_bytes = (size_t)len * CODE_VALUE_SLOT_SIZE;
        size_t chars_bytes = 0;
        for (long long i = 0; i < len; i++) {
            chars_bytes += (keys[i] ? strlen(keys[i]) : 0) + 1;
        }
        key_buf = heap_alloc(keys_bytes + slots_bytes + chars_bytes);
        value_buf = (char *)key_buf + keys_bytes;
        char *chars = (char *)value_buf + slots_bytes;
        for (long long i = 0; i < len; i++) {
            key_buf[i] = copy_key(&chars, keys[i]);
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

/* The characters of a Str, for use as an object key by generated code.
 *
 * Infallible on purpose: the only thing that reaches it is a computed key
 * (`{ "$name" = v }`), which is an interpolation, and interpolation renders
 * every value — so it is always a Str. A non-Str would be a codegen bug
 * rather than a program error, and answering "" says so without inventing a
 * failure path for a case that cannot happen. */
const char *code_str_text(const CodeValue *v) {
    return v->tag == CODE_STR && v->str ? v->str : "";
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

/* The two shapes every operand-type message in this file is built from.
 *
 * They exist so the wording lives in one place per shape rather than at each
 * `fail` site, because it has to match `interpreter.rs` *exactly*:
 * `Exception.message` is a value a program can read, so two backends wording
 * the same failure differently is a difference in what a program computes,
 * not a cosmetic one. `tests/message_parity.rs` runs both backends over the
 * same failing programs and compares the text. */
static void operand_message(char *buf, size_t n, const char *requirement, const CodeValue *v) {
    snprintf(buf, n, "%s, found %s %s", requirement, article_for(v), type_name(v));
}

static void fail_operand(const char *requirement, const CodeValue *v) {
    char msg[192];
    operand_message(msg, sizeof msg, requirement, v);
    fail(msg);
}

static void fail_binary(const char *op, const CodeValue *a, const CodeValue *b) {
    char msg[192];
    snprintf(msg, sizeof msg, "cannot apply '%s' to %s %s and %s %s", op, article_for(a),
             type_name(a), article_for(b), type_name(b));
    fail(msg);
}

void code_field(CodeValue *out, const CodeValue *obj, const char *field) {
    if (obj->tag != CODE_OBJECT) {
        char msg[128];
        snprintf(msg, sizeof msg,
                 "cannot read field '%s' of %s %s — '.' requires an object", field,
                 article_for(obj), type_name(obj));
        fail(msg);
        return;
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
    fail(msg);
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
    /* `code_check_emittable` ran at the emit site, so a `_class` is here. A
     * non-Str one is not a class core knows, and core answers null like any
     * other recipient. */
    if (particle->tag != CODE_OBJECT) {
        code_null(out);
        return;
    }
    const CodeValue *class_val = find_field(particle, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        code_null(out);
        return;
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
        /* A field the particle does not carry is null — the same answer
         * `.field` gives — so there is no separate "you didn't supply it"
         * case to report. Emitting a particle is not a form to be validated
         * before the handler may run: `Length { }` means `Length { "value":
         * null }`, and null has no length, which is what the type check below
         * says. (Owner's rule, 2026-08-28; `net` was rewritten around it in
         * phase 2 and this is core catching up.) */
        static const CodeValue absent = {.tag = CODE_NULL};
        const CodeValue *value = find_field(particle, "value");
        if (!value) {
            value = &absent;
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
        /* Core answers rather than unwinding its caller, the same as a
         * module and the same as a handler written in the language: `core` is
         * a recipient like any other, so `emit Length { } to core get r`
         * binds `r` instead of ending the frame that emitted (2026-08-28).
         *
         * Only failures from *here* — after the particle has been accepted
         * and dispatched — answer this way. A malformed emit (`emit 5 to
         * core`) is the emitting frame's own mistake and still fails there,
         * exactly as `emit 5 to this` does. */
        char msg[192];
        operand_message(msg, sizeof msg, "Length requires an array or string 'value'", value);
        code_make_exception(out, "core", msg, NULL);
        return;
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

/* The ring below is touched from two threads once a module has one of its
 * own, so it is locked. Under CODE_WASM there are no native modules at all
 * (`code_native_open` refuses one) and the freestanding shim has no pthreads,
 * so the lock compiles away to nothing there. */
#ifdef CODE_WASM
typedef int CodeMutex;
#define code_mutex_init(m) ((void)(m))
#define code_mutex_lock(m) ((void)(m))
#define code_mutex_unlock(m) ((void)(m))
#else
typedef pthread_mutex_t CodeMutex;
#define code_mutex_init(m) pthread_mutex_init((m), NULL)
#define code_mutex_lock(m) pthread_mutex_lock(m)
#define code_mutex_unlock(m) pthread_mutex_unlock(m)
#endif

typedef struct {
    void (*dispatch)(CodeValue *out, const CodeValue *particle);
    void (*release)(CodeValue *v);
    /* Optional: what the module wants told about the answer to a particle
     * it pushed — the program's handler's return value. NULL when the module
     * does not export `code_module_inbound_reply`, which is most of them: a
     * module that only announces things has no use for the answer. */
    CodeInboundReplyFn reply;
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
    /* Guards the three fields above and the deep copy that fills a slot.
     * Held for the whole of a push so a poll can never see a half-built
     * value. */
    CodeMutex lock;
    /* Optional `code_module_serving`: non-zero while this module still
     * expects to speak, which is what holds the program open after its last
     * statement (see `code_host_park`). NULL for a module that exports none,
     * and for every `.a` — a static module's exports are called by their
     * prefixed names from generated code, so there is no pointer to keep. */
    int (*serving)(void);
    /* Whether the module took the inbound channel — i.e. whether anything
     * but this thread can ever reach the ring. Decides what
     * `code_native_close` may do with the handle. */
    int has_inbound;
    /* The `dlopen` result, kept only so a runtime-linked organelle can be
     * unloaded again (`code_runtime_unlink`). NULL for a `.a`, which was
     * never opened. A top-level `link` never reads it: that module stays
     * mapped for the life of the process, which is what lets an exported
     * value's key strings be borrowed rather than copied (see
     * `code_native_vars_object`). */
    void *lib;
    /* Set when this organelle was supplied by a host rather than opened from
     * a file (`code_abi.h` item 10). Every crossing below — dispatch,
     * exported values, whether it is still serving — goes through `host`
     * instead of the `dlsym`'d pointers above, which are all NULL then. */
    int from_host;
    CodeHostModule host;
    /* Optional `code_module_release` — `code_abi.h` item 9, the point at
     * which the module gives up the top-level values it otherwise owns for
     * its whole lifetime. NULL for every module that has none, which is all
     * of them except a `.code` library. */
    void (*module_release)(void);
    /* Set at cleanup: the program is done, and a push arriving after it is
     * dropped rather than queued. Without it a module thread still running
     * at exit would allocate into a ring nobody will drain, and
     * `code_check_leaks` would report those blocks as a leak — a race
     * between two threads showing up as a flaky failure in an unrelated
     * test. */
    int closed;
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

/* ---- Being hosted -------------------------------------------------------
 *
 * `code_abi.h` item 10, from the guest's side. Installed by whoever opened
 * this module while their program was running; NULL in a program running on
 * its own, which is what keeps every existing application working unchanged.
 *
 * One pair per loaded module, not per link: a `.so` carries its own copy of
 * this runtime, so these statics are private to it and there is exactly one
 * host for it. */
static const CodeHostVtable *code_host = NULL;
static void *code_host_ctx = NULL;

void code_module_set_host(const CodeHostVtable *host, void *host_ctx) {
    code_host = host;
    code_host_ctx = host_ctx;
}

/* Builds a handle around an organelle the host supplied. No `dlopen`, no
 * symbols, no mapping of its own: everything this handle can do, it does by
 * calling back through the host. */
static NativeHandle *host_native(const CodeHostModule *supplied, char *err, size_t errlen) {
    if (!supplied->dispatch || !supplied->release) {
        snprintf(err, errlen, "the host offered an organelle with no dispatch");
        return NULL;
    }
    NativeHandle *nh = malloc(sizeof(NativeHandle));
    if (!nh) {
        code_runtime_error("out of memory");
    }
    memset(nh, 0, sizeof *nh);
    nh->from_host = 1;
    nh->host = *supplied;
    /* A host-supplied organelle never pushes. It cannot: the queue and its
     * drain belong to *this* module's runtime, while the thing actually
     * doing the work lives in the host's. Anything that has to speak first
     * is the host's own organelle, spoken for on the host's side. */
    nh->has_inbound = 0;
    code_mutex_init(&nh->lock);
    return nh;
}

/* Opens `path` and builds the handle around it, or returns NULL with the
 * reason written into `err`.
 *
 * Split out of `code_native_open` because the two ways a link can fail have
 * to differ. A top-level `link` names a module in the source and the program
 * cannot run without it, so failing there ends the process. A `link` that
 * runs inside a handler is opening something the program worked out at run
 * time — a host loading a guest — and there the failure has to be a value
 * the program can answer, not the end of the host. Same opening, two
 * reporting rules, one implementation. */
static NativeHandle *open_native(const char *path, char *err, size_t errlen) {
    /* A host, once installed, is the only way out. Not a fallback to opening
     * the file: a guest that could quietly reach past its host is a guest
     * whose memory cannot be reclaimed and whose reach cannot be bounded —
     * see `code_abi.h` item 10. */
    if (code_host) {
        CodeHostModule supplied = {0};
        if (!code_host->resolve(code_host_ctx, path, &supplied)) {
            snprintf(err, errlen, "organelle '%s' is not offered by the host", path);
            return NULL;
        }
        return host_native(&supplied, err, errlen);
    }
#ifdef CODE_WASM
    (void)path;
    snprintf(err, errlen, "native modules are not available in a wasm build");
    return NULL;
#else
    void *handle = dlopen(path, RTLD_NOW);
    if (!handle) {
        snprintf(err, errlen, "cannot load native module '%s': %s", path, dlerror());
        return NULL;
    }

    uint32_t (*version_fn)(void) = (uint32_t (*)(void))dlsym(handle, "code_module_abi_version");
    if (!version_fn) {
        snprintf(err, errlen, "native module '%s' missing 'code_module_abi_version'", path);
        dlclose(handle);
        return NULL;
    }
    uint32_t version = version_fn();
    if (version != CODE_ABI_VERSION) {
        snprintf(err, errlen, "native module '%s' has ABI version %u (expected %u)", path,
                 (unsigned)version, (unsigned)CODE_ABI_VERSION);
        dlclose(handle);
        return NULL;
    }

    NativeHandle *nh = malloc(sizeof(NativeHandle));
    if (!nh) {
        code_runtime_error("out of memory");
    }
    /* Zeroed whole, not field by field. Every field below is assigned, but
     * that is exactly the promise that broke: a field added to this struct
     * later was set on the other two construction paths and missed here, so
     * a freshly opened module inherited whatever the last freed handle had
     * left in that byte. It read as "this organelle was supplied by a host",
     * and the module was never called at all — the program dispatched into
     * the wrong thing entirely, only when the allocator happened to hand
     * back a dirty block, which made it look like a layout-sensitive
     * corruption for a long time. Starting from zero costs nothing and
     * cannot be forgotten. */
    memset(nh, 0, sizeof *nh);
    nh->lib = handle;
    nh->dispatch = (void (*)(CodeValue *, const CodeValue *))dlsym(handle, "code_module_dispatch");
    nh->release = (void (*)(CodeValue *))dlsym(handle, "code_release");
    if (!nh->dispatch || !nh->release) {
        snprintf(err, errlen, "native module '%s' missing 'code_module_dispatch' or 'code_release'",
                 path);
        free(nh);
        dlclose(handle);
        return NULL;
    }
    /* Optional — a module without it simply has no exported variables. */
    nh->vars = (const CodeVarList *(*)(void))dlsym(handle, "code_module_vars");
    /* Also optional: only a module that wants an answer to what it pushed. */
    nh->reply = (CodeInboundReplyFn)dlsym(handle, "code_module_inbound_reply");
    /* Optional too: a module that holds the program open while it works. */
    nh->serving = (int (*)(void))dlsym(handle, "code_module_serving");
    /* Optional as well, and only a `.code` library has one: the point at
     * which this module may let go of its top-level values (item 9). */
    nh->module_release = (void (*)(void))dlsym(handle, "code_module_release");

    /* Also optional: a module that never speaks first doesn't export it.
     * The pusher handed across is *this* runtime's, not the module's own
     * copy — see code_abi.h for why that distinction matters. */
    memset(nh->inbound, 0, sizeof nh->inbound);
    nh->inbound_head = 0;
    nh->inbound_count = 0;
    nh->closed = 0;
    code_mutex_init(&nh->lock);
    void (*set_inbound)(void *, CodeEmitFn) =
        (void (*)(void *, CodeEmitFn))dlsym(handle, "code_module_set_inbound");
    nh->has_inbound = set_inbound != NULL;
    if (set_inbound) {
        set_inbound(nh, code_emit_inbound);
    }
    return nh;
#endif
}

void *code_native_open(const char *path) {
    char err[256];
    NativeHandle *nh = open_native(path, err, sizeof err);
    if (!nh) {
        code_runtime_error(err);
    }
    return nh;
}

/* A queue for a `.a` static module, and nothing else.
 *
 * A `.so` gets its ring as part of the `NativeHandle` that `code_native_open`
 * builds around a `dlopen` result. A `.a` has no such thing — it is linked
 * straight into this binary, so codegen calls its `<prefix>_code_module_*`
 * functions directly and never needed a handle at all. Which is why static
 * modules could not speak first: there was nowhere to queue *into*, not a
 * decision that they shouldn't.
 *
 * So this allocates the same struct with only the ring live. The three
 * function pointers stay NULL and are never read: dispatch goes direct, there
 * is no per-module `code_release` (one runtime, the host's), and exported
 * variables come through `code_static_vars_object`. `code_native_close` frees
 * it and drains whatever is still queued, exactly as for a `.so`. */
void *code_static_open(void) {
    NativeHandle *nh = malloc(sizeof(NativeHandle));
    if (!nh) {
        code_runtime_error("out of memory");
    }
    /* See `open_native` for why this is a whole-struct zero. */
    memset(nh, 0, sizeof *nh);
    nh->dispatch = NULL;
    nh->release = NULL;
    nh->vars = NULL;
    /* A `.a`'s reply export is called directly by generated code, by its
     * prefixed name, exactly as its dispatch is — there is no pointer to
     * keep here. */
    nh->reply = NULL;
    nh->serving = NULL;
    /* A `.a` was linked into this binary, not opened, so there is nothing to
     * unload and no separate release point — its cleanup is the program's. */
    nh->lib = NULL;
    nh->module_release = NULL;
    nh->from_host = 0;
    memset(&nh->host, 0, sizeof nh->host);
    memset(nh->inbound, 0, sizeof nh->inbound);
    nh->inbound_head = 0;
    nh->inbound_count = 0;
    nh->closed = 0;
    /* A `.a` is only given a handle at all because it declared an inbound
     * export — that is what `loader.rs` looks for before emitting the call. */
    nh->has_inbound = 1;
    code_mutex_init(&nh->lock);
    return nh;
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
 * Callable from a thread the program knows nothing about: a module that
 * spawns one (a timer, a socket accept loop) pushes from there, which is what
 * makes an event loop more than polling. Everything that costs is inside the
 * lock — the ring's three fields *and* the deep copy that fills a slot, so a
 * poll never sees a half-built value. The copy allocates, which is why
 * `live_blocks` is atomic and `code_release`'s work stack is thread-local;
 * see both.
 *
 * The rest of the runtime stays single-threaded and unlocked. That holds
 * because a pushed value is only ever reachable from one thread at a time:
 * the pusher builds it alone, the ring holds it under this lock, and the
 * program owns it alone once `code_poll_inbound` hands it over. */

/* How long `code_host_park` sleeps before re-asking whether anything is still
 * serving. **Not a poll interval** — delivery is exact, since every push
 * signals below. This only bounds how long it takes to notice a module that
 * *stopped* serving without pushing on its way out. `interpreter.rs`'s
 * `SERVING_RECHECK` is the same number, because the two output modes have to
 * idle the same way. */
#define CODE_SERVING_RECHECK_SECONDS 1

#ifndef CODE_WASM
/* Raised by every push, waited on by `code_host_park`.
 *
 * One signal for every module rather than one per ring: the program waits for
 * *something* to arrive, not for a particular module to speak, and a condvar
 * per ring would mean choosing which one to sleep on. `interpreter.rs` keeps a
 * single pair for exactly the same reason.
 *
 * A count, not a bare signal, because the two sides race by design: a push can
 * land between the drain that emptied the rings and the park that follows it.
 * A signal sent in that window would be sent to nobody. A count survives the
 * gap — the parker sees it is already non-zero and returns without sleeping. */
static pthread_mutex_t code_wakeup_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t code_wakeup_cond = PTHREAD_COND_INITIALIZER;
static unsigned long code_wakeups = 0;
#endif

void code_emit_inbound(void *queue, const CodeValue *value) {
    if (!queue || !value) {
        return;
    }
    NativeHandle *nh = (NativeHandle *)queue;
    code_mutex_lock(&nh->lock);
    if (nh->closed) {
        /* The program has finished and drained. Nothing would ever read this,
         * and allocating it would read as a leak. */
        code_mutex_unlock(&nh->lock);
        return;
    }
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
    code_mutex_unlock(&nh->lock);
#ifndef CODE_WASM
    /* After the ring's own lock is released, never while holding it: the
     * parker wakes into a drain, and waking it inside that critical section
     * only means it blocks again on the way out. */
    pthread_mutex_lock(&code_wakeup_lock);
    code_wakeups++;
    pthread_cond_broadcast(&code_wakeup_cond);
    pthread_mutex_unlock(&code_wakeup_lock);
#endif
}

/* Whether one linked module still expects to speak. Generated code asks this
 * of every `.so` handle it holds; the answer decides whether the program stays
 * up past its last statement. A module exporting no `code_module_serving`
 * holds nothing open, which is what keeps every program that ever worked
 * ending exactly when it used to. */
int code_native_serving(void *handle) {
    if (!handle) {
        return 0;
    }
    NativeHandle *nh = (NativeHandle *)handle;
    return nh->serving ? nh->serving() : 0;
}

/* Sleep until something is pushed, or until it is time to re-ask who is still
 * serving. Called once per iteration of the keep-alive loop generated at the
 * end of `main` (see codegen.rs's `gen_keep_alive`).
 *
 * This is why an application writes no keep-alive loop of its own, and why
 * that loop could never have been a plain `join`: a pushed particle is
 * dispatched to the program's own handlers, which run on *this* thread
 * between statements. A thread blocked in `join` is not between statements,
 * and every request would time out one frame below the handler that should
 * have answered it. So: park, wake on a push, drain, park again. */
void code_host_park(void) {
#ifndef CODE_WASM
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += CODE_SERVING_RECHECK_SECONDS;

    pthread_mutex_lock(&code_wakeup_lock);
    while (code_wakeups == 0) {
        /* Non-zero is a timeout (or an error we treat as one): stop waiting
         * and let the caller re-ask whether anything is still serving. */
        if (pthread_cond_timedwait(&code_wakeup_cond, &code_wakeup_lock, &deadline) != 0) {
            break;
        }
    }
    /* Consumed whole: the drain that follows hands over everything queued in
     * one pass, so one park answers every push that led to it. */
    code_wakeups = 0;
    pthread_mutex_unlock(&code_wakeup_lock);
#endif
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
    code_mutex_lock(&nh->lock);
    if (nh->inbound_count == 0) {
        code_mutex_unlock(&nh->lock);
        return 0;
    }
    int slot = nh->inbound_head;
    code_copy(out, &nh->inbound[slot]);
    code_release(&nh->inbound[slot]);
    memset(&nh->inbound[slot], 0, sizeof(CodeValue));
    nh->inbound_head = (nh->inbound_head + 1) % CODE_INBOUND_CAPACITY;
    nh->inbound_count--;
    code_mutex_unlock(&nh->lock);
    return 1;
}

/* Hands a module the answer to a particle it pushed: whatever the program's
 * handler returned, or null when nothing handled it. Called by the generated
 * drain after each dispatch (see codegen.rs's `gen_drain_body`), and a no-op
 * for the modules — most of them — that export no
 * `code_module_inbound_reply`.
 *
 * `particle` and `result` stay the host's. The module reads what it needs
 * during the call and copies it out; nothing is retained across the return,
 * which is the same boundary rule every other crossing here follows. */
void code_native_reply(void *handle, const CodeValue *particle, const CodeValue *result) {
    if (!handle) {
        return;
    }
    NativeHandle *nh = (NativeHandle *)handle;
    if (nh->reply) {
        nh->reply(particle, result);
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
    if (!handle) {
        return;
    }
    NativeHandle *nh = (NativeHandle *)handle;
    /* Anything still queued at exit is this runtime's to free — the
     * "owns nothing when it exits" rule `code_check_leaks` enforces. */
    code_mutex_lock(&nh->lock);
    for (int i = 0; i < nh->inbound_count; i++) {
        code_release(&nh->inbound[(nh->inbound_head + i) % CODE_INBOUND_CAPACITY]);
    }
    nh->inbound_count = 0;
    nh->closed = 1;
    int has_inbound = nh->has_inbound;
    code_mutex_unlock(&nh->lock);
    if (has_inbound) {
        /* Deliberately not freed. A module that took the inbound channel may
         * still have a thread holding this pointer, and there is no way to
         * ask it to stop — the ABI has no shutdown call, on purpose (a module
         * that must be asked politely before the program may exit is a module
         * that can hang it). Leaving the struct mapped, with `closed` set,
         * turns a late push into a no-op instead of a use-after-free. It is
         * one small malloc per linked module, and `code_check_leaks` doesn't
         * see it: this is not a refcounted block. */
        return;
    }
    free(nh);
}

/* `emit <particle> to <alias> [get <name>]` for a linked native module.
 * `handle` is whatever `code_native_open` returned for that alias. */
void code_native_dispatch(void *handle, CodeValue *out, const CodeValue *particle) {
    code_null(out);
    if (handle && ((NativeHandle *)handle)->from_host) {
        /* Answered through the host, and the deep copy still happens: the
         * answer was built by the host's copy of this runtime, so it is no
         * more ours to keep than a `.so`'s would be. */
        NativeHandle *nh = (NativeHandle *)handle;
        CodeValue result = {0};
        nh->host.dispatch(nh->host.ctx, &result, particle);
        code_native_copy_in(out, &result);
        nh->host.release(nh->host.ctx, &result);
        return;
    }
    if (!handle) {
        /* Only reachable through a stale alias: a library that was released
         * and then dispatched to without being initialised again. Saying so
         * beats dereferencing the closed handle. */
        fail("this organelle was released");
        return;
    }
    NativeHandle *nh = (NativeHandle *)handle;
    CodeValue result = {0};
    nh->dispatch(&result, particle);
    code_native_copy_in(out, &result);
    nh->release(&result);
}

/* ---- Organelles linked while the program is running ---------------------
 *
 * `link <expr> as <name>` inside a handler (see `ast::Stmt::LinkRuntime`).
 * Everything below exists because such an organelle has no alias to be found
 * by: the program holds it as an ordinary value and may pass it around, so
 * this table is the only thing that outlives the binding.
 *
 * Rows are appended and never reused, even after `unlink` empties one. Reuse
 * would turn a stale address into a *live* one naming an unrelated
 * organelle — the exact failure this table exists to prevent — and an
 * ever-growing array of NULLs is much the cheaper problem.
 *
 * Not locked. Every one of these runs on the thread executing the program's
 * statements, the same thread that drains the inbound ring, and an organelle
 * that could speak from a thread of its own is refused at link time. */
typedef struct HostedGuest HostedGuest;
typedef struct {
    NativeHandle *handle;
    /* The path this row was opened from, owned here — what a second `link`
     * of the same file is matched against. NULL once the row is empty. */
    char *path;
    /* How many live addresses name this row.
     *
     * `dlopen` returns the *same* mapping for a path already open, so two
     * `link`s of one file are two names for one organelle whether this
     * counts them or not. Counting is what makes that safe: without it the
     * first `unlink` runs the module's release point and the second address
     * goes on talking to an organelle that has already let go of everything
     * it owned — measured, and it answers with freed memory rather than
     * failing. So the language matches the loader instead of fighting it:
     * same file, same organelle, same address, released once the last name
     * for it is gone. */
    long long refs;
    /* Which guest row this program keeps for it, or -1 for an organelle that
     * cannot be hosted. A row, not a pointer — see the hosting tables. */
    long long guest;
} RuntimeOrganelle;

static RuntimeOrganelle *runtime_organelles = NULL;
static long long runtime_organelle_count = 0;
static long long runtime_organelle_cap = 0;

/* The field an address value carries, kept in step with
 * `interpreter::ORGANELLE_FIELD` — the two backends mint the same value. */
#define CODE_ORGANELLE_FIELD "_organelle"

/* The row an address names, or -1 with the reason failed. Strict about the
 * shape on purpose: an address is something the runtime minted, so anything
 * else is a program mistake worth naming precisely rather than a lookup that
 * quietly finds nothing. */
static long long organelle_row(const CodeValue *address) {
    if (address->tag != CODE_OBJECT) {
        fail("expected an organelle address (from a 'link' inside a handler)");
        return -1;
    }
    const CodeValue *row = find_field(address, CODE_ORGANELLE_FIELD);
    if (!row || row->tag != CODE_NUMBER || row->number < 0) {
        fail("expected an organelle address (from a 'link' inside a handler)");
        return -1;
    }
    return (long long)row->number;
}

/* The organelle a valid address names, or NULL with the reason failed. Both
 * readings of "nothing here" — a row past the end and a row `unlink` emptied
 * — are the same mistake seen from the program's side, so they read alike. */
static NativeHandle *organelle_at(const CodeValue *address) {
    long long row = organelle_row(address);
    if (row < 0) {
        return NULL;
    }
    if (row >= runtime_organelle_count || !runtime_organelles[row].handle) {
        fail("this organelle has been unlinked");
        return NULL;
    }
    return runtime_organelles[row].handle;
}

/* Writes the address value for `row` into `out`. */
static void organelle_address(CodeValue *out, long long row) {
    const char *keys[1] = {CODE_ORGANELLE_FIELD};
    _Alignas(8) char slots[CODE_VALUE_SLOT_SIZE] = {0};
    code_number(slot_at(slots, 0), (double)row);
    code_object(out, keys, slots, 1);
    code_release(slot_at(slots, 0));
}

/* ---- Hosting (debug build) ---------------------------------------------- */
struct HostedGuest {
    char *app;
};

typedef struct {
    long long guest;
    char *name;
    int offered;
} HostedOrganelle;

static HostedGuest *hosted_guests = NULL;
static long long hosted_guest_count = 0;
static long long hosted_guest_cap = 0;
static HostedOrganelle *hosted_organelles = NULL;
static long long hosted_organelle_count = 0;
static long long hosted_organelle_cap = 0;

static void *row_handle(long long row) { return (void *)(uintptr_t)(row + 1); }
static long long handle_row(void *handle) { return (long long)(uintptr_t)handle - 1; }

/* The program's own dispatch chain — what `emit ... to this` calls. Filled
 * in at startup by generated code, because only codegen knows the chain's
 * name; NULL in a program with no handlers at all, which then offers
 * nothing. */
static void (*code_program_dispatch)(CodeValue *out, const CodeValue *particle) = NULL;

void code_set_program_dispatch(void (*fn)(CodeValue *out, const CodeValue *particle)) {
    code_program_dispatch = fn;
}

/* `name` for the particle: the file's stem, not the path the guest wrote.
 * A host's handler wants to say `if name = "net_server"`, and matching on
 * whatever spelling happened to be baked into the guest — a bare name in one
 * build, a directory-qualified one in another — would make that handler
 * depend on how the guest was compiled. */
static void organelle_stem(const char *ref, char *out, size_t outlen) {
    const char *start = ref;
    for (const char *c = ref; *c; c++) {
        if (*c == '/') start = c + 1;
    }
    size_t n = strlen(start);
    if (n > 3 && strcmp(start + n - 3, ".so") == 0) n -= 3;
    if (n >= outlen) n = outlen - 1;
    memcpy(out, start, n);
    out[n] = '\0';
}

static void ask_program(CodeValue *out, const char *class_name, const char *app, const char *name,
                        const CodeValue *extra) {
    code_null(out);
    if (!code_program_dispatch) return;
    const char *keys[4] = {"_class", "app", "name", "particle"};
    _Alignas(8) char slots[4 * CODE_VALUE_SLOT_SIZE] = {0};
    code_str(slot_at(slots, 0), class_name);
    code_str_owned(slot_at(slots, 1), app);
    code_str_owned(slot_at(slots, 2), name);
    long long len = 3;
    if (extra) {
        /* `code_native_copy_in`, not `code_copy`. `extra` is the particle a
         * *guest* sent, built by the guest's own copy of this runtime with
         * its own refcounts — retaining it here would count it in the host's
         * bookkeeping and free it in the guest's. Values never cross this
         * boundary by shared ownership. */
        code_native_copy_in(slot_at(slots, 3), extra);
        len = 4;
    }
    CodeValue particle = {0};
    code_object(&particle, keys, slots, len);
    for (long long i = 0; i < len; i++) code_release(slot_at(slots, i));
    code_program_dispatch(out, &particle);
    code_release(&particle);
}

static int is_class(const CodeValue *v, const char *class_name) {
    if (v->tag != CODE_OBJECT) return 0;
    const CodeValue *cls = find_field(v, "_class");
    return cls && cls->tag == CODE_STR && cls->str && strcmp(cls->str, class_name) == 0;
}

/* What a guest's `emit ... to <organelle>` becomes: an `Organelle` particle
 * asked of the host's own handlers, on the host's thread, as an ordinary
 * nested handler call.
 *
 * Nested rather than queued, and that is the whole reason it works. A queue
 * is drained between the program's statements, and this call happens *during*
 * one — the host is inside the emit that reached the guest in the first
 * place. `code_abi.h` item 8 describes that trap from the other side. The
 * existing re-entry guard still applies, so a host whose answer loops back
 * into the same guest gets an `Exception` rather than a hang. */
static void hosted_dispatch(void *ctx, CodeValue *out, const CodeValue *particle) {
    long long row = handle_row(ctx);
    if (row < 0 || row >= hosted_organelle_count || !hosted_organelles[row].name) {
        code_make_exception(out, "host", "this organelle's application has been stopped", NULL);
        return;
    }
    HostedOrganelle *o = &hosted_organelles[row];
    if (!o->offered) {
        char msg[256];
        snprintf(msg, sizeof msg, "organelle '%s' is not offered by the host", o->name);
        code_make_exception(out, "host", msg, NULL);
        return;
    }
    const char *app = hosted_guests[o->guest].app;
    ask_program(out, "Organelle", app ? app : "", o->name, particle);
}

static void hosted_release(void *ctx, CodeValue *v) {
    (void)ctx;
    code_release(v);
}

/* A guest is asking for an organelle. The program decides. */
static int hosted_resolve(void *host_ctx, const char *ref, CodeHostModule *out) {
    long long guest = handle_row(host_ctx);
    if (guest < 0 || guest >= hosted_guest_count || !hosted_guests[guest].app) return 0;
    char name[128];
    organelle_stem(ref, name, sizeof name);
    CodeValue answer = {0};
    ask_program(&answer, "Offer", hosted_guests[guest].app, name, NULL);
    int offered = is_class(&answer, "Offered");
    code_release(&answer);

    /* A refusal is never a *failure to resolve*, and this is the one place
     * that distinction decides whether a host survives its guests.
     *
     * The ABI lets a host answer "I do not offer that", and the guest's
     * `link` then fails. But a guest's top-level `link` failing ends the
     * guest — and a fatal error inside a module ends the process it was
     * loaded into. So a host that refused an organelle would be killed by
     * its own policy, by a guest it deliberately said no to. Measured, and
     * exactly backwards.
     *
     * So a refused organelle is handed over as an organelle that refuses:
     * the guest links it, and every particle it sends gets an `Exception`.
     * That is the language's own rule everywhere else — trouble is a value,
     * not the end of the program. */
    if (hosted_organelle_count == hosted_organelle_cap) {
        long long cap = hosted_organelle_cap ? hosted_organelle_cap * 2 : 8;
        HostedOrganelle *grown = realloc(hosted_organelles, (size_t)cap * sizeof(HostedOrganelle));
        if (!grown) code_runtime_error("out of memory");
        hosted_organelles = grown;
        hosted_organelle_cap = cap;
    }
    char *kept = malloc(strlen(name) + 1);
    if (!kept) code_runtime_error("out of memory");
    memcpy(kept, name, strlen(name) + 1);
    long long row = hosted_organelle_count++;
    hosted_organelles[row].guest = guest;
    hosted_organelles[row].name = kept;
    hosted_organelles[row].offered = offered;
    out->dispatch = hosted_dispatch;
    out->release = hosted_release;
    /* No exported values and nothing held open. A stand-in is reached only
     * by `emit`, and what actually holds the program up is the host's own
     * organelle, which the host holds directly. */
    out->vars = NULL;
    out->serving = NULL;
    out->ctx = row_handle(row);
    return 1;
}

static const CodeHostVtable hosted_vtable = {hosted_resolve};

static long long open_hosted_guest(const char *path) {
    if (hosted_guest_count == hosted_guest_cap) {
        long long cap = hosted_guest_cap ? hosted_guest_cap * 2 : 8;
        HostedGuest *grown = realloc(hosted_guests, (size_t)cap * sizeof(HostedGuest));
        if (!grown) code_runtime_error("out of memory");
        hosted_guests = grown;
        hosted_guest_cap = cap;
    }
    char *kept = malloc(strlen(path) + 1);
    if (!kept) code_runtime_error("out of memory");
    memcpy(kept, path, strlen(path) + 1);
    long long row = hosted_guest_count++;
    hosted_guests[row].app = kept;
    return row;
}

/* Empties a guest's row and every stand-in handed out on its behalf. Their
 * handles stay valid *as handles* — they simply name nothing now, and answer
 * so. */
static void close_hosted_guest(long long guest) {
    if (guest < 0 || guest >= hosted_guest_count) return;
    for (long long i = 0; i < hosted_organelle_count; i++) {
        if (hosted_organelles[i].name && hosted_organelles[i].guest == guest) {
            free(hosted_organelles[i].name);
            hosted_organelles[i].name = NULL;
        }
    }
    free(hosted_guests[guest].app);
    hosted_guests[guest].app = NULL;
}

/* `link <path> as <name>` inside a handler: opens the organelle and answers
 * with the address value naming it. On any failure `out` is null and the
 * frame's landing block turns the failure into an `Exception` — a host must
 * survive a guest it cannot load. */
void code_runtime_link(CodeValue *out, const CodeValue *path) {
    code_null(out);
    if (path->tag != CODE_STR) {
        fail("'link' needs a path");
        return;
    }
    const char *text = path->str ? path->str : "";
    /* Only a `.so`. A `.code` source would mean adding handlers while the
     * program runs — deliberately out of scope — and a `.a` is already part
     * of this binary and has nothing to open. Checked on the value rather
     * than in the parser because there is no value until now. */
    size_t n = strlen(text);
    if (n < 3 || strcmp(text + n - 3, ".so") != 0) {
        char msg[256];
        snprintf(msg, sizeof msg,
                 "'link %s' inside a handler can only open an organelle ('.so')", text);
        fail(msg);
        return;
    }

    /* `dlopen` only treats its argument as a *path* when it contains a
     * slash; a bare name it looks for the way it looks for a shared library,
     * along the loader's search paths — so `link "guest.so"` would quietly
     * miss the file sitting right there and report it as absent. A top-level
     * `link` never runs into this because `loader.rs` has already turned the
     * spelling into a real path before the runtime sees it. Here there is no
     * such pass, so "taken as written" has to be made to mean "as a path",
     * which is what a program that just built a path out of a directory and
     * a name meant by it. */
    char rooted[512];
    int has_slash = 0;
    /* A loop rather than `strchr`: the freestanding wasm shim declares only
     * the handful of string functions this file actually needed, and one
     * more would be a header change for a single character search. */
    for (const char *c = text; *c; c++) {
        if (*c == '/') {
            has_slash = 1;
            break;
        }
    }
    if (!has_slash) {
        snprintf(rooted, sizeof rooted, "./%s", text);
        text = rooted;
    }

    /* Already open? Then this is another name for the same organelle — see
     * `RuntimeOrganelle.refs`. Answering with the same address is the honest
     * result: they are the same thing. */
    for (long long i = 0; i < runtime_organelle_count; i++) {
        if (runtime_organelles[i].handle && strcmp(runtime_organelles[i].path, text) == 0) {
            runtime_organelles[i].refs++;
            organelle_address(out, i);
            return;
        }
    }

    char err[256];
    NativeHandle *nh = open_native(text, err, sizeof err);
    if (!nh) {
        fail(err);
        return;
    }
    /* An organelle that speaks first cannot be linked here. The drain runs
     * over the modules known when the program started, so a queue that
     * appears later is never read and nothing it pushed would ever be
     * handled — a silence far worse than a refusal. */
    if (nh->has_inbound) {
        char msg[256];
        snprintf(msg, sizeof msg,
                 "'link %s' inside a handler: this organelle speaks first, and only "
                 "organelles linked at the top level are ever listened to",
                 text);
        fail(msg);
        code_native_close(nh);
        return;
    }

    if (runtime_organelle_count == runtime_organelle_cap) {
        long long cap = runtime_organelle_cap ? runtime_organelle_cap * 2 : 8;
        RuntimeOrganelle *grown =
            realloc(runtime_organelles, (size_t)cap * sizeof(RuntimeOrganelle));
        if (!grown) {
            code_runtime_error("out of memory");
        }
        runtime_organelles = grown;
        runtime_organelle_cap = cap;
    }
    long long row = runtime_organelle_count++;
    /* Become its host, if it can be hosted. From here on every `link` inside
     * this organelle asks this program's handlers instead of the
     * filesystem — which is what lets a guest share what the host already
     * has rather than opening its own. An organelle built before this
     * existed has no such symbol and is simply left to open its own; it can
     * still be linked and talked to, it just cannot be furnished. */
    long long guest = -1;
#ifndef CODE_WASM
    void (*set_host)(const CodeHostVtable *, void *) =
        (void (*)(const CodeHostVtable *, void *))dlsym(nh->lib, "code_module_set_host");
    if (set_host) {
        guest = open_hosted_guest(text);
        /* Before anything else touches the module. A `.code` library runs
         * its top level lazily, on the first dispatch or the first read of
         * its values, and its own `link`s run with it — installing this
         * afterwards would be too late for exactly the statements it exists
         * to intercept. */
        set_host(&hosted_vtable, row_handle(guest));
    }
#endif
    /* Not `heap_alloc`: this is bookkeeping, not a `CodeValue` block, and
     * must not move the leak counter. */
    size_t kept_len = strlen(text);
    char *kept = malloc(kept_len + 1);
    if (!kept) {
        code_runtime_error("out of memory");
    }
    memcpy(kept, text, kept_len + 1);
    runtime_organelles[row].handle = nh;
    runtime_organelles[row].path = kept;
    runtime_organelles[row].refs = 1;
    runtime_organelles[row].guest = guest;

    organelle_address(out, row);
}

/* Drops one name for a row, releasing and unloading the organelle once the
 * last one is gone. Shared by `code_runtime_unlink` and the end-of-program
 * sweep, which differ only in how they find the row. */
static void release_organelle(long long row) {
    RuntimeOrganelle *slot = &runtime_organelles[row];
    if (--slot->refs > 0) {
        return;
    }
    NativeHandle *nh = slot->handle;
    /* Order is the whole of it: the module's own release point runs while
     * its code is still mapped, and only then is the mapping dropped.
     * Reversed, the release would be a call into unmapped memory. */
    if (nh->module_release) {
        nh->module_release();
    }
    void *lib = nh->lib;
    /* Frees the handle and drains anything still queued. Safe to free here,
     * unlike at program exit, precisely because `has_inbound` was refused at
     * link time: no other thread holds this pointer. */
    code_native_close(nh);
    close_hosted_guest(slot->guest);
    free(slot->path);
    slot->handle = NULL;
    slot->path = NULL;
#ifndef CODE_WASM
    if (lib) {
        dlclose(lib);
    }
#else
    (void)lib;
#endif
}

/* `unlink <address>` — the symmetric half.
 *
 * Order is the whole of it: the module's own release point runs *first*,
 * while its code is still mapped, and only then is the mapping dropped.
 * Reversed, the release would be a call into unmapped memory. */
void code_runtime_unlink(const CodeValue *address) {
    if (!organelle_at(address)) {
        return;
    }
    release_organelle(organelle_row(address));
}

/* Closes whatever is still linked when the program ends — the same "owns
 * nothing when it exits" rule every `CodeValue` slot is already held to, and
 * for the same reason: a guest still holding its world at exit is a guest
 * whose release point never ran, which is exactly the thing `unlink` exists
 * to guarantee. Called from the sweep generated at the end of `main`, before
 * `code_check_leaks` looks. */
void code_runtime_unlink_all(void) {
    for (long long i = 0; i < runtime_organelle_count; i++) {
        while (runtime_organelles[i].handle) {
            release_organelle(i);
        }
    }
    free(runtime_organelles);
    runtime_organelles = NULL;
    runtime_organelle_count = 0;
    runtime_organelle_cap = 0;
}

/* `emit <particle> to <address>` — the runtime-linked half of
 * `code_native_dispatch`, which takes an alias's handle directly. */
void code_runtime_dispatch(CodeValue *out, const CodeValue *address, const CodeValue *particle) {
    code_null(out);
    NativeHandle *nh = organelle_at(address);
    if (!nh) {
        return;
    }
    code_native_dispatch(nh, out, particle);
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
        fail_operand("loop requires an array or object", v);
        return 0;
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
/* Whether `v` can be emitted at all: emitting is dispatch by `_class`, so a
 * value carrying none is not a particle and there is nothing to dispatch on.
 * Deliberately *not* the same question as "does anyone handle this class" —
 * that one answers null, because sending a particle is not a demand.
 *
 * Called once by generated code before the target is chosen (codegen.rs's
 * `gen_emit`), which is why `code_core_dispatch` below no longer asks: a
 * non-particle `emit` is the emitting frame's own mistake, not something a
 * recipient did, and a module could never have asked at all — it reads
 * `_class`, finds none, and cannot tell "not a particle" from "a class I
 * don't handle". Must match interpreter.rs's `check_emittable` exactly. */
void code_check_emittable(const CodeValue *v) {
    if (v->tag == CODE_OBJECT) {
        for (long long i = 0; i < v->len; i++) {
            if (strcmp(v->keys[i], "_class") == 0) {
                return;
            }
        }
    }
    char msg[160];
    snprintf(msg, sizeof msg,
             "emit requires a particle — an object with a '_class' field — found %s %s",
             article_for(v), type_name(v));
    fail(msg);
}

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
    fail(msg);
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
 * common case, and it is exact.
 *
 * The fractional path needs exactly two things a freestanding build cannot
 * compute for itself — the exact expansion, and reading a candidate back —
 * and they are the two helpers below. Everything between them, the rounding
 * rule included, is the same code on every target, so wasm and native agree
 * by construction rather than by two implementations happening to match.
 * Until 2026-08-29 wasm had no answer for either and a fractional number was
 * a loud error there; see docs/todo/wasm-fractional-number-text.md. */

/* The exact decimal expansion, to 41 significant digits. */
static void number_exact(char *out, size_t cap, double d) {
#ifdef CODE_WASM
    int written = code_host_number_exact(d, out, (unsigned int)cap);
    if (written < 0 || (size_t)written >= cap) {
        code_runtime_error("the host could not render a number as text");
    }
    out[written] = '\0';
#else
    snprintf(out, cap, "%.40e", d);
#endif
}

/* Reading one back — the round-trip half of "shortest that round-trips". */
static double number_parse(const char *text, size_t len) {
#ifdef CODE_WASM
    return code_host_number_parse(text, (unsigned int)len);
#else
    (void)len;
    return strtod(text, NULL);
#endif
}

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
    /* 41 significant digits: more than the 17 any double needs to round-trip,
     * so `full` is the exact expansion as far as the rounding below can care. */
    char exact[80];
    number_exact(exact, sizeof exact, d);
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
        if (number_parse(sci, o) == d) {
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
     * `code_object` exactly — one allocation holding
     * `[keys...][values...][characters...]`, with the key characters copied
     * in rather than borrowed from the operand that supplied them.
     *
     * Copied, not borrowed, since 2026-08-29: this used to keep the
     * operand's pointers, on the reasoning that a key's storage outlives the
     * program. That was true while every key was a program literal in
     * read-only data, and stopped being true the day `{ "$name" = v }` began
     * building one at run time — `code_object` started copying then, and
     * this was missed. What it cost: `acc = acc + { "$k" = v }` in a loop
     * left `acc` naming characters inside the literal's block, which the
     * next iteration released, so the merged object's field names were read
     * out of freed memory. It survived on borrowed time, reading bytes that
     * happened not to have been handed out again yet. */
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
            size_t slots_bytes = (size_t)total * CODE_VALUE_SLOT_SIZE;
            size_t chars_bytes = 0;
            for (long long i = 0; i < a->len; i++) {
                chars_bytes += (a->keys[i] ? strlen(a->keys[i]) : 0) + 1;
            }
            for (long long j = 0; j < b->len; j++) {
                if (find_field(a, b->keys[j]) == NULL) {
                    chars_bytes += (b->keys[j] ? strlen(b->keys[j]) : 0) + 1;
                }
            }
            key_buf = heap_alloc(keys_bytes + slots_bytes + chars_bytes);
            value_buf = (char *)key_buf + keys_bytes;
            char *chars = (char *)value_buf + slots_bytes;
            long long n = 0;
            for (long long i = 0; i < a->len; i++) {
                const CodeValue *override_val = find_field(b, a->keys[i]);
                const CodeValue *src = override_val ? override_val : slot_at(a->items, i);
                key_buf[n] = copy_key(&chars, a->keys[i]);
                code_retain(src);
                *slot_at(value_buf, n) = *src;
                n++;
            }
            for (long long j = 0; j < b->len; j++) {
                if (find_field(a, b->keys[j]) != NULL) {
                    continue;
                }
                key_buf[n] = copy_key(&chars, b->keys[j]);
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
    /* A string on either side makes `+` string concatenation: the other
     * operand is rendered exactly as `code_to_text` (string interpolation)
     * renders it. The array branch above already returned for every
     * string-and-array pairing, and string-and-object stays a type error —
     * both container kinds are excluded here and fall through to
     * `fail_binary`. Mirrors interpreter.rs's `Str`-on-either-side arms. */
    if ((a->tag == CODE_STR || b->tag == CODE_STR)
        && a->tag != CODE_ARRAY && b->tag != CODE_ARRAY
        && a->tag != CODE_OBJECT && b->tag != CODE_OBJECT) {
        CodeValue ta = {0};
        CodeValue tb = {0};
        code_to_text(&ta, a);
        code_to_text(&tb, b);
        size_t la = strlen(ta.str);
        size_t lb = strlen(tb.str);
        char *buf = heap_alloc(la + lb + 1);
        memcpy(buf, ta.str, la);
        memcpy(buf + la, tb.str, lb);
        buf[la + lb] = '\0';
        code_release(&ta);
        code_release(&tb);
        /* `ta`/`tb` are independent copies, so `buf` holds no reference into
         * the operands — releasing `out` here is safe even when `out` is `a`
         * or `b` (`s = s + 1`), the same ordering point as the cases above. */
        code_release(out);
        out->tag = CODE_STR;
        out->heap = 1;
        out->str = buf;
        return;
    }
    fail_binary("+", a, b);
}

/* Every failing branch below leaves `out` exactly as it found it, rather than
 * writing a placeholder. That is safe and deliberate: `out` is either a
 * zero-initialized slot or still holds its previous value, so it is a valid
 * `CodeValue` that the frame's cleanup sweep can release exactly once — and
 * writing null instead would have to reason about `out` aliasing `a` or `b`
 * (`x = x / x`) for no gain, since the caller branches away without reading
 * it. */
void code_sub(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        code_number(out, a->number - b->number);
        return;
    }
    fail_binary("-", a, b);
}

void code_mul(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        code_number(out, a->number * b->number);
        return;
    }
    fail_binary("*", a, b);
}

void code_div(CodeValue *out, const CodeValue *a, const CodeValue *b) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        if (b->number == 0.0) {
            /* Not Infinity: the value model is JSON, which has no way to
             * represent that (see ast.rs's BinOp doc comment). */
            fail("division by zero");
            return;
        }
        code_number(out, a->number / b->number);
        return;
    }
    fail_binary("/", a, b);
}

/* -1/0/1 for two Numbers; fails for anything else, strings included —
 * ordering is Number-only (see ast.rs's BinOp doc comment). codegen.rs turns
 * the result into `<`/`>`/`≤`/`≥` with a plain LLVM icmp against 0 — one
 * runtime function instead of four.
 *
 * The 0 on the failing path is not an answer, it is a value to return with:
 * the caller checks `code_failed` before it looks at this at all. Same for
 * `code_bool_value` and `code_iter_len` below — the three helpers whose
 * result is a plain integer rather than a `CodeValue*` out-parameter, which
 * is exactly why the channel is a flag and not a status return. */
long long code_compare(const CodeValue *a, const CodeValue *b, const char *op) {
    if (a->tag == CODE_NUMBER && b->tag == CODE_NUMBER) {
        if (a->number < b->number) {
            return -1;
        }
        return a->number > b->number ? 1 : 0;
    }
    /* `op` exists only for this message. Ordering still goes through one
     * runtime call rather than four (codegen turns the result into
     * `<`/`>`/`≤`/`≥` with an icmp), but "cannot order these values" could
     * not say which operator the program actually wrote, and
     * interpreter.rs's version always could. */
    fail_binary(op, a, b);
    return 0;
}

void code_neg(CodeValue *out, const CodeValue *a) {
    if (a->tag == CODE_NUMBER) {
        code_number(out, -a->number);
        return;
    }
    char msg[96];
    snprintf(msg, sizeof msg, "cannot negate %s %s", article_for(a), type_name(a));
    fail(msg);
}

void code_not(CodeValue *out, const CodeValue *a) {
    if (a->tag == CODE_BOOL) {
        code_bool(out, !a->boolean);
        return;
    }
    fail_operand("'not' requires a boolean", a);
}

/* `expr is ClassName` — the type test (see ast.rs's `Expr::Is`): 1 when
 * `a` is an object whose `"_class"` field holds the string `name`, 0 for
 * everything else. Total by design — a missing `_class` or a non-object
 * operand simply answers 0, mirroring how `find_field` reports absence as
 * null and equality turns that into false. Must match interpreter.rs's
 * `Expr::Is` arm exactly. */
/* `x is String` and its five siblings. The kinds are exactly `CodeTag`, so
 * this is one integer compare — codegen passes the tag rather than a name,
 * since which six exist is settled at compile time and a string comparison
 * would be answering a question nobody asked. A particle is an Object, so
 * `p is Object` and `p is Reply` are both true of the same value. */
int code_is_kind(const CodeValue *a, int tag) {
    return a->tag == (CodeTag)tag ? 1 : 0;
}

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

/* Used by `and`/`or`/`if` codegen to check an operand is actually a bool
 * before branching on it. `requirement` is the whole clause, not just the
 * operator name — `if` is not an operator and wants "if requires a boolean",
 * not "'if' requires booleans". codegen.rs passes exactly what
 * interpreter.rs's matching arm formats. */
int code_bool_value(const CodeValue *v, const char *requirement) {
    if (v->tag != CODE_BOOL) {
        fail_operand(requirement, v);
        return 0;
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
 * CODE_BOOL, and its value must be true — anything else goes down the
 * failure channel, same as every other operator error here.
 *
 * This is the one phase 4 turns into `return Exception`; nothing about that
 * change lands in this function, only in the block codegen branches to. */
void code_assert(const CodeValue *v) {
    if (v->tag != CODE_BOOL) {
        fail_operand("assert requires a boolean", v);
        return;
    }
    if (!v->boolean) {
        fail("assertion failed");
    }
}
