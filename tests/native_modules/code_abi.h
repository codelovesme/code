/*
 * Code Native Module ABI v2.
 *
 * Include this header in C/C++ native modules to provide the required
 * struct definitions and tag constants.
 */
#ifndef CODE_ABI_H
#define CODE_ABI_H

#include <stdint.h>

/* ---- ABI version (return this from code_module_abi_version) ---- */
#define CODE_ABI_VERSION  2

/* ---- Value tags ---- */
#define CODE_TAG_NUMBER   0
#define CODE_TAG_STRING   1
#define CODE_TAG_BOOLEAN  2
#define CODE_TAG_OBJECT   3
#define CODE_TAG_NULL     4
#define CODE_TAG_ARRAY    5

/* ---- Emission target constants ---- */
#define CODE_EMIT_TARGET_BASE  0

/* ---- Forward declarations ---- */
typedef struct CodeField    CodeField;
typedef struct CodeValue    CodeValue;

/* ---- Value representation ---- */
struct CodeValue {
    uint8_t      tag;
    double       number;       /* valid when tag == CODE_TAG_NUMBER */
    const char  *string;       /* valid when tag == CODE_TAG_STRING */
    uint8_t      boolean;      /* valid when tag == CODE_TAG_BOOLEAN */
    CodeField    *fields;       /* valid when tag == CODE_TAG_OBJECT */
    uint32_t     field_count;
    CodeValue    *elements;     /* valid when tag == CODE_TAG_ARRAY */
    uint32_t     element_count;
};

struct CodeField {
    const char *name;
    CodeValue    value;
};

/* ---- Handler signature ---- */
typedef CodeValue (*CodeNativeHandlerFn)(CodeValue particle);

/* ---- Emit callback (provided by host) ---- */
typedef void (*CodeEmitFn)(void *context, CodeValue particle);

/* ---- Export descriptors ---- */
typedef struct {
    const char *name;
    CodeValue    value;
} CodeExportVar;

typedef struct {
    const char          *class_name;
    CodeNativeHandlerFn   handler;
} CodeExportHandler;

typedef struct {
    const char *name;
    const char *type_name;
    uint8_t     is_optional;
} CodeTypeField;

typedef struct {
    const char   *name;
    CodeTypeField *fields;
    uint32_t      field_count;
} CodeExportType;

/* ---- Emission declaration ---- */
typedef struct {
    const char *class_name;
    uint32_t    target;    /* CODE_EMIT_TARGET_BASE = 0 */
} CodeEmission;

/* ---- Module descriptor ---- */
typedef struct {
    uint32_t          abi_version;
    CodeExportVar     *vars;
    uint32_t          var_count;
    CodeExportHandler *handlers;
    uint32_t          handler_count;
    CodeExportType    *types;
    uint32_t          type_count;
    CodeEmission      *emissions;
    uint32_t          emission_count;
} CodeModuleDesc;

/* ---- Helper macros ---- */
#define CODE_NUMBER(n) ((CodeValue){ .tag = CODE_TAG_NUMBER, .number = (n) })
#define CODE_STRING(s) ((CodeValue){ .tag = CODE_TAG_STRING, .string = (s) })
#define CODE_BOOLEAN(b) ((CodeValue){ .tag = CODE_TAG_BOOLEAN, .boolean = (b) })
#define CODE_NULL()    ((CodeValue){ .tag = CODE_TAG_NULL })

#endif /* CODE_ABI_H */
