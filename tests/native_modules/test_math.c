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

    char msg[96];
    snprintf(msg, sizeof msg, "test_math: unknown handler '%s'", class_val->str);
    code_runtime_error(msg);
}
