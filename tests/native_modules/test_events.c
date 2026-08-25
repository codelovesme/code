/* A native module that pushes particles *into* the program rather than only
 * answering it — the inbound half of the boundary (see
 * docs/todo/inbound-emissions-from-native-modules.md).
 *
 * `Start { value: n }` queues n `Tick` particles and returns `StartedResult`.
 * The queue and the function that pushes onto it are handed over by the host
 * at link time through `code_module_set_inbound`, which is optional: a module
 * that never speaks first simply doesn't export it.
 *
 * The push function is a *pointer* rather than a direct call to
 * `code_emit_inbound` for the same reason a module exports its own
 * `code_release`: a `.so` carries its own copy of runtime.c, so calling
 * `code_emit_inbound` here would push onto this copy's queue, which the host
 * never reads. See code_abi.h.
 */
#include "../../src/runtime.c"

static void *inbound_queue = NULL;
static CodeEmitFn inbound_emit = NULL;

uint32_t code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}

void code_module_set_inbound(void *queue, CodeEmitFn emit) {
    inbound_queue = queue;
    inbound_emit = emit;
}

void code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    if (particle->tag != CODE_OBJECT) {
        code_runtime_error("test_events: emit requires a particle");
    }
    const CodeValue *class_val = find_field(particle, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        code_runtime_error("test_events: emit requires a particle");
    }

    if (strcmp(class_val->str, "Start") == 0) {
        const CodeValue *value = find_field(particle, "value");
        if (!value || value->tag != CODE_NUMBER) {
            code_runtime_error("Start requires a numeric 'value'");
        }
        long long count = (long long)value->number;
        for (long long i = 0; i < count; i++) {
            CodeValue n = {0};
            code_number(&n, (double)i);
            CodeValue tick = {0};
            code_make_result(&tick, "Tick", &n);
            if (inbound_emit) {
                inbound_emit(inbound_queue, &tick);
            }
            code_release(&tick);
            code_release(&n);
        }
        CodeValue ok = {0};
        code_number(&ok, (double)count);
        code_make_result(out, "StartedResult", &ok);
        code_release(&ok);
        return;
    }

    /* A class this module pushes but never answers — emitting it *to* the
     * module is a mistake worth reporting. */
    char msg[128];
    snprintf(msg, sizeof msg, "test_events: unknown handler '%s'", class_val->str);
    code_runtime_error(msg);
}
