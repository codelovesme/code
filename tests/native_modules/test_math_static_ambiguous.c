/* A `.a` whose author picked two prefixes by mistake — used only by
 * `fail_native_link_static_ambiguous.code` to exercise loader.rs's "a .a
 * module's prefix must be unique" check (`static_module_symbols`). Neither
 * function needs to do anything real; `link` never gets far enough to call
 * either. */
#include "../../src/code_abi.h"

uint32_t foo_code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}
void foo_code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    (void)particle;
    code_null(out);
}

uint32_t bar_code_module_abi_version(void) {
    return CODE_ABI_VERSION;
}
void bar_code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    (void)particle;
    code_null(out);
}
