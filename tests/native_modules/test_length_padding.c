/* Pointer members occupy four bytes on wasm32. The other four payload bytes
 * need not be zero. This is a valid empty array with nonzero union padding. */
#include "code_abi.h"
#include <stddef.h>

extern void arrays_code_module_dispatch(CodeValue *, const CodeValue *);
uint32_t padding_code_module_abi_version(void) { return CODE_ABI_VERSION; }

void padding_code_module_dispatch(CodeValue *out, const CodeValue *unused) {
    (void)unused;
    const char *keys[] = {"_class", "value"};
    struct {
        CodeList head;
        CodeValue fields[2];
    } input = {{2, keys}, {{.tag = CODE_STR, .str = "Length"}, {.tag = CODE_ARRAY, .list = NULL}}};
    _Static_assert(offsetof(typeof(input), fields) == sizeof(CodeList), "ABI slot offset");
    for (size_t i = sizeof(void *); i < sizeof(double); ++i)
        ((unsigned char *)&input.fields[1])[offsetof(CodeValue, list) + i] = 0xa5;
    CodeValue particle = {.tag = CODE_OBJECT, .list = &input.head};
    arrays_code_module_dispatch(out, &particle);
}
