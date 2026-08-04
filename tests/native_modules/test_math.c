/*
 * test_math — A simple Code native module for testing.
 *
 * Exports:
 *   Variables:  PI (Number)
 *   Types:      Point { x: Number, y: Number }
 *   Handlers:   Point (returns the same particle with x+y as "sum" field)
 */

#include "code_abi.h"
#include <string.h>

/* ---- Handler ---- */

static CodeValue handle_point(CodeValue particle) {
    /* Extract x and y from the particle fields, compute sum. */
    double x = 0.0, y = 0.0;
    for (uint32_t i = 0; i < particle.field_count; i++) {
        if (strcmp(particle.fields[i].name, "x") == 0 &&
            particle.fields[i].value.tag == CODE_TAG_NUMBER) {
            x = particle.fields[i].value.number;
        }
        if (strcmp(particle.fields[i].name, "y") == 0 &&
            particle.fields[i].value.tag == CODE_TAG_NUMBER) {
            y = particle.fields[i].value.number;
        }
    }

    /* Build result: Point { _class="Point", x, y, sum } */
    static CodeField result_fields[4];
    result_fields[0] = (CodeField){ .name = "_class", .value = CODE_STRING("Point") };
    result_fields[1] = (CodeField){ .name = "x",      .value = CODE_NUMBER(x) };
    result_fields[2] = (CodeField){ .name = "y",      .value = CODE_NUMBER(y) };
    result_fields[3] = (CodeField){ .name = "sum",    .value = CODE_NUMBER(x + y) };

    return (CodeValue){
        .tag = CODE_TAG_OBJECT,
        .fields = result_fields,
        .field_count = 4,
    };
}

/* ---- Module descriptor (static data) ---- */

static CodeExportVar module_vars[] = {
    { .name = "PI", .value = { .tag = CODE_TAG_NUMBER, .number = 3.14159265358979 } },
};

static CodeTypeField point_fields[] = {
    { .name = "x", .type_name = "Number", .is_optional = 0 },
    { .name = "y", .type_name = "Number", .is_optional = 0 },
};

static CodeExportType module_types[] = {
    { .name = "Point", .fields = point_fields, .field_count = 2 },
};

static CodeExportHandler module_handlers[] = {
    { .class_name = "Point", .handler = handle_point },
};

static CodeModuleDesc module_desc = {
    .abi_version    = 2,
    .vars           = module_vars,
    .var_count      = 1,
    .handlers       = module_handlers,
    .handler_count  = 1,
    .types          = module_types,
    .type_count     = 1,
    .emissions      = NULL,
    .emission_count = 0,
};

/* ---- Exported ABI symbols ---- */

uint32_t code_module_abi_version(void) {
    return 2;
}

const CodeModuleDesc* code_module_init(void) {
    return &module_desc;
}
