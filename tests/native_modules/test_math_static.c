/* A `.a`-flavored native module used only by the fixture harness (tests
 * fixtures that `link "native_modules/test_math_static.a"`). Unlike
 * `test_math.c` (the `.so` example, which `#include`s runtime.c wholesale
 * and brings its own copy of the runtime), this module calls the *host's*
 * own constructors directly, declared `extern` in code_abi.h — see that
 * header's ".a static modules" section for why: a `.a` links straight into
 * the same binary as the host, so there is only ever one copy of the
 * runtime, and no deep-copy boundary to cross.
 *
 * Its three entry points are named with the `testmath_` prefix — chosen by
 * this file, unique among every `.a` a program might ever link alongside —
 * so `code build` can find them by running `nm` on the archive (see
 * `loader.rs`'s `static_module_symbols`).
 */
#include <string.h>

#include "../../src/code_abi.h"

uint32_t testmath_code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}

void testmath_code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    if (particle->tag != CODE_OBJECT) {
        code_runtime_error("test_math_static: emit requires a particle");
    }
    const CodeValue *class_val = NULL;
    for (long long i = 0; i < particle->len; i++) {
        if (strcmp(particle->keys[i], "_class") == 0) {
            class_val = code_slot_at(particle->items, i);
            break;
        }
    }
    if (!class_val || class_val->tag != CODE_STR) {
        code_runtime_error("test_math_static: emit requires a particle");
    }

    if (strcmp(class_val->str, "Sum") == 0) {
        const CodeValue *value = NULL;
        for (long long i = 0; i < particle->len; i++) {
            if (strcmp(particle->keys[i], "value") == 0) {
                value = code_slot_at(particle->items, i);
                break;
            }
        }
        if (!value || value->tag != CODE_ARRAY) {
            code_runtime_error("Sum requires an array 'value'");
        }
        double total = 0.0;
        for (long long i = 0; i < value->len; i++) {
            const CodeValue *elem = code_slot_at(value->items, i);
            if (elem->tag != CODE_NUMBER) {
                code_runtime_error("Sum requires an array of numbers");
            }
            total += elem->number;
        }
        CodeValue sum = {0};
        code_number(&sum, total);
        const char *result_keys[] = {"_class", "value"};
        char slots[2 * CODE_VALUE_SLOT_SIZE] = {0};
        code_str(code_slot_at(slots, 0), "SumResult");
        code_copy(code_slot_at(slots, 1), &sum);
        code_object(out, result_keys, slots, 2);
        code_release(code_slot_at(slots, 0));
        code_release(code_slot_at(slots, 1));
        return;
    }

    code_runtime_error("test_math_static: unknown handler");
}

/* Exported variables (constants) — what `link "x.a" as m` exposes as
 * `m.<name>`. Built once (a plain static initializer, no constructor
 * attribute needed: unlike the `.so` case, this runs as part of the host's
 * own `main`, well after any use, so there's no load-order subtlety) into
 * static storage owned by this translation unit for the program's whole
 * lifetime — the host only ever retains references into it (see
 * `code_static_vars_object` in runtime.c), never frees it. */
static char var_values[1 * CODE_VALUE_SLOT_SIZE];
static const char *var_names[] = {"answer"};
static CodeVarList var_list = {
    .count = 1,
    .names = var_names,
    .values = (CodeValue *)var_values,
};

const CodeVarList *testmath_code_module_vars(void) {
    static int initialized = 0;
    if (!initialized) {
        code_number(code_slot_at(var_values, 0), 42);
        initialized = 1;
    }
    return &var_list;
}
