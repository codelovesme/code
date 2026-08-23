/* The `terminal` native module — the native host's console.
 *
 * It exists because the language has no core I/O: a program's only way to say
 * something to a human is to `emit` a particle to a linked module, and this
 * module is where that lands. Being a plain `.so`, `Print` just writes stdout
 * in C — no purity seam in the interpreter, no captured-and-compared output.
 * That last part is deliberate: the harness runs every fixture in both output
 * modes and checks exit status plus leak-freeness, but it does NOT capture or
 * compare stdout, so a module that prints simply prints, in both modes.
 *
 * Deliberately minimal — the same shape as the old language's `console`
 * module (old/tests/native_modules/console.rs), which took a message and
 * printed it straight away. One handler (`Print`), one stream (stdout), and
 * nothing that reaches outside the process except the bytes it writes: no env
 * vars, no subprocess, no TTY sniffing, no color.
 *
 * The particle carries a `value` (this language's uniform field name — see
 * `code_make_result`'s `_class`/`value` shape) and we render it ourselves,
 * because unlike the old language there is no host-side renderer to lean on.
 *
 * `#include`s runtime.c directly rather than re-declaring its constructors —
 * see code_abi.h's doc comment for why that's the intended way to satisfy the
 * "export your own code_release" part of the ABI contract, and it also hands
 * this file every `static` helper runtime.c has (`find_field`, `slot_at`).
 */
#include "../../src/runtime.c"

uint32_t code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}

/* Render a value the way a person expects to read it: strings bare (they're
 * already text — quoting them would make `Print "hi"` show `"hi"`), numbers
 * integral when they have no fractional part (so `Print 5` shows `5`, not
 * `5.0`), everything else as compact JSON. Kept deliberately simple — this is
 * a console, not a serializer. */
static void render_value(const CodeValue *v, char **buf, size_t *cap, size_t *len) {
    switch (v->tag) {
    case CODE_STR: {
        size_t n = strlen(v->str);
        while (*len + n + 1 > *cap) {
            *cap *= 2;
            *buf = realloc(*buf, *cap);
        }
        memcpy(*buf + *len, v->str, n);
        *len += n;
        break;
    }
    case CODE_NUMBER: {
        double d = v->number;
        int is_int = (d == (double)(long long)d) && (llabs((long long)d) < (1LL << 53));
        char tmp[64];
        int t;
        if (is_int) {
            t = snprintf(tmp, sizeof tmp, "%lld", (long long)d);
        } else {
            t = snprintf(tmp, sizeof tmp, "%g", d);
        }
        while (*len + (size_t)t + 1 > *cap) {
            *cap *= 2;
            *buf = realloc(*buf, *cap);
        }
        memcpy(*buf + *len, tmp, (size_t)t);
        *len += (size_t)t;
        break;
    }
    default:
        /* Bool / Null / Array / Object — fall back to a stable, readable form.
         * Arrays and objects are rare things to Print; showing their element
         * count keeps the line honest without pulling in a full JSON encoder.
         * Kept ASCII on purpose: the result reports a byte count, and a
         * multibyte glyph would make that disagree with what's visible. */
        char tmp[64];
        int t;
        if (v->tag == CODE_BOOL) {
            t = snprintf(tmp, sizeof tmp, "%s", v->boolean ? "true" : "false");
        } else if (v->tag == CODE_NULL) {
            t = snprintf(tmp, sizeof tmp, "null");
        } else if (v->tag == CODE_ARRAY) {
            t = snprintf(tmp, sizeof tmp, "[%lld items]", v->len);
        } else {
            t = snprintf(tmp, sizeof tmp, "{%lld fields}", v->len);
        }
        while (*len + (size_t)t + 1 > *cap) {
            *cap *= 2;
            *buf = realloc(*buf, *cap);
        }
        memcpy(*buf + *len, tmp, (size_t)t);
        *len += (size_t)t;
        break;
    }
}

void code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    if (particle->tag != CODE_OBJECT) {
        code_runtime_error("terminal: emit requires a particle");
    }
    const CodeValue *class_val = find_field(particle, "_class");
    if (!class_val || class_val->tag != CODE_STR) {
        code_runtime_error("terminal: emit requires a particle");
    }
    if (strcmp(class_val->str, "Print") != 0) {
        char msg[96];
        snprintf(msg, sizeof msg, "terminal: unknown handler '%s'", class_val->str);
        code_runtime_error(msg);
    }

    const CodeValue *value = find_field(particle, "value");
    if (!value) {
        code_runtime_error("terminal: Print requires a 'value' field");
    }

    char *buf = malloc(256);
    size_t cap = 256;
    size_t len = 0;
    render_value(value, &buf, &cap, &len);

    /* Straight to stdout, newline-terminated, flushed so the line is visible
     * immediately even under pipe buffering. This is the whole module. */
    fwrite(buf, 1, len, stdout);
    fputc('\n', stdout);
    fflush(stdout);
    long long chars = (long long)len;
    free(buf);

    /* Result shape matches the rest of the ecosystem: a particle carrying how
     * many characters landed on the wire, so a program can `assert` on it if
     * it wants to prove the print happened. */
    CodeValue count = {0};
    code_number(&count, (double)chars);
    code_make_result(out, "TerminalResult", &count);
}
