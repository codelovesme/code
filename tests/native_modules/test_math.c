/* A native module used only by the fixture harness (tests fixtures that
 * link "native_modules/test_math.so"), covering the shapes a real module
 * will need: a Number result (Double), a fresh heap-owned Str result
 * (Shout), a reduction over an Array (Sum), and unchanged passthrough of
 * whatever `value` was handed in, including nested Array/Object (Echo).
 *
 * `#include`s runtime.c directly rather than re-declaring its constructors —
 * see code_abi.h's doc comment for why that's the intended way to satisfy
 * the "export your own code_release" part of the ABI contract, and it also
 * hands this file every `static` helper runtime.c has (`find_field`,
 * `slot_at`, `code_make_result`, `code_str_owned`) for free.
 */
#include "../../src/runtime.c"

uint32_t code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}

void code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    if (particle->tag != CODE_OBJECT) {
        code_runtime_error("test_math: emit requires a particle");
    }
    const CodeValue *class_val = find_field(particle, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        code_runtime_error("test_math: emit requires a particle");
    }

    if (strcmp(class_val->str, "Double") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value || value->tag != CODE_NUMBER) {
            code_runtime_error("Double requires a numeric 'value'");
        }
        CodeValue doubled = {0};
        code_number(&doubled, value->number * 2.0);
        code_make_result(out, "DoubleResult", &doubled);
        return;
    }

    if (strcmp(class_val->str, "Shout") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value || value->tag != CODE_STR) {
            code_runtime_error("Shout requires a string 'value'");
        }
        size_t n = strlen(value->str);
        char *buf = (char *)malloc(n + 2);
        for (size_t i = 0; i < n; i++) {
            char c = value->str[i];
            buf[i] = (c >= 'a' && c <= 'z') ? (char)(c - 32) : c;
        }
        buf[n] = '!';
        buf[n + 1] = '\0';
        /* A fresh heap-owned value, not a borrowed literal — code_str_owned
         * (not code_str) is what gives it a refcounted block at all. */
        CodeValue shouted = {0};
        code_str_owned(&shouted, buf);
        free(buf);
        code_make_result(out, "ShoutResult", &shouted);
        /* code_make_result's own copy of `shouted` now owns a reference;
         * this drops the one this function still holds. */
        code_release(&shouted);
        return;
    }

    if (strcmp(class_val->str, "Sum") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value || value->tag != CODE_ARRAY) {
            code_runtime_error("Sum requires an array 'value'");
        }
        double total = 0.0;
        for (long long i = 0; i < value->len; i++) {
            const CodeValue *elem = slot_at(value->items, i);
            if (elem->tag != CODE_NUMBER) {
                code_runtime_error("Sum requires an array of numbers");
            }
            total += elem->number;
        }
        CodeValue sum = {0};
        code_number(&sum, total);
        code_make_result(out, "SumResult", &sum);
        return;
    }

    if (strcmp(class_val->str, "Echo") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value) {
            code_runtime_error("Echo requires a 'value' field");
        }
        /* Unchanged passthrough, including nested Array/Object — exercises
         * a handler that shares structure with its input rather than always
         * building something fresh, which every other handler here does. */
        code_make_result(out, "EchoResult", value);
        return;
    }

    /* A class this module does not handle answers null rather than ending
     * the program — whether to act on a particle is the recipient's
     * business (2026-08-28, docs/todo/errors-as-particles.md). */
    code_null(out);
}

/* Exported variables (constants) — what `link "x.so" as m` exposes as
 * `m.<name>`. Built once at load time into static storage; the host deep-
 * copies each value out at `link` time and never frees them (they're the
 * module's own permanent constants, owned for the module's whole lifetime —
 * `code_native_close` never `dlclose`s, so they outlive the object that
 * borrows their key strings). Covers all six value kinds so the
 * deep-copy-at-the-boundary path (`code_native_copy_in` / `ffi_to_value`) is
 * exercised for each. `var_values` is a flat `CODE_VALUE_SLOT_SIZE`-strided
 * buffer, addressed through `slot_at`, never `[]`. */
static const char *var_names[] = {
    "answer",  /* Number */
    "name",    /* Str    */
    "enabled", /* Bool   */
    "nothing", /* Null   */
    "factors", /* Array  */
    "meta",    /* Object */
};
static char var_values[6 * CODE_VALUE_SLOT_SIZE];
static CodeVarList var_list = {
    .count = 6,
    .names = var_names,
    .values = (CodeValue *)var_values,
};

__attribute__((constructor))
static void test_math_init_vars(void) {
    code_number(slot_at(var_values, 0), 42);
    code_str(slot_at(var_values, 1), "test_math");
    code_bool(slot_at(var_values, 2), 1);
    code_null(slot_at(var_values, 3));
    /* factors = [2, 3, 5] */
    {
        char elems[3 * CODE_VALUE_SLOT_SIZE] = {0};
        code_number(slot_at(elems, 0), 2);
        code_number(slot_at(elems, 1), 3);
        code_number(slot_at(elems, 2), 5);
        code_array(slot_at(var_values, 4), elems, 3);
        for (int i = 0; i < 3; i++) {
            code_release(slot_at(elems, i));
        }
    }
    /* meta = { "version": 1, "owner": "test" } */
    {
        const char *keys[] = {"version", "owner"};
        char vals[2 * CODE_VALUE_SLOT_SIZE] = {0};
        code_number(slot_at(vals, 0), 1);
        code_str(slot_at(vals, 1), "test");
        code_object(slot_at(var_values, 5), keys, vals, 2);
        for (int i = 0; i < 2; i++) {
            code_release(slot_at(vals, i));
        }
    }
}

const CodeVarList *code_module_vars(void) {
    return &var_list;
}
