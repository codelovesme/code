/* A `.a` static module built for wasm — the smallest one that proves an
 * application and its modules can be linked into a *single* `.wasm`.
 *
 * C rather than Rust deliberately. The wasm runtime is compiled freestanding
 * (`-nostdlib`), and a Rust `staticlib` for wasm32 brings its own standard
 * library and allocator into the same link; that is a real question, and it
 * is not the question this fixture asks. This one asks only whether the
 * static-module contract survives the change of target.
 *
 * The contract is `code_abi.h`'s ".a static modules" section: one flat symbol
 * table, so every entry point carries a prefix the linker step discovers by
 * reading the archive. No runtime of its own — it calls the host's
 * constructors, which are in the same `.wasm`. */

/* `stddef.h` only — a freestanding header the compiler itself provides, so
 * it is there even under `-nostdlib`.
 *
 * Deliberately *not* the runtime's wasm shim. That shim is the runtime's own
 * private libc and it **defines** `memset`; a module that includes it
 * defines a second one, and the link fails on the duplicate. A module needs
 * none of it: `memset` stays undefined here and is resolved by the runtime
 * sitting in the same `.wasm`. */
#include <stddef.h>
#include "code_abi.h"

int wasmmath_code_module_abi_version(void) { return CODE_ABI_VERSION; }

/* `keys`/`items` are right there in the value — see `code_abi.h`, "Four
 * functions were removed": a module reads fields by walking them. */
static const CodeValue *field(const CodeValue *v, const char *name) {
    if (!v || v->tag != CODE_OBJECT) return NULL;
    for (long long i = 0; i < v->len; i++) {
        const char *k = v->keys[i];
        const char *n = name;
        while (*k && *k == *n) { k++; n++; }
        if (*k == 0 && *n == 0) {
            return (const CodeValue *)((char *)v->items + (size_t)i * CODE_VALUE_SLOT_SIZE);
        }
    }
    return NULL;
}

static int is_class(const CodeValue *particle, const char *name) {
    const CodeValue *c = field(particle, "_class");
    if (!c || c->tag != CODE_STR || !c->str) return 0;
    const char *a = c->str;
    while (*a && *a == *name) { a++; name++; }
    return *a == 0 && *name == 0;
}

void wasmmath_code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    if (is_class(particle, "Double")) {
        const CodeValue *v = field(particle, "value");
        double n = (v && v->tag == CODE_NUMBER) ? v->number : 0.0;
        CodeValue answer = {0};
        code_number(&answer, n * 2.0);
        const char *keys[2] = {"_class", "value"};
        _Alignas(8) char slots[2 * CODE_VALUE_SLOT_SIZE] = {0};
        code_str((CodeValue *)slots, "Doubled");
        code_copy((CodeValue *)(slots + CODE_VALUE_SLOT_SIZE), &answer);
        code_object(out, keys, slots, 2);
        code_release((CodeValue *)slots);
        code_release((CodeValue *)(slots + CODE_VALUE_SLOT_SIZE));
        code_release(&answer);
        return;
    }
    code_null(out);
}
